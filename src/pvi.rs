//! extern "C" wrapper for `pvi`, mirroring the core `tulip_rs` crate's
//! `Pvi::indicator` / `IndicatorState::batch_indicator` interface with a
//! Tulip-Indicators-style C calling convention: `inputs` is an array of
//! `INPUTS` pointers (one per input series, each `data_len` long, in the same
//! order as `tulip_rs::indicators::pvi::Pvi::INFO.inputs`), `options`
//! is a flat array of `OPTIONS` values -- exactly like `ti_pvi`'s
//! `double const *const *inputs, double const *options`. Output allocation
//! stays inside the indicator function (never caller-supplied).
//!
//! Parameter order convention (kept consistent across every function in
//! this crate): each pointer parameter is immediately followed by the
//! count(s) that describe it, e.g. `inputs, data_len, options, ...,
//! optional_outputs, numoptional`.
//!
//! SIMD entry points:
//! - `pvi_simd_by_assets`: compute PVI for N assets simultaneously,
//!   sharing a single options array. N must be 2, 4, 8, or 16.
//!
//! `#[no_mangle]` functions can't be generic, so each SIMD entry point is a
//! thin non-generic dispatcher that matches the runtime `N` against
//! {2,4,8,16} and calls a private generic `_n::<N>` helper that does the
//! real work.
//!
//! NOTE: PVI has OPTIONS=0, so there is no `pvi_simd_by_options` function.

use std::os::raw::c_void;

use tulip_rs::indicator_types::{Indicator, TIndicatorState};
use tulip_rs::indicators::pvi::{IndicatorState as PviState, Pvi, INPUTS};
use tulip_rs::types::IndicatorError;

use crate::common::{
    optional_outputs_slice, pack_info, pack_outputs, pack_simd_outputs, pack_states, read_inputs,
    read_simd_assets_inputs, CBatchResult, CIndicatorError, CIndicatorInfo, CIndicatorResult,
    CSimdResult,
};

/// Opaque state handle returned by `pvi_indicator()` and consumed by
/// `pvi_batch()` / `pvi_state_free()`.
pub type PviStateHandle = PviState;

/// Returns static metadata about the `pvi` indicator: its name, input
/// names, option names, and (mandatory/optional) output names, mirroring
/// `Pvi::INFO`.
///
/// The returned strings are leaked, process-lifetime C strings -- read them,
/// don't free them.
#[no_mangle]
pub extern "C" fn pvi_info() -> CIndicatorInfo {
    pack_info(&Pvi::INFO)
}

/// Returns the minimum number of bars `pvi` needs to produce any output at
/// all, given `options`.
///
/// # Safety
/// `options` must point to `OPTIONS` (0) valid `f64`s.
#[no_mangle]
pub unsafe extern "C" fn pvi_min_data(_options: *const f64) -> usize {
    Pvi::min_data(&[])
}

