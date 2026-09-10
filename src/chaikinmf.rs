//! extern "C" wrapper for `chaikinmf`, mirroring the core `tulip_rs` crate's
//! `ChaikinMf::indicator` / `IndicatorState::batch_indicator` interface. See
//! `adosc.rs` for the general Tulip-Indicators-style `inputs`/`options`
//! calling convention this follows, including the parameter-order rule
//! (every pointer parameter is immediately followed by the count(s) that
//! describe it).
//!
//! SIMD entry points:
//! - `chaikinmf_simd_by_assets`: compute Chaikin Money Flow for N assets simultaneously,
//!   sharing a single options array. N must be 2, 4, 8, or 16.
//! - `chaikinmf_simd_by_options`: compute Chaikin Money Flow for one asset with N different
//!   option sets simultaneously. N must be 2, 4, 8, or 16.
//!
//! `#[no_mangle]` functions can't be generic, so each SIMD entry point is a
//! thin non-generic dispatcher that matches the runtime `N` against
//! {2,4,8,16} and calls a private generic `_n::<N>` helper that does the
//! real work.

use std::os::raw::c_void;

use tulip_rs::indicator_types::{Indicator, IndicatorByOptions, TIndicatorState};
use tulip_rs::indicators::chaikinmf::{
    ChaikinMf, IndicatorState as ChaikinMfState, INPUTS, OPTIONS,
};
use tulip_rs::types::IndicatorError;

use crate::common::{
    optional_outputs_slice, pack_outputs, pack_simd_outputs, pack_states, read_inputs,
    read_simd_assets_inputs, read_simd_options, CBatchResult, CIndicatorError, CIndicatorResult,
    CSimdResult,
};

/// Opaque state handle returned by `chaikinmf_indicator()` and consumed by
/// `chaikinmf_batch()` / `chaikinmf_state_free()`.
pub type ChaikinMfStateHandle = ChaikinMfState;

