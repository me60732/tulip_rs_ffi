//! extern "C" wrapper for `obv`, mirroring the core `tulip_rs` crate's
//! `Obv::indicator` / `IndicatorState::batch_indicator` interface with a
//! Tulip-Indicators-style C calling convention: `inputs` is an array of
//! `INPUTS` pointers (one per input series, each `data_len` long, in the same
//! order as `tulip_rs::indicators::obv::Obv::INFO.inputs`), `options`
//! is a flat array of `OPTIONS` values -- exactly like `ti_obv`'s
//! `double const *const *inputs, double const *options`. Output allocation
//! stays inside the indicator function (never caller-supplied).
//!
//! Parameter order convention (kept consistent across every function in
//! this crate): each pointer parameter is immediately followed by the
//! count(s) that describe it, e.g. `inputs, data_len, options, ...,
//! optional_outputs, numoptional`.
//!
//! SIMD entry points:
//! - `obv_simd_by_assets`: compute OBV for N assets simultaneously,
//!   sharing a single options array. N must be 2, 4, 8, or 16.
//!
//! NOTE: `obv` has OPTIONS=0, so there is no `obv_simd_by_options` function.

use std::os::raw::c_void;

use tulip_rs::indicator_types::{Indicator, TIndicatorState};
use tulip_rs::indicators::obv::{IndicatorState as ObvState, Obv, INPUTS};
use tulip_rs::types::IndicatorError;

use crate::common::{
    optional_outputs_slice, pack_info, pack_outputs, pack_simd_outputs, pack_states, read_inputs,
    read_simd_assets_inputs, CBatchResult, CIndicatorError, CIndicatorInfo, CIndicatorResult,
    CSimdResult,
};

/// Opaque state handle returned by `obv_indicator()` and consumed by
/// `obv_batch()` / `obv_state_free()`.
pub type ObvStateHandle = ObvState;

/// Returns static metadata about the `obv` indicator: its name, input
/// names, option names, and (mandatory/optional) output names, mirroring
/// `Obv::INFO`.
///
/// The returned strings are leaked, process-lifetime C strings -- read them,
/// don't free them.
#[no_mangle]
pub extern "C" fn obv_info() -> CIndicatorInfo {
    pack_info(&Obv::INFO)
}

/// Returns the minimum number of bars `obv` needs to produce any output at
/// all. Since `obv` has OPTIONS=0, no options are required.
#[no_mangle]
pub unsafe extern "C" fn obv_min_data(_options: *const f64) -> usize {
    Obv::min_data(&[])
}

