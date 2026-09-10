//! extern "C" wrapper for `dx`, mirroring the core `tulip_rs` crate's
//! `Dx::indicator` / `IndicatorState::batch_indicator` interface. See
//! `adosc.rs` for the general Tulip-Indicators-style `inputs`/`options`
//! calling convention this follows, including the parameter-order rule
//! (every pointer parameter is immediately followed by the count(s) that
//! describe it).
//!
//! SIMD entry points:
//! - `dx_simd_by_assets`: compute DX for N assets simultaneously,
//!   sharing a single options array. N must be 2, 4, 8, or 16.
//! - `dx_simd_by_options`: compute DX for one asset with N different
//!   option sets simultaneously. N must be 2, 4, 8, or 16.
//!
//! `#[no_mangle]` functions can't be generic, so each SIMD entry point is a
//! thin non-generic dispatcher that matches the runtime `N` against
//! {2,4,8,16} and calls a private generic `_n::<N>` helper that does the
//! real work.

use std::os::raw::c_void;

use tulip_rs::indicator_types::{Indicator, IndicatorByOptions, TIndicatorState};
use tulip_rs::indicators::dx::{Dx, IndicatorState as DxState, INPUTS, OPTIONS};
use tulip_rs::types::IndicatorError;

use crate::common::{
    optional_outputs_slice, pack_outputs, pack_simd_outputs, pack_states, read_inputs,
    read_simd_assets_inputs, read_simd_options, CBatchResult, CIndicatorError, CIndicatorResult,
    CSimdResult,
};

/// Opaque state handle returned by `dx_indicator()` and consumed by
/// `dx_batch()` / `dx_state_free()`.
pub type DxStateHandle = DxState;

/// Runs `dx` over `data_len` bars.
///
/// `inputs` must point to `INPUTS` (3) pointers, in order:
/// `high, low, close`, each `data_len` `f64`s long.
/// `options` must point to `OPTIONS` (1) value: `period`.
///
/// Returns the mandatory `dx` output plus any requested optional outputs
/// (`atr`, `tr`, in that fixed order) and a fresh continuation state.
///
/// # Safety
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `num_optional` valid
///   `bool`s (pass null + 0 to request no optional outputs).
#[no_mangle]
pub unsafe extern "C" fn dx_indicator(
    inputs: *const *const f64,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    num_optional: usize,
) -> CIndicatorResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    let optional = optional_outputs_slice(optional_outputs, num_optional);

    match Dx::indicator(&inputs, &options, optional) {
        Ok((rows, state)) => {
            let (outputs, output_lens, num_outputs) = pack_outputs(rows);
            let state = Box::into_raw(Box::new(state)) as *mut c_void;
            CIndicatorResult {
                error: CIndicatorError::Ok,
                outputs,
                output_lens,
                num_outputs,
                state,
            }
        }
        Err(e) => CIndicatorResult::err(e),
    }
}