/// Runs `chaikinmf` over `data_len` bars.
///
/// `inputs` must point to `INPUTS` (4) pointers, in order:
/// `high, low, close, volume`, each `data_len` `f64`s long.
/// `options` must point to `OPTIONS` (1) value: `period`.
///
/// Returns the mandatory `cmf` output plus any requested optional outputs
/// and a fresh continuation state.
///
/// # Safety
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `num_optional` valid
///   `bool`s (pass null + 0 to request no optional outputs).
#[no_mangle]
pub unsafe extern "C" fn chaikinmf_indicator(
    inputs: *const *const f64,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    num_optional: usize,
) -> CIndicatorResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    let optional = optional_outputs_slice(optional_outputs, num_optional);

    match ChaikinMf::indicator(&inputs, &options, optional) {
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

/// Continues a `chaikinmf` calculation from `state`, mutating it in place so
/// it's ready for the next call. `state` must have come from
/// `chaikinmf_indicator()` and not yet have been passed to `chaikinmf_state_free()`.
///
/// `inputs` must point to `INPUTS` (4) pointers (`high, low, close, volume`),
/// each `data_len` `f64`s long.
///
/// # Safety
/// - `state` must be a live pointer previously returned by
///   `chaikinmf_indicator()`.
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `num_optional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn chaikinmf_batch(
    state: *mut c_void,
    inputs: *const *const f64,
    data_len: usize,
    optional_outputs: *const bool,
    num_optional: usize,
) -> CBatchResult {
    if state.is_null() {
        return CBatchResult::err(IndicatorError::InvalidIndicatorState);
    }
    let state = &mut *(state as *mut ChaikinMfStateHandle);

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

/// Frees a state handle returned by `chaikinmf_indicator()` (or one of the
/// `states[i]` entries from `chaikinmf_simd_by_assets()`/`chaikinmf_simd_by_options()`).
/// Call this once you're done streaming (after your last `chaikinmf_batch()` call).
///
/// # Safety
/// `state` must be a live pointer previously returned by `chaikinmf_indicator()`
/// or found in a `CSimdResult::states` array, and must not be used again
/// after this call.
#[no_mangle]
pub unsafe extern "C" fn chaikinmf_state_free(state: *mut c_void) {
    if !state.is_null() {
        drop(Box::from_raw(state as *mut ChaikinMfStateHandle));
    }
}

/// Computes Chaikin Money Flow for `N` assets simultaneously (SIMD), sharing a single
/// options array. `num_assets` must be 2, 4, 8, or 16 -- anything else
/// returns `InvalidInputs`.
///
/// `inputs` must point to `num_assets` pointers, each itself pointing to
/// `INPUTS` (4) pointers (`high, low, close, volume`), each `data_len` `f64`s long.
/// `options` must point to `OPTIONS` (1) shared values.
///
/// Returns a `CSimdResult` with `num_assets` results, each with its own
/// output rows and its own ordinary `chaikinmf` continuation state (reusable
/// with `chaikinmf_batch()`/`chaikinmf_state_free()`). Free each state via
/// `chaikinmf_state_free()`, then free the rest via `tulip_ffi_simd_result_free()`.
///
/// # Safety
/// - `inputs` must point to `num_assets` valid pointers, each pointing to
///   `INPUTS` valid non-null `*const f64`s, each pointing to `data_len` valid
///   `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `num_optional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn chaikinmf_simd_by_assets(
    inputs: *const *const *const f64,
    num_assets: usize,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    num_optional: usize,
) -> CSimdResult {
    match num_assets {
        2 => chaikinmf_simd_by_assets_n::<2>(
            inputs,
            data_len,
            options,
            optional_outputs,
            num_optional,
        ),
        4 => chaikinmf_simd_by_assets_n::<4>(
            inputs,
            data_len,
            options,
            optional_outputs,
            num_optional,
        ),
        8 => chaikinmf_simd_by_assets_n::<8>(
            inputs,
            data_len,
            options,
            optional_outputs,
            num_optional,
        ),
        16 => chaikinmf_simd_by_assets_n::<16>(
            inputs,
            data_len,
            options,
            optional_outputs,
            num_optional,
        ),
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn chaikinmf_simd_by_assets_n<const N: usize>(
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

    match ChaikinMf::indicator_by_assets::<N>(&refs, &options, optional) {
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

/// Computes Chaikin Money Flow for one asset with `N` different option sets
/// simultaneously (SIMD). `num_option_sets` must be 2, 4, 8, or 16 --
/// anything else returns `InvalidInputs`.
///
/// `inputs` must point to `INPUTS` (4) pointers (`high, low, close, volume`),
/// each `data_len` `f64`s long, shared across all option sets. `options`
/// must point to `num_option_sets` pointers, each itself pointing to
/// `OPTIONS` (1) values.
///
/// Returns a `CSimdResult` with `num_option_sets` results, each with its
/// own output rows and its own ordinary `chaikinmf` continuation state
/// (reusable with `chaikinmf_batch()`/`chaikinmf_state_free()`). Free each state via
/// `chaikinmf_state_free()`, then free the rest via
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
pub unsafe extern "C" fn chaikinmf_simd_by_options(
    inputs: *const *const f64,
    data_len: usize,
    options: *const *const f64,
    num_option_sets: usize,
    optional_outputs: *const bool,
    num_optional: usize,
) -> CSimdResult {
    match num_option_sets {
        2 => chaikinmf_simd_by_options_n::<2>(
            inputs,
            data_len,
            options,
            optional_outputs,
            num_optional,
        ),
        4 => chaikinmf_simd_by_options_n::<4>(
            inputs,
            data_len,
            options,
            optional_outputs,
            num_optional,
        ),
        8 => chaikinmf_simd_by_options_n::<8>(
            inputs,
            data_len,
            options,
            optional_outputs,
            num_optional,
        ),
        16 => chaikinmf_simd_by_options_n::<16>(
            inputs,
            data_len,
            options,
            optional_outputs,
            num_optional,
        ),
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn chaikinmf_simd_by_options_n<const N: usize>(
    inputs: *const *const f64,
    data_len: usize,
    options: *const *const f64,
    optional_outputs: *const bool,
    num_optional: usize,
) -> CSimdResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let options = read_simd_options::<N, OPTIONS>(options);
    let optional = optional_outputs_slice(optional_outputs, num_optional);

    match ChaikinMf::indicator_by_options::<N>(&inputs, &options, optional) {
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
    fn test_chaikinmf_indicator() {
        unsafe {
            // Create test data
            let data_len = 60;
            let high: Vec<f64> = (0..data_len).map(|i| i as f64 + 105.0).collect();
            let low: Vec<f64> = (0..data_len).map(|i| i as f64 + 95.0).collect();
            let close: Vec<f64> = (0..data_len).map(|i| i as f64 + 100.0).collect();
            let volume: Vec<f64> = (0..data_len).map(|i| i as f64 * 100.0).collect();

            let inputs = [high.as_ptr(), low.as_ptr(), close.as_ptr(), volume.as_ptr()];
            let options: [f64; OPTIONS] = [20.0];

            let result = chaikinmf_indicator(
                inputs.as_ptr(),
                data_len,
                options.as_ptr(),
                std::ptr::null(),
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_outputs, 1);

            // Check output length
            let lens = std::slice::from_raw_parts(result.output_lens, result.num_outputs);
            assert_eq!(lens[0], data_len - 20); // cmf

            // Free outputs and state
            tulip_ffi_result_free(result);
        }
    }

    #[test]
    fn test_chaikinmf_batch() {
        unsafe {
            let data_len = 60;
            let high: Vec<f64> = (0..data_len).map(|i| i as f64 + 105.0).collect();
            let low: Vec<f64> = (0..data_len).map(|i| i as f64 + 95.0).collect();
            let close: Vec<f64> = (0..data_len).map(|i| i as f64 + 100.0).collect();
            let volume: Vec<f64> = (0..data_len).map(|i| i as f64 * 100.0).collect();

            let inputs = [high.as_ptr(), low.as_ptr(), close.as_ptr(), volume.as_ptr()];
            let options: [f64; OPTIONS] = [20.0];

            // First call to indicator
            let result = chaikinmf_indicator(
                inputs.as_ptr(),
                data_len,
                options.as_ptr(),
                std::ptr::null(),
                0,
            );
            assert_eq!(result.error, CIndicatorError::Ok);

            // Second call to batch with new data
            let more_high: Vec<f64> = (0..10).map(|i| i as f64 + 205.0).collect();
            let more_low: Vec<f64> = (0..10).map(|i| i as f64 + 195.0).collect();
            let more_close: Vec<f64> = (0..10).map(|i| i as f64 + 200.0).collect();
            let more_volume: Vec<f64> = (0..10).map(|i| i as f64 * 200.0).collect();

            let new_inputs = [
                more_high.as_ptr(),
                more_low.as_ptr(),
                more_close.as_ptr(),
                more_volume.as_ptr(),
            ];

            let batch_result =
                chaikinmf_batch(result.state, new_inputs.as_ptr(), 10, std::ptr::null(), 0);
            assert_eq!(batch_result.error, CIndicatorError::Ok);

            // Free outputs
            tulip_ffi_batch_result_free(batch_result);
            chaikinmf_state_free(result.state);
        }
    }

    #[test]
    fn test_chaikinmf_simd_by_assets() {
        unsafe {
            let data_len = 60;
            let num_assets = 2;

            // Create inputs for 2 assets
            let high1: Vec<f64> = (0..data_len).map(|i| i as f64 + 105.0).collect();
            let low1: Vec<f64> = (0..data_len).map(|i| i as f64 + 95.0).collect();
            let close1: Vec<f64> = (0..data_len).map(|i| i as f64 + 100.0).collect();
            let volume1: Vec<f64> = (0..data_len).map(|i| i as f64 * 100.0).collect();

            let high2: Vec<f64> = (0..data_len).map(|i| i as f64 + 155.0).collect();
            let low2: Vec<f64> = (0..data_len).map(|i| i as f64 + 145.0).collect();
            let close2: Vec<f64> = (0..data_len).map(|i| i as f64 + 150.0).collect();
            let volume2: Vec<f64> = (0..data_len).map(|i| i as f64 * 150.0).collect();

            let inputs1 = [
                high1.as_ptr(),
                low1.as_ptr(),
                close1.as_ptr(),
                volume1.as_ptr(),
            ];
            let inputs2 = [
                high2.as_ptr(),
                low2.as_ptr(),
                close2.as_ptr(),
                volume2.as_ptr(),
            ];

            let asset_inputs = [inputs1.as_ptr(), inputs2.as_ptr()];
            let options: [f64; OPTIONS] = [20.0];

            let result = chaikinmf_simd_by_assets(
                asset_inputs.as_ptr(),
                num_assets,
                data_len,
                options.as_ptr(),
                std::ptr::null(),
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, num_assets);
            assert_eq!(result.num_outputs, 1);

            // Free outputs and states
            tulip_ffi_simd_result_free(result);
        }
    }

    #[test]
    fn test_chaikinmf_simd_by_options() {
        unsafe {
            let data_len = 60;
            let num_option_sets = 2;

            let high: Vec<f64> = (0..data_len).map(|i| i as f64 + 105.0).collect();
            let low: Vec<f64> = (0..data_len).map(|i| i as f64 + 95.0).collect();
            let close: Vec<f64> = (0..data_len).map(|i| i as f64 + 100.0).collect();
            let volume: Vec<f64> = (0..data_len).map(|i| i as f64 * 100.0).collect();

            let inputs = [high.as_ptr(), low.as_ptr(), close.as_ptr(), volume.as_ptr()];

            // Two different option sets
            let options1: [f64; OPTIONS] = [20.0];
            let options2: [f64; OPTIONS] = [15.0];

            let option_sets = [options1.as_ptr(), options2.as_ptr()];

            let result = chaikinmf_simd_by_options(
                inputs.as_ptr(),
                data_len,
                option_sets.as_ptr(),
                num_option_sets,
                std::ptr::null(),
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, num_option_sets);
            assert_eq!(result.num_outputs, 1);

            // Free outputs and states
            tulip_ffi_simd_result_free(result);
        }
    }
}