/// Runs `pvi` over `data_len` bars.
///
/// `inputs` must point to `INPUTS` (2) pointers, in order:
/// `close, volume`, each `data_len` `f64`s long.
/// `options` is ignored (PVI has no options).
///
/// Returns the mandatory `pvi` output and a fresh continuation state.
///
/// # Safety
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s (pass null + 0 to request no optional outputs).
#[no_mangle]
pub unsafe extern "C" fn pvi_indicator(
    inputs: *const *const f64,
    data_len: usize,
    _options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CIndicatorResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match Pvi::indicator(&inputs, &[], optional) {
        Ok((rows, state)) => {
            let (outputs, output_lens, num_outputs) = pack_outputs(rows, optional);
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

/// Continues a `pvi` calculation from `state`, mutating it in place so
/// it's ready for the next call. `state` must have come from
/// `pvi_indicator()` and not yet have been passed to `pvi_state_free()`.
///
/// `inputs` must point to `INPUTS` (2) pointers (`close, volume`), each `data_len` `f64`s long.
///
/// # Safety
/// - `state` must be a live pointer previously returned by
///   `pvi_indicator()`.
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn pvi_batch(
    state: *mut c_void,
    inputs: *const *const f64,
    data_len: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CBatchResult {
    if state.is_null() {
        return CBatchResult::err(IndicatorError::InvalidIndicatorState);
    }
    let state = &mut *(state as *mut PviStateHandle);

    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match state.batch_indicator(&inputs, optional) {
        Ok(rows) => {
            let (outputs, output_lens, num_outputs) = pack_outputs(rows, optional);
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

/// Frees a state handle returned by `pvi_indicator()` (or one of the
/// `states[i]` entries from `pvi_simd_by_assets()`).
/// Call this once you're done streaming (after your last `pvi_batch()` call).
///
/// # Safety
/// `state` must be a live pointer previously returned by `pvi_indicator()`
/// or found in a `CSimdResult::states` array, and must not be used again
/// after this call.
#[no_mangle]
pub unsafe extern "C" fn pvi_state_free(state: *mut c_void) {
    if !state.is_null() {
        drop(Box::from_raw(state as *mut PviStateHandle));
    }
}

/// Computes PVI for `N` assets simultaneously (SIMD), sharing a single
/// options array. `num_assets` must be 2, 4, 8, or 16 -- anything else
/// returns `InvalidInputs`.
///
/// `inputs` must point to `num_assets` pointers, each itself pointing to
/// `INPUTS` (2) pointers (`close, volume`), each `data_len` `f64`s long.
///
/// Returns a `CSimdResult` with `num_assets` results, each with its own
/// output rows and its own ordinary `pvi` continuation state (reusable
/// with `pvi_batch()`/`pvi_state_free()`). Free each state via
/// `pvi_state_free()`, then free the rest via `tulip_ffi_simd_result_free()`.
///
/// # Safety
/// - `inputs` must point to `num_assets` valid pointers, each pointing to
///   `INPUTS` valid non-null `*const f64`s, each pointing to `data_len` valid
///   `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn pvi_simd_by_assets(
    inputs: *const *const *const f64,
    num_assets: usize,
    data_len: usize,
    _options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    match num_assets {
        2 => pvi_simd_by_assets_n::<2>(inputs, data_len, _options, optional_outputs, numoptional),
        4 => pvi_simd_by_assets_n::<4>(inputs, data_len, _options, optional_outputs, numoptional),
        8 => pvi_simd_by_assets_n::<8>(inputs, data_len, _options, optional_outputs, numoptional),
        16 => pvi_simd_by_assets_n::<16>(inputs, data_len, _options, optional_outputs, numoptional),
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn pvi_simd_by_assets_n<const N: usize>(
    inputs: *const *const *const f64,
    data_len: usize,
    _options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    // `owned` holds the per-asset input slices; `refs` borrows from it, so
    // both must live in this stack frame for the duration of the call.
    let owned = read_simd_assets_inputs::<N, INPUTS>(inputs, data_len);
    let refs: [&[&[f64]; INPUTS]; N] = std::array::from_fn(|i| &owned[i]);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match Pvi::indicator_by_assets::<N>(&refs, &[], optional) {
        Ok((results, states)) => {
            let (outputs, output_lens, num_outputs, num_results) =
                pack_simd_outputs(results, optional);
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
    use crate::{tulip_ffi_batch_result_free, tulip_ffi_result_free, tulip_ffi_simd_result_free};
    use std::ptr;

    #[test]
    fn test_pvi_indicator() {
        unsafe {
            use crate::common::test::build_synthetic_data;

            let data_len = 60;
            let close_arr: Vec<f64> = build_synthetic_data(data_len).map(|x| x * 100.0).collect();
            let volume_arr: Vec<f64> = build_synthetic_data(data_len).map(|x| x * 1000.0).collect();

            let inputs_arr: [*const f64; INPUTS] = [close_arr.as_ptr(), volume_arr.as_ptr()];
            let inputs: *const *const f64 = inputs_arr.as_ptr();

            let result = pvi_indicator(inputs, data_len, ptr::null(), ptr::null(), 0);

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_outputs, 1);
            assert!(!result.state.is_null());

            tulip_ffi_result_free(result);
        }
    }

    #[test]
    fn test_pvi_batch() {
        unsafe {
            use crate::common::test::build_synthetic_data;

            let data_len = 60;
            let close_arr: Vec<f64> = build_synthetic_data(data_len).map(|x| x * 100.0).collect();
            let volume_arr: Vec<f64> = build_synthetic_data(data_len).map(|x| x * 1000.0).collect();

            let inputs_arr: [*const f64; INPUTS] = [close_arr.as_ptr(), volume_arr.as_ptr()];
            let inputs: *const *const f64 = inputs_arr.as_ptr();

            let result = pvi_indicator(inputs, data_len, ptr::null(), ptr::null(), 0);
            assert_eq!(result.error, CIndicatorError::Ok);

            // Second batch call with exactly 10 elements (batch length)
            let new_close_arr: Vec<f64> = build_synthetic_data(10).map(|x| x * 100.0).collect();
            let new_volume_arr: Vec<f64> = build_synthetic_data(10).map(|x| x * 1500.0).collect();

            let new_inputs_arr: [*const f64; INPUTS] =
                [new_close_arr.as_ptr(), new_volume_arr.as_ptr()];
            let new_inputs: *const *const f64 = new_inputs_arr.as_ptr();

            let batch_result = pvi_batch(result.state, new_inputs, 10, ptr::null(), 0);
            assert_eq!(batch_result.error, CIndicatorError::Ok);

            tulip_ffi_batch_result_free(batch_result);
            pvi_state_free(result.state);
        }
    }

    #[test]
    fn test_pvi_simd_by_assets() {
        unsafe {
            use crate::common::test::build_synthetic_data;

            let data_len = 60;
            let num_assets = 4;

            // Create inputs for 4 assets
            let asset0_close: Vec<f64> =
                build_synthetic_data(data_len).map(|x| x * 100.0).collect();
            let asset0_volume: Vec<f64> =
                build_synthetic_data(data_len).map(|x| x * 1000.0).collect();

            let asset1_close: Vec<f64> =
                build_synthetic_data(data_len).map(|x| x * 150.0).collect();
            let asset1_volume: Vec<f64> =
                build_synthetic_data(data_len).map(|x| x * 1200.0).collect();

            let asset2_close: Vec<f64> =
                build_synthetic_data(data_len).map(|x| x * 200.0).collect();
            let asset2_volume: Vec<f64> =
                build_synthetic_data(data_len).map(|x| x * 1800.0).collect();

            let asset3_close: Vec<f64> =
                build_synthetic_data(data_len).map(|x| x * 250.0).collect();
            let asset3_volume: Vec<f64> =
                build_synthetic_data(data_len).map(|x| x * 2000.0).collect();

            let inputs_arr0: [*const f64; INPUTS] = [asset0_close.as_ptr(), asset0_volume.as_ptr()];
            let inputs_arr1: [*const f64; INPUTS] = [asset1_close.as_ptr(), asset1_volume.as_ptr()];
            let inputs_arr2: [*const f64; INPUTS] = [asset2_close.as_ptr(), asset2_volume.as_ptr()];
            let inputs_arr3: [*const f64; INPUTS] = [asset3_close.as_ptr(), asset3_volume.as_ptr()];

            let assets_inputs: [*const *const f64; 4] = [
                inputs_arr0.as_ptr(),
                inputs_arr1.as_ptr(),
                inputs_arr2.as_ptr(),
                inputs_arr3.as_ptr(),
            ];
            let inputs: *const *const *const f64 = assets_inputs.as_ptr();

            let result =
                pvi_simd_by_assets(inputs, num_assets, data_len, ptr::null(), ptr::null(), 0);

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, 4);
            assert_eq!(result.num_outputs, 1);
            assert!(!result.states.is_null());

            tulip_ffi_simd_result_free(result);
        }
    }

    #[test]
    fn test_pvi_info() {
        let info = pvi_info();
        assert!(info.inputs.len > 0);
        assert_eq!(info.options.len, 0);
        assert!(info.outputs.len > 0);
        assert_eq!(info.optional_outputs.len, 0);
    }

    #[test]
    fn test_pvi_min_data() {
        unsafe {
            let min = pvi_min_data(std::ptr::null());
            assert!(min > 0);
        }
    }
}