/// Runs `obv` over `data_len` bars.
///
/// `inputs` must point to `INPUTS` (2) pointers, in order:
/// `close, volume`, each `data_len` `f64`s long.
/// `options` is ignored (OPTIONS=0).
///
/// Returns the mandatory `obv` output plus any requested optional outputs
/// and a fresh continuation state.
///
/// # Safety
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s (pass null + 0 to request no optional outputs).
#[no_mangle]
pub unsafe extern "C" fn obv_indicator(
    inputs: *const *const f64,
    data_len: usize,
    _options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CIndicatorResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match Obv::indicator(&inputs, &[], optional) {
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

/// Continues an `obv` calculation from `state`, mutating it in place so
/// it's ready for the next call. `state` must have come from
/// `obv_indicator()` and not yet have been passed to `obv_state_free()`.
///
/// `inputs` must point to `INPUTS` (2) pointers (`close, volume`),
/// each `data_len` `f64`s long.
///
/// # Safety
/// - `state` must be a live pointer previously returned by
///   `obv_indicator()`.
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn obv_batch(
    state: *mut c_void,
    inputs: *const *const f64,
    data_len: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CBatchResult {
    if state.is_null() {
        return CBatchResult::err(IndicatorError::InvalidIndicatorState);
    }
    let state = &mut *(state as *mut ObvStateHandle);

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

/// Frees a state handle returned by `obv_indicator()` (or one of the
/// `states[i]` entries from `obv_simd_by_assets()`).
/// Call this once you're done streaming (after your last `obv_batch()` call).
///
/// # Safety
/// `state` must be a live pointer previously returned by `obv_indicator()`
/// or found in a `CSimdResult::states` array, and must not be used again
/// after this call.
#[no_mangle]
pub unsafe extern "C" fn obv_state_free(state: *mut c_void) {
    if !state.is_null() {
        drop(Box::from_raw(state as *mut ObvStateHandle));
    }
}

/// Computes OBV for `N` assets simultaneously (SIMD), sharing a single
/// options array. `num_assets` must be 2, 4, 8, or 16 -- anything else
/// returns `InvalidInputs`.
///
/// `inputs` must point to `num_assets` pointers, each itself pointing to
/// `INPUTS` (2) pointers (`close, volume`), each `data_len` `f64`s long.
/// `options` is ignored (OPTIONS=0).
///
/// Returns a `CSimdResult` with `num_assets` results, each with its own
/// output rows and its own ordinary `obv` continuation state (reusable
/// with `obv_batch()`/`obv_state_free()`). Free each state via
/// `obv_state_free()`, then free the rest via `tulip_ffi_simd_result_free()`.
///
/// # Safety
/// - `inputs` must point to `num_assets` valid pointers, each pointing to
///   `INPUTS` valid non-null `*const f64`s, each pointing to `data_len` valid
///   `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn obv_simd_by_assets(
    inputs: *const *const *const f64,
    num_assets: usize,
    data_len: usize,
    _options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    match num_assets {
        2 => obv_simd_by_assets_n::<2>(inputs, data_len, optional_outputs, numoptional),
        4 => obv_simd_by_assets_n::<4>(inputs, data_len, optional_outputs, numoptional),
        8 => obv_simd_by_assets_n::<8>(inputs, data_len, optional_outputs, numoptional),
        16 => obv_simd_by_assets_n::<16>(inputs, data_len, optional_outputs, numoptional),
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn obv_simd_by_assets_n<const N: usize>(
    inputs: *const *const *const f64,
    data_len: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    // `owned` holds the per-asset input slices; `refs` borrows from it, so
    // both must live in this stack frame for the duration of the call.
    let owned = read_simd_assets_inputs::<N, INPUTS>(inputs, data_len);
    let refs: [&[&[f64]; INPUTS]; N] = std::array::from_fn(|i| &owned[i]);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match Obv::indicator_by_assets::<N>(&refs, &[], optional) {
        Ok((results, states)) => {
            let (outputs, output_lens, num_outputs, num_results) = pack_simd_outputs(results, optional);
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

    #[test]
    fn test_obv_info() {
        let info = obv_info();
        assert_eq!(info.inputs.len, INPUTS);
        assert_eq!(info.options.len, 0);
        assert_eq!(info.outputs.len, 1);
        assert_eq!(info.optional_outputs.len, 0);
    }

    #[test]
    fn test_obv_min_data() {
        unsafe {
            assert!(obv_min_data(std::ptr::null()) > 0);
        }
    }

    #[test]
    fn test_obv_indicator() {
        let data_len = 60;

        unsafe {
            let close = build_synthetic_data(data_len, 0);
            let volume = build_synthetic_data(data_len, 0);
            let inputs: [*const f64; INPUTS] = [close.as_ptr(), volume.as_ptr()];
            let result = obv_indicator(
                inputs.as_ptr(),
                data_len,
                std::ptr::null(),
                std::ptr::null(),
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_outputs, 1);
            assert!(!result.outputs.is_null());
            assert!(!result.output_lens.is_null());

            let state = result.state;
            tulip_ffi_result_free(result);
            obv_state_free(state);
        }
    }

    #[test]
    fn test_obv_batch() {
        let data_len = 60;

        unsafe {
            let close = build_synthetic_data(data_len, 0);
            let volume = build_synthetic_data(data_len, 0);
            let inputs: [*const f64; INPUTS] = [close.as_ptr(), volume.as_ptr()];
            let result = obv_indicator(
                inputs.as_ptr(),
                data_len,
                std::ptr::null(),
                std::ptr::null(),
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            let state = result.state;
            tulip_ffi_result_free(result);

            let more_close = build_synthetic_data(data_len, 0);
            let more_volume = build_synthetic_data(data_len, 0);
            let more_inputs: [*const f64; INPUTS] = [more_close.as_ptr(), more_volume.as_ptr()];

            let batch_result =
                obv_batch(state, more_inputs.as_ptr(), data_len, std::ptr::null(), 0);

            assert_eq!(batch_result.error, CIndicatorError::Ok);
            assert_eq!(batch_result.num_outputs, 1);

            tulip_ffi_batch_result_free(batch_result);
            obv_state_free(state);
        }
    }

    #[test]
    fn test_obv_simd_by_assets() {
        const NUM_ASSETS: usize = 4;
        let data_len = 60;

        unsafe {
            let c0 = build_synthetic_data(data_len, 0);
            let v0 = build_synthetic_data(data_len, 0);
            let c1 = build_synthetic_data(data_len, 0);
            let v1 = build_synthetic_data(data_len, 0);
            let c2 = build_synthetic_data(data_len, 0);
            let v2 = build_synthetic_data(data_len, 0);
            let c3 = build_synthetic_data(data_len, 0);
            let v3 = build_synthetic_data(data_len, 0);
            let asset_inputs: [[*const f64; INPUTS]; NUM_ASSETS] = [
                [c0.as_ptr(), v0.as_ptr()],
                [c1.as_ptr(), v1.as_ptr()],
                [c2.as_ptr(), v2.as_ptr()],
                [c3.as_ptr(), v3.as_ptr()],
            ];
            let inputs_ptr: [*const *const f64; NUM_ASSETS] = [
                asset_inputs[0].as_ptr(),
                asset_inputs[1].as_ptr(),
                asset_inputs[2].as_ptr(),
                asset_inputs[3].as_ptr(),
            ];

            let result = obv_simd_by_assets(
                inputs_ptr.as_ptr(),
                NUM_ASSETS,
                data_len,
                std::ptr::null(),
                std::ptr::null(),
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, NUM_ASSETS);
            assert_eq!(result.num_outputs, 1);

            for i in 0..NUM_ASSETS {
                obv_state_free(*result.states.add(i));
            }
            tulip_ffi_simd_result_free(result);
        }
    }
}
