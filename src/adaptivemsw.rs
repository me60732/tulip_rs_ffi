//! extern "C" wrapper for `adaptivemsw`, mirroring the core `tulip_rs` crate's
//! `AdaptiveMSW::indicator` / `IndicatorState::batch_indicator` interface with a
//! Tulip-Indicators-style C calling convention: `inputs` is an array of
//! `INPUTS` pointers (one per input series, each `data_len` long, in the same
//! order as `tulip_rs::indicators::adaptivemsw::AdaptiveMSW::INFO.inputs`), `options`
//! is a flat array of `OPTIONS` values -- exactly like `ti_adaptivemsw`'s
//! `double const *const *inputs, double const *options`. Output allocation
//! stays inside the indicator function (never caller-supplied).
//!
//! Parameter order convention (kept consistent across every function in
//! this crate): each pointer parameter is immediately followed by the
//! count(s) that describe it, e.g. `inputs, data_len, options, ...,
//! optional_outputs, numoptional`.
//!
//! SIMD entry points:
//! - `adaptivemsw_simd_by_assets`: compute AdaptiveMSW for N assets simultaneously,
//!   sharing a single options array. N must be 2, 4, 8, or 16.
//!
//! Note: `adaptivemsw` does not support SIMD by-options (no `indicator_by_options` impl).
//!
//! `#[no_mangle]` functions can't be generic, so each SIMD entry point is a
//! thin non-generic dispatcher that matches the runtime `N` against
//! {2,4,8,16} and calls a private generic `_n::<N>` helper that does the
//! real work.

use std::os::raw::c_void;

use tulip_rs::indicator_types::{Indicator, TIndicatorState};
use tulip_rs::indicators::adaptivemsw::{
    AdaptiveMSW, IndicatorState as AdaptiveMSWState, INPUTS, OPTIONS,
};
use tulip_rs::types::IndicatorError;

use crate::common::{
    optional_outputs_slice, pack_info, pack_outputs, pack_simd_outputs, pack_states, read_inputs,
    read_simd_assets_inputs, CBatchResult, CIndicatorError, CIndicatorInfo, CIndicatorResult,
    CSimdResult,
};

/// Opaque state handle returned by `adaptivemsw_indicator()` and consumed by
/// `adaptivemsw_batch()` / `adaptivemsw_state_free()`.
pub type AdaptiveMSWStateHandle = AdaptiveMSWState;

/// Returns static metadata about the `adaptivemsw` indicator: its name, input
/// names, option names, and (mandatory/optional) output names, mirroring
/// `AdaptiveMSW::INFO`.
///
/// The returned strings are leaked, process-lifetime C strings -- read them,
/// don't free them.
#[no_mangle]
pub extern "C" fn adaptivemsw_info() -> CIndicatorInfo {
    pack_info(&AdaptiveMSW::INFO)
}

/// Returns the minimum number of bars `adaptivemsw` needs to produce any output at
/// all, given `options`.
#[no_mangle]
pub extern "C" fn adaptivemsw_min_data(_options: *const f64) -> usize {
    AdaptiveMSW::min_data(&[])
}

