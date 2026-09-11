//! extern "C" wrapper for `emv`, mirroring the core `tulip_rs` crate's
//! `Emv::indicator` / `IndicatorState::batch_indicator` interface with a
//! Tulip-Indicators-style C calling convention: `inputs` is an array of
//! `INPUTS` pointers (one per input series, each `data_len` long, in the same
//! order as `tulip_rs::indicators::emv::Emv::INFO.inputs`), `options`
//! is a flat array of `OPTIONS` values -- exactly like `ti_emv`'s
//! `double const *const *inputs, double const *options`. Output allocation
//! stays inside the indicator function (never caller-supplied).
//!
//! Parameter order convention (kept consistent across every function in
//! this crate): each pointer parameter is immediately followed by the
//! count(s) that describe it, e.g. `inputs, data_len, options, ...,
//! optional_outputs, numoptional`.
//!
//! SIMD entry points:
//! - `emv_simd_by_assets`: compute EMV for N assets simultaneously,
//!   sharing a single options array. N must be 2, 4, 8, or 16.
//! - `emv_simd_by_options`: compute EMV for one asset with N different
//!   option sets simultaneously. N must be 2, 4, 8, or 16.
//!
//! `#[no_mangle]` functions can't be generic, so each SIMD entry point is a
//! thin non-generic dispatcher that matches the runtime `N` against
//! {2,4,8,16} and calls a private generic `_n::<N>` helper that does the
//! real work.

use std::os::raw::c_void;

use tulip_rs::indicator_types::{Indicator, TIndicatorState};
use tulip_rs::indicators::emv::{Emv, IndicatorState as EmvState, INPUTS, OPTIONS};
use tulip_rs::types::IndicatorError;

use crate::common::{
    optional_outputs_slice, pack_info, pack_outputs, pack_simd_outputs, pack_states, read_inputs,
    read_simd_assets_inputs, CBatchResult, CIndicatorError, CIndicatorInfo, CIndicatorResult,
    CSimdResult,
};

/// Opaque state handle returned by `emv_indicator()` and consumed by
/// `emv_batch()` / `emv_state_free()`.
pub type EmvStateHandle = EmvState;

/// Returns static metadata about the `emv` indicator: its name, input
/// names, option names, and (mandatory/optional) output names, mirroring
/// `Emv::INFO`.
///
/// The returned strings are leaked, process-lifetime C strings -- read them,
/// don't free them.
#[no_mangle]
pub extern "C" fn emv_info() -> CIndicatorInfo {
    pack_info(&Emv::INFO)
}

/// Returns the minimum number of bars `emv` needs to produce any output at
/// all, given `options`.
///
/// Note: EMV has no options (`OPTIONS == 0`).
#[no_mangle]
pub extern "C" fn emv_min_data(_options: *const f64) -> usize {
    Emv::min_data(&[])
}