/// Continues a `dx` calculation from `state`, mutating it in place so
/// it's ready for the next call. `state` must have come from
/// `dx_indicator()` and not yet have been passed to `dx_state_free()`.
///
/// `inputs` must point to `INPUTS` (3) pointers (`high, low, close`), each
/// `data_len` `f64`s long.
///
/// # Safety
/// - `state` must be a live pointer previously returned by
///   `dx_indicator()`.
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `num_optional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn dx_batch(
    state: *mut c_void,
    inputs: *const *const f64,
    data_len: usize,
    optional_outputs: *const bool,
    num_optional: usize,
) -> CBatchResult {
    if state.is_null() {
        return CBatchResult::err(IndicatorError::InvalidIndicatorState);
    }
    let state = &mut *(state as *mut DxStateHandle);

    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let optional = optional_outputs_slice(optional_outputs, num_optional);

    match state.batch_indicator(&inputs, optional) {
        Ok(rows) => {
            let (outputs, output_lens, num_outputs) = pack_outputs(rows);
            CBatchResult {
                error: CIndicatorError::Ok,
                outputs,
                output_lens,
                num_outputs,
            }
        }
        Err(e) => CBatchResult::err(e),
    }
}

/// Frees a state handle returned by `dx_indicator()` (or one of the
/// `states[i]` entries from `dx_simd_by_assets()`/`dx_simd_by_options()`).
/// Call this once you're done streaming (after your last `dx_batch()` call).
///
/// # Safety
/// `state` must be a live pointer previously returned by `dx_indicator()`
/// or found in a `CSimdResult::states` array, and must not be used again
/// after this call.
#[no_mangle]
pub unsafe extern "C" fn dx_state_free(state: *mut c_void) {
    if !state.is_null() {
        drop(Box::from_raw(state as *mut DxStateHandle));
    }
}

/// Computes DX for `N` assets simultaneously (SIMD), sharing a single
/// options array. `num_assets` must be 2, 4, 8, or 16 -- anything else
/// returns `InvalidInputs`.
///
/// `inputs` must point to `num_assets` pointers, each itself pointing to
/// `INPUTS` (3) pointers (`high, low, close`), each `data_len` `f64`s long.
/// `options` must point to `OPTIONS` (1) shared value.
///
/// Returns a `CSimdResult` with `num_assets` results, each with its own
/// output rows and its own ordinary `dx` continuation state (reusable
/// with `dx_batch()`/`dx_state_free()`). Free each state via
/// `dx_state_free()`, then free the rest via `tulip_ffi_simd_result_free()`.
///
/// # Safety
/// - `inputs` must point to `num_assets` valid pointers, each pointing to
///   `INPUTS` valid non-null `*const f64`s, each pointing to `data_len` valid
///   `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `num_optional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn dx_simd_by_assets(
    inputs: *const *const *const f64,
    num_assets: usize,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    num_optional: usize,
) -> CSimdResult {
    match num_assets {
        2 => dx_simd_by_assets_n::<2>(inputs, data_len, options, optional_outputs, num_optional),
        4 => dx_simd_by_assets_n::<4>(inputs, data_len, options, optional_outputs, num_optional),
        8 => dx_simd_by_assets_n::<8>(inputs, data_len, options, optional_outputs, num_optional),
        16 => dx_simd_by_assets_n::<16>(inputs, data_len, options, optional_outputs, num_optional),
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn dx_simd_by_assets_n<const N: usize>(
    inputs: *const *const *const f64,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    num_optional: usize,
) -> CSimdResult {
    // `owned` holds the per-asset input slices; `refs` borrows from it, so
    // both must live in this stack frame for the duration of the call.
    let owned = read_simd_assets_inputs::<N, INPUTS>(inputs, data_len);
    let refs: [&[&[f64]; INPUTS]; N] = std::array::from_fn(|i| &owned[i]);
    let options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    let optional = optional_outputs_slice(optional_outputs, num_optional);

    match Dx::indicator_by_assets::<N>(&refs, &options, optional) {
        Ok((results, states)) => {
            let (outputs, output_lens, num_outputs, num_results) = pack_simd_outputs(results);
            let states = pack_states(states);
            CSimdResult {
                error: CIndicatorError::Ok,
                outputs,
                output_lens,
                num_outputs,
                states,
                num_results,
            }
        }
        Err(e) => CSimdResult::err(e),
    }
}

/// Computes DX for one asset with `N` different option sets
/// simultaneously (SIMD). `num_option_sets` must be 2, 4, 8, or 16 --
/// anything else returns `InvalidInputs`.
///
/// `inputs` must point to `INPUTS` (3) pointers (`high, low, close`), each
/// `data_len` `f64`s long, shared across all option sets. `options` must point
/// to `num_option_sets` pointers, each itself pointing to `OPTIONS` (1)
/// value.
///
/// Returns a `CSimdResult` with `num_option_sets` results, each with its
/// own output rows and its own ordinary `dx` continuation state
/// (reusable with `dx_batch()`/`dx_state_free()`). Free each state via
/// `dx_state_free()`, then free the rest via
/// `tulip_ffi_simd_result_free()`.
///
/// # Safety
/// - `inputs` must point to `INPUTS` valid non-null `*const f64`s, each
///   pointing to `data_len` valid `f64`s.
/// - `options` must point to `num_option_sets` valid pointers, each
///   pointing to `OPTIONS` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `num_optional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn dx_simd_by_options(
    inputs: *const *const f64,
    data_len: usize,
    options: *const *const f64,
    num_option_sets: usize,
    optional_outputs: *const bool,
    num_optional: usize,
) -> CSimdResult {
    match num_option_sets {
        2 => dx_simd_by_options_n::<2>(inputs, data_len, options, optional_outputs, num_optional),
        4 => dx_simd_by_options_n::<4>(inputs, data_len, options, optional_outputs, num_optional),
        8 => dx_simd_by_options_n::<8>(inputs, data_len, options, optional_outputs, num_optional),
        16 => dx_simd_by_options_n::<16>(inputs, data_len, options, optional_outputs, num_optional),
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn dx_simd_by_options_n<const N: usize>(
    inputs: *const *const f64,
    data_len: usize,
    options: *const *const f64,
    optional_outputs: *const bool,
    num_optional: usize,
) -> CSimdResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let options = read_simd_options::<N, OPTIONS>(options);
    let optional = optional_outputs_slice(optional_outputs, num_optional);

    match Dx::indicator_by_options::<N>(&inputs, &options, optional) {
        Ok((results, states)) => {
            let (outputs, output_lens, num_outputs, num_results) = pack_simd_outputs(results);
            let states = pack_states(states);
            CSimdResult {
                error: CIndicatorError::Ok,
                outputs,
                output_lens,
                num_outputs,
                states,
                num_results,
            }
        }
        Err(e) => CSimdResult::err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::{
        tulip_ffi_batch_result_free, tulip_ffi_result_free, tulip_ffi_simd_result_free,
    };

    #[test]
    fn test_dx_indicator() {
        unsafe {
            // Create test data: high, low, close prices
            let high = vec![10.5, 11.5, 12.5, 13.5, 14.5, 15.5];
            let low = vec![9.5, 10.5, 11.5, 12.5, 13.5, 14.5];
            let close = vec![10.0, 11.0, 12.0, 13.0, 14.0, 15.0];

            let inputs_array = [high.as_ptr(), low.as_ptr(), close.as_ptr()];
            let inputs_ptr = inputs_array.as_ptr();

            // Options: period=3
            let options = [3.0];
            let options_ptr = options.as_ptr();

            // No optional outputs
            let result = dx_indicator(inputs_ptr, high.len(), options_ptr, std::ptr::null(), 0);

            assert_eq!(result.error, CIndicatorError::Ok);
            // dx has 1 mandatory + 2 optional outputs = 3 total
            assert_eq!(result.num_outputs, 3);

            // Free results
            tulip_ffi_result_free(result);
        }
    }

    #[test]
    fn test_dx_batch() {
        unsafe {
            // Create test data: high, low, close prices
            let high = vec![10.5, 11.5, 12.5, 13.5, 14.5, 15.5];
            let low = vec![9.5, 10.5, 11.5, 12.5, 13.5, 14.5];
            let close = vec![10.0, 11.0, 12.0, 13.0, 14.0, 15.0];

            let inputs_array = [high.as_ptr(), low.as_ptr(), close.as_ptr()];
            let inputs_ptr = inputs_array.as_ptr();

            // Options: period=3
            let options = [3.0];
            let options_ptr = options.as_ptr();

            // First call to get state
            let result = dx_indicator(inputs_ptr, high.len(), options_ptr, std::ptr::null(), 0);
            assert_eq!(result.error, CIndicatorError::Ok);
            let state = result.state;

            // Second call with batch
            let high2 = vec![16.5, 17.5];
            let low2 = vec![15.5, 16.5];
            let close2 = vec![16.0, 17.0];
            let inputs_array2 = [high2.as_ptr(), low2.as_ptr(), close2.as_ptr()];
            let inputs_ptr2 = inputs_array2.as_ptr();

            let batch_result = dx_batch(state, inputs_ptr2, high2.len(), std::ptr::null(), 0);

            assert_eq!(batch_result.error, CIndicatorError::Ok);
            // dx has 1 mandatory + 2 optional outputs = 3 total
            assert_eq!(batch_result.num_outputs, 3);

            // Free results
            tulip_ffi_batch_result_free(batch_result);
            dx_state_free(state);
        }
    }

    #[test]
    fn test_dx_simd_by_assets() {
        unsafe {
            // Two assets, each with high, low, close prices
            let high1 = vec![10.5, 11.5, 12.5, 13.5];
            let low1 = vec![9.5, 10.5, 11.5, 12.5];
            let close1 = vec![10.0, 11.0, 12.0, 13.0];

            let high2 = vec![20.5, 21.5, 22.5, 23.5];
            let low2 = vec![19.5, 20.5, 21.5, 22.5];
            let close2 = vec![20.0, 21.0, 22.0, 23.0];

            // Asset inputs: each asset has INPUTS(3) pointers
            let assets_inputs1 = [high1.as_ptr(), low1.as_ptr(), close1.as_ptr()];
            let assets_inputs2 = [high2.as_ptr(), low2.as_ptr(), close2.as_ptr()];
            // Create array of pointers to each asset's input array
            let ptr_to_assets1 = &assets_inputs1 as *const [_; 3] as *const *const f64;
            let ptr_to_assets2 = &assets_inputs2 as *const [_; 3] as *const *const f64;
            let asset_pointers = [ptr_to_assets1, ptr_to_assets2];
            let inputs_ptr = asset_pointers.as_ptr();

            // Shared options: period=3
            let options = [3.0];
            let options_ptr = options.as_ptr();

            let result =
                dx_simd_by_assets(inputs_ptr, 2, high1.len(), options_ptr, std::ptr::null(), 0);

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, 2);
            // dx has 1 mandatory + 2 optional outputs = 3 total
            assert_eq!(result.num_outputs, 3);

            // Free results
            tulip_ffi_simd_result_free(result);
        }
    }

    #[test]
    fn test_dx_simd_by_options() {
        unsafe {
            // Single asset with high, low, close prices (need enough data for period=5)
            let high = vec![10.5, 11.5, 12.5, 13.5, 14.5, 15.5, 16.5, 17.5];
            let low = vec![9.5, 10.5, 11.5, 12.5, 13.5, 14.5, 15.5, 16.5];
            let close = vec![10.0, 11.0, 12.0, 13.0, 14.0, 15.0, 16.0, 17.0];

            let inputs_array = [high.as_ptr(), low.as_ptr(), close.as_ptr()];
            let inputs_ptr = inputs_array.as_ptr();

            // Two different option sets
            let options1 = [3.0];
            let options2 = [5.0];

            let options1_ptr = &options1 as *const [f64; OPTIONS] as *const f64;
            let options2_ptr = &options2 as *const [f64; OPTIONS] as *const f64;
            let options_ptrs = [options1_ptr, options2_ptr];
            let options_ptr = options_ptrs.as_ptr();

            let result =
                dx_simd_by_options(inputs_ptr, high.len(), options_ptr, 2, std::ptr::null(), 0);

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, 2);
            // dx has 1 mandatory + 2 optional outputs = 3 total
            assert_eq!(result.num_outputs, 3);

            // Free results
            tulip_ffi_simd_result_free(result);
        }
    }
}