/// Runs `adaptivemsw` over `data_len` bars.
///
/// `inputs` must point to `INPUTS` (1) pointer: `real`, `data_len` `f64`s long.
/// `options` must point to `OPTIONS` (0) values (no options).
///
/// Returns the mandatory `sine`/`lead_sine` outputs plus any requested optional outputs
/// (`dc_period`) and a fresh continuation state.
///
/// # Safety
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s (no options, so this can be null or any pointer).
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s (pass null + 0 to request no optional outputs).
#[no_mangle]
pub unsafe extern "C" fn adaptivemsw_indicator(
    inputs: *const *const f64,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CIndicatorResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let _options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match AdaptiveMSW::indicator(&inputs, &_options, optional) {
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

/// Continues an `adaptivemsw` calculation from `state`, mutating it in place so
/// it's ready for the next call. `state` must have come from
/// `adaptivemsw_indicator()` and not yet have been passed to `adaptivemsw_state_free()`.
///
/// `inputs` must point to `INPUTS` (1) pointer: `real`, `data_len` `f64`s long.
///
/// # Safety
/// - `state` must be a live pointer previously returned by
///   `adaptivemsw_indicator()`.
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn adaptivemsw_batch(
    state: *mut c_void,
    inputs: *const *const f64,
    data_len: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CBatchResult {
    if state.is_null() {
        return CBatchResult::err(IndicatorError::InvalidIndicatorState);
    }
    let state = &mut *(state as *mut AdaptiveMSWStateHandle);

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

/// Frees a state handle returned by `adaptivemsw_indicator()` (or one of the
/// `states[i]` entries from `adaptivemsw_simd_by_assets()`).
/// Call this once you're done streaming (after your last `adaptivemsw_batch()` call).
///
/// # Safety
/// `state` must be a live pointer previously returned by `adaptivemsw_indicator()`
/// or found in a `CSimdResult::states` array, and must not be used again
/// after this call.
#[no_mangle]
pub unsafe extern "C" fn adaptivemsw_state_free(state: *mut c_void) {
    if !state.is_null() {
        drop(Box::from_raw(state as *mut AdaptiveMSWStateHandle));
    }
}

/// Computes AdaptiveMSW for `N` assets simultaneously (SIMD), sharing a single
/// options array. `num_assets` must be 2, 4, 8, or 16 -- anything else
/// returns `InvalidInputs`.
///
/// `inputs` must point to `num_assets` pointers, each itself pointing to
/// `INPUTS` (1) pointer (`real`), `data_len` `f64`s long. `options` must point
/// to `OPTIONS` (0) shared values.
///
/// Returns a `CSimdResult` with `num_assets` results, each with its own
/// output rows and its own ordinary `adaptivemsw` continuation state (reusable
/// with `adaptivemsw_batch()`/`adaptivemsw_state_free()`). Free each state via
/// `adaptivemsw_state_free()`, then free the rest via `tulip_ffi_simd_result_free()`.
///
/// # Safety
/// - `inputs` must point to `num_assets` valid pointers, each pointing to
///   `INPUTS` valid non-null `*const f64`s, each pointing to `data_len` valid
///   `f64`s.
#[no_mangle]
pub unsafe extern "C" fn adaptivemsw_simd_by_assets(
    inputs: *const *const *const f64,
    num_assets: usize,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    match num_assets {
        2 => adaptivemsw_simd_by_assets_n::<2>(
            inputs,
            data_len,
            options,
            optional_outputs,
            numoptional,
        ),
        4 => adaptivemsw_simd_by_assets_n::<4>(
            inputs,
            data_len,
            options,
            optional_outputs,
            numoptional,
        ),
        8 => adaptivemsw_simd_by_assets_n::<8>(
            inputs,
            data_len,
            options,
            optional_outputs,
            numoptional,
        ),
        16 => adaptivemsw_simd_by_assets_n::<16>(
            inputs,
            data_len,
            options,
            optional_outputs,
            numoptional,
        ),
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn adaptivemsw_simd_by_assets_n<const N: usize>(
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
    let _options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match AdaptiveMSW::indicator_by_assets::<N>(&refs, &_options, optional) {
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

/// Computes AdaptiveMSW for one asset with `N` different option sets
/// simultaneously (SIMD). `num_option_sets` must be 2, 4, 8, or 16 --
/// anything else returns `InvalidInputs`.
///
/// `inputs` must point to `INPUTS` (1) pointer (`real`), `data_len` `f64`s
/// long, shared across all option sets. `options` must point to
/// `num_option_sets` pointers, each itself pointing to `OPTIONS` (0)
/// values.
///
/// Returns a `CSimdResult` with `num_option_sets` results, each with its
/// own output rows and its own ordinary `adaptivemsw` continuation state
/// (reusable with `adaptivemsw_batch()`/`adaptivemsw_state_free()`). Free each state via
/// `adaptivemsw_state_free()`, then free the rest via
/// `tulip_ffi_simd_result_free()`.
///
#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::{
        tulip_ffi_batch_result_free, tulip_ffi_result_free, tulip_ffi_simd_result_free,
    };
    use std::slice;

    unsafe fn free_result(result: CIndicatorResult) {
        tulip_ffi_result_free(result);
    }

    unsafe fn free_batch_result(result: CBatchResult) {
        tulip_ffi_batch_result_free(result);
    }

    unsafe fn free_simd_result(result: CSimdResult) {
        tulip_ffi_simd_result_free(result);
    }

    #[test]
    fn test_adaptivemsw_info() {
        let info = adaptivemsw_info();
        assert!(info.inputs.len > 0);
        assert_eq!(info.options.len, 0);
        assert!(info.outputs.len > 0);
        assert_eq!(info.optional_outputs.len, 1);
    }

    #[test]
    fn test_adaptivemsw_min_data() {
        let min = adaptivemsw_min_data(std::ptr::null());
        assert!(min > 0);
    }

    #[test]
    fn test_adaptivemsw_indicator() {
        unsafe {
            let data_len = 60;
            let real: Vec<f64> = (0..data_len)
                .map(|i| 100.0 + (i as f64 * 0.5).sin())
                .collect();

            let inputs_ptr: [*const f64; INPUTS] = [real.as_ptr()];
            let inputs = inputs_ptr.as_ptr();
            let options = [0f64; OPTIONS].as_ptr();

            // Request all optional outputs
            let optional_outputs = [true]; // dc_period is the only optional output
            let result =
                adaptivemsw_indicator(inputs, data_len, options, optional_outputs.as_ptr(), 1);

            assert_eq!(result.error, CIndicatorError::Ok);
            // With optional outputs requested: all rows returned (sine, lead_sine, dc_period)
            assert_eq!(result.num_outputs, 3);

            let _outputs_slice = slice::from_raw_parts(result.outputs, result.num_outputs);
            let output_lens_slice = slice::from_raw_parts(result.output_lens, result.num_outputs);

            assert_eq!(output_lens_slice[0], data_len - 22); // min_data is 23, so first output at index 22
            assert_eq!(output_lens_slice[1], data_len - 22);
            assert_eq!(output_lens_slice[2], data_len - 22);

            free_result(result);
        }
    }

    #[test]
    fn test_adaptivemsw_batch() {
        unsafe {
            let data_len = 60;
            let real: Vec<f64> = (0..data_len)
                .map(|i| 100.0 + (i as f64 * 0.5).sin())
                .collect();

            let inputs_ptr: [*const f64; INPUTS] = [real.as_ptr()];
            let inputs = inputs_ptr.as_ptr();
            let options = [0f64; OPTIONS].as_ptr();

            // Request all optional outputs
            let optional_outputs = [true];
            let result =
                adaptivemsw_indicator(inputs, data_len, options, optional_outputs.as_ptr(), 1);

            assert_eq!(result.error, CIndicatorError::Ok);
            let state = result.state;

            // Second batch with more data
            let extra_data_len = 10;
            let real_extra: Vec<f64> = (data_len..data_len + extra_data_len)
                .map(|i| 100.0 + (i as f64 * 0.5).sin())
                .collect();

            let inputs_ptr_extra: [*const f64; INPUTS] = [real_extra.as_ptr()];
            let inputs_extra = inputs_ptr_extra.as_ptr();
            let batch_result =
                adaptivemsw_batch(state, inputs_extra, extra_data_len, std::ptr::null(), 0);

            assert_eq!(batch_result.error, CIndicatorError::Ok);
            // Without optional outputs: only mandatory rows returned (sine, lead_sine)
            assert_eq!(batch_result.num_outputs, 2);

            free_batch_result(batch_result);
            adaptivemsw_state_free(state);
        }
    }

    #[test]
    fn test_adaptivemsw_simd_by_assets() {
        unsafe {
            let data_len = 60;
            let real: Vec<f64> = (0..data_len)
                .map(|i| 100.0 + (i as f64 * 0.5).sin())
                .collect();

            // Two identical "assets"
            let inputs_ptr_0: [*const f64; INPUTS] = [real.as_ptr()];
            let inputs_ptr_1: [*const f64; INPUTS] = [real.as_ptr()];
            let assets_arr: [*const *const f64; 2] = [inputs_ptr_0.as_ptr(), inputs_ptr_1.as_ptr()];
            let assets_ptrs = assets_arr.as_ptr();
            let options_arr = [0f64; OPTIONS];
            let options = options_arr.as_ptr();

            let result =
                adaptivemsw_simd_by_assets(assets_ptrs, 2, data_len, options, std::ptr::null(), 0);

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, 2);
            // Without optional outputs: only mandatory rows returned (sine, lead_sine)
            assert_eq!(result.num_outputs, 2);

            let states_slice = slice::from_raw_parts(result.states, result.num_results);
            for i in 0..result.num_results {
                adaptivemsw_state_free(states_slice[i]);
            }
            free_simd_result(result);
        }
    }
}