/// Runs `emv` over `data_len` bars.
///
/// `inputs` must point to `INPUTS` (3) pointers, in order:
/// `high, low, volume`, each `data_len` `f64`s long.
/// `options` is ignored (EMV has no options).
///
/// Returns the mandatory `emv` output plus any requested optional outputs
/// (`medprice`, in that fixed order) and a fresh continuation state.
///
/// # Safety
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s (but OPTIONS is 0 for EMV).
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s (pass null + 0 to request no optional outputs).
#[no_mangle]
pub unsafe extern "C" fn emv_indicator(
    inputs: *const *const f64,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CIndicatorResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    // EMV has no options
    let _options: [f64; OPTIONS] = if OPTIONS > 0 {
        *(options as *const [f64; OPTIONS])
    } else {
        [0.0; OPTIONS]
    };
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match Emv::indicator(&inputs, &[], optional) {
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

/// Continues an `emv` calculation from `state`, mutating it in place so
/// it's ready for the next call. `state` must have come from
/// `emv_indicator()` and not yet have been passed to `emv_state_free()`.
///
/// `inputs` must point to `INPUTS` (3) pointers (`high, low, volume`),
/// each `data_len` `f64`s long.
///
/// # Safety
/// - `state` must be a live pointer previously returned by
///   `emv_indicator()`.
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn emv_batch(
    state: *mut c_void,
    inputs: *const *const f64,
    data_len: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CBatchResult {
    if state.is_null() {
        return CBatchResult::err(IndicatorError::InvalidIndicatorState);
    }
    let state = &mut *(state as *mut EmvStateHandle);

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

/// Frees a state handle returned by `emv_indicator()` (or one of the
/// `states[i]` entries from `emv_simd_by_assets()`/`emv_simd_by_options()`).
/// Call this once you're done streaming (after your last `emv_batch()` call).
///
/// # Safety
/// `state` must be a live pointer previously returned by `emv_indicator()`
/// or found in a `CSimdResult::states` array, and must not be used again
/// after this call.
#[no_mangle]
pub unsafe extern "C" fn emv_state_free(state: *mut c_void) {
    if !state.is_null() {
        drop(Box::from_raw(state as *mut EmvStateHandle));
    }
}

/// Computes EMV for `N` assets simultaneously (SIMD), sharing a single
/// options array. `num_assets` must be 2, 4, 8, or 16 -- anything else
/// returns `InvalidInputs`.
///
/// `inputs` must point to `num_assets` pointers, each itself pointing to
/// `INPUTS` (3) pointers (`high, low, volume`), each `data_len` `f64`s
/// long. `options` is ignored (EMV has no options).
///
/// Returns a `CSimdResult` with `num_assets` results, each with its own
/// output rows and its own ordinary `emv` continuation state (reusable
/// with `emv_batch()`/`emv_state_free()`). Free each state via
/// `emv_state_free()`, then free the rest via `tulip_ffi_simd_result_free()`.
///
/// # Safety
/// - `inputs` must point to `num_assets` valid pointers, each pointing to
///   `INPUTS` valid non-null `*const f64`s, each pointing to `data_len` valid
///   `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s (but OPTIONS is 0 for EMV).
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn emv_simd_by_assets(
    inputs: *const *const *const f64,
    num_assets: usize,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    match num_assets {
        2 => emv_simd_by_assets_n::<2>(inputs, data_len, options, optional_outputs, numoptional),
        4 => emv_simd_by_assets_n::<4>(inputs, data_len, options, optional_outputs, numoptional),
        8 => emv_simd_by_assets_n::<8>(inputs, data_len, options, optional_outputs, numoptional),
        16 => emv_simd_by_assets_n::<16>(inputs, data_len, options, optional_outputs, numoptional),
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn emv_simd_by_assets_n<const N: usize>(
    inputs: *const *const *const f64,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    // `owned` holds the per-asset input slices; `refs` borrows from it, so
    // both must live in this stack frame for the duration of the call.
    let owned = read_simd_assets_inputs::<N, INPUTS>(inputs, data_len);
    let refs: [&[&[f64]; INPUTS]; N] = std::array::from_fn(|i| &owned[i]);
    // EMV has no options
    let _options: [f64; OPTIONS] = if OPTIONS > 0 {
        *(options as *const [f64; OPTIONS])
    } else {
        [0.0; OPTIONS]
    };
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match Emv::indicator_by_assets::<N>(&refs, &[], optional) {
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
    use crate::common::test::build_synthetic_data;
    use crate::{tulip_ffi_batch_result_free, tulip_ffi_result_free, tulip_ffi_simd_result_free};
    use std::ptr;
    #[test]
    fn test_emv_indicator() {
        unsafe {
            let data_len = 20;
            let high_arr: Vec<f64> = build_synthetic_data(data_len);
            let low_arr: Vec<f64> = build_synthetic_data(data_len);
            let volume_arr: Vec<f64> = build_synthetic_data(data_len);

            let inputs_arr: [*const f64; INPUTS] =
                [high_arr.as_ptr(), low_arr.as_ptr(), volume_arr.as_ptr()];
            let inputs: *const *const f64 = inputs_arr.as_ptr();
            // EMV has no options, pass null
            let options: *const f64 = std::ptr::null();

            let result = emv_indicator(inputs, data_len, options, ptr::null(), 0);

            assert_eq!(result.error, CIndicatorError::Ok);
            // Without optional outputs: only mandatory rows returned (emv)
            assert_eq!(result.num_outputs, 1);
            assert!(!result.state.is_null());

            tulip_ffi_result_free(result);
        }
    }

    #[test]
    fn test_emv_batch() {
        unsafe {
            let data_len = 20;
            let high_arr: Vec<f64> = build_synthetic_data(data_len);
            let low_arr: Vec<f64> = build_synthetic_data(data_len);
            let volume_arr: Vec<f64> = build_synthetic_data(data_len);

            let inputs_arr: [*const f64; INPUTS] =
                [high_arr.as_ptr(), low_arr.as_ptr(), volume_arr.as_ptr()];
            let inputs: *const *const f64 = inputs_arr.as_ptr();
            // EMV has no options, pass null
            let options: *const f64 = std::ptr::null();

            let result = emv_indicator(inputs, data_len, options, ptr::null(), 0);
            assert_eq!(result.error, CIndicatorError::Ok);

            let new_high_arr: Vec<f64> = build_synthetic_data(data_len);
            let new_low_arr: Vec<f64> = build_synthetic_data(data_len);
            let new_volume_arr: Vec<f64> = build_synthetic_data(data_len);

            let new_inputs_arr: [*const f64; INPUTS] = [
                new_high_arr.as_ptr(),
                new_low_arr.as_ptr(),
                new_volume_arr.as_ptr(),
            ];
            let new_inputs: *const *const f64 = new_inputs_arr.as_ptr();
            // EMV has no options, pass null

            let batch_result = emv_batch(result.state, new_inputs, 10, ptr::null(), 0);
            assert_eq!(batch_result.error, CIndicatorError::Ok);

            tulip_ffi_batch_result_free(batch_result);
            emv_state_free(result.state);
        }
    }

    #[test]
    fn test_emv_info() {
        let info = emv_info();
        assert!(info.inputs.len > 0);
        assert_eq!(info.options.len, OPTIONS);
        assert!(info.outputs.len > 0);
        assert_eq!(info.optional_outputs.len, 1); // medprice
    }

    #[test]
    fn test_emv_min_data() {
        let min = emv_min_data(std::ptr::null());
        assert!(min > 0);
    }

    #[test]
    fn test_emv_simd_by_assets() {
        unsafe {
            let data_len = 20;
            let num_assets = 2;

            // Create inputs for 2 assets
            let asset0_high: Vec<f64> = build_synthetic_data(data_len);
            let asset0_low: Vec<f64> = build_synthetic_data(data_len);
            let asset0_volume: Vec<f64> = build_synthetic_data(data_len);

            let asset1_high: Vec<f64> = build_synthetic_data(data_len);
            let asset1_low: Vec<f64> = build_synthetic_data(data_len);
            let asset1_volume: Vec<f64> = build_synthetic_data(data_len);

            let inputs_arr0: [*const f64; INPUTS] = [
                asset0_high.as_ptr(),
                asset0_low.as_ptr(),
                asset0_volume.as_ptr(),
            ];
            let inputs_arr1: [*const f64; INPUTS] = [
                asset1_high.as_ptr(),
                asset1_low.as_ptr(),
                asset1_volume.as_ptr(),
            ];

            let inputs_arr_arr: [*const *const f64; 2] =
                [inputs_arr0.as_ptr(), inputs_arr1.as_ptr()];
            let inputs: *const *const *const f64 = inputs_arr_arr.as_ptr();
            // EMV has no options, pass null
            let options: *const f64 = std::ptr::null();

            let result = emv_simd_by_assets(inputs, num_assets, data_len, options, ptr::null(), 0);

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, 2);
            // Without optional outputs: only mandatory rows returned (emv)
            assert_eq!(result.num_outputs, 1);
            assert!(!result.states.is_null());

            tulip_ffi_simd_result_free(result);
        }
    }
}
