//! extern "C" wrapper for `min`, mirroring the core `tulip_rs` crate's
//! `Min::indicator` / `IndicatorState::batch_indicator` interface with a
//! Tulip-Indicators-style C calling convention: `inputs` is an array of
//! `INPUTS` pointers (one per input series, each `data_len` long, in the same
//! order as `tulip_rs::indicators::min::Min::INFO.inputs`), `options`
//! is a flat array of `OPTIONS` values -- exactly like `ti_min`'s
//! `double const *const *inputs, double const *options`. Output allocation
//! stays inside the indicator function (never caller-supplied).
//!
//! Parameter order convention (kept consistent across every function in
//! this crate): each pointer parameter is immediately followed by the
//! count(s) that describe it, e.g. `inputs, data_len, options, ...,
//! optional_outputs, numoptional`.
//!
//! SIMD entry points:
//! - `min_simd_by_assets`: compute MIN for N assets simultaneously,
//!   sharing a single options array. N must be 2, 4, 8, or 16.
//! - `min_simd_by_options`: compute MIN for one asset with N different
//!   option sets simultaneously. N must be 2, 4, 8, or 16.
//!
//! `#[no_mangle]` functions can't be generic, so each SIMD entry point is a
//! thin non-generic dispatcher that matches the runtime `N` against
//! {2,4,8,16} and calls a private generic `_n::<N>` helper that does the
//! real work.

use std::os::raw::c_void;

use tulip_rs::indicator_types::{Indicator, IndicatorByOptions, TIndicatorState};
use tulip_rs::indicators::min::{IndicatorState as MinState, Min, INPUTS, OPTIONS};
use tulip_rs::types::IndicatorError;

use crate::common::{
    pack_info, pack_outputs, pack_simd_outputs, pack_states, read_inputs, read_simd_assets_inputs,
    read_simd_options, CBatchResult, CIndicatorError, CIndicatorInfo, CIndicatorResult,
    CSimdResult,
};

/// Opaque state handle returned by `min_indicator()` and consumed by
/// `min_batch()` / `min_state_free()`.
pub type MinStateHandle = MinState;

/// Returns static metadata about the `min` indicator: its name, input
/// names, option names, and (mandatory/optional) output names, mirroring
/// `Min::INFO`.
///
/// The returned strings are leaked, process-lifetime C strings -- read them,
/// don't free them.
#[no_mangle]
pub extern "C" fn min_info() -> CIndicatorInfo {
    pack_info(&Min::INFO)
}

/// Returns the minimum number of bars `min` needs to produce any output at
/// all, given `options`.
///
/// # Safety
/// `options` must point to `OPTIONS` (1) valid `f64`s.
#[no_mangle]
pub unsafe extern "C" fn min_min_data(options: *const f64) -> usize {
    let options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    Min::min_data(&options)
}

/// Runs `min` over `data_len` bars.
///
/// `inputs` must point to `INPUTS` (1) pointer: `real`, each `data_len` `f64`s long.
/// `options` must point to `OPTIONS` (1) value: `period`.
///
/// Returns the mandatory `min` output (no optional outputs available).
///
/// # Safety
/// - `inputs` must point to `INPUTS` valid non-null `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s.
#[no_mangle]
pub unsafe extern "C" fn min_indicator(
    inputs: *const *const f64,
    data_len: usize,
    options: *const f64,
    _optional_outputs: *const bool,
    _num_optional: usize,
) -> CIndicatorResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);

    match Min::indicator(&inputs, &options, None) {
        Ok((rows, state)) => {
            let (outputs, output_lens, num_outputs) = pack_outputs(rows, None);
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

/// Continues a `min` calculation from `state`, mutating it in place so
/// it's ready for the next call. `state` must have come from
/// `min_indicator()` and not yet have been passed to `min_state_free()`.
///
/// `inputs` must point to `INPUTS` (1) pointer: `real`, each `data_len` `f64`s long.
///
/// # Safety
/// - `state` must be a live pointer previously returned by
///   `min_indicator()`.
/// - `inputs` must point to `INPUTS` valid non-null `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
#[no_mangle]
pub unsafe extern "C" fn min_batch(
    state: *mut c_void,
    inputs: *const *const f64,
    data_len: usize,
    _optional_outputs: *const bool,
    _num_optional: usize,
) -> CBatchResult {
    if state.is_null() {
        return CBatchResult::err(IndicatorError::InvalidIndicatorState);
    }
    let state = &mut *(state as *mut MinStateHandle);

    let inputs = read_inputs::<INPUTS>(inputs, data_len);

    match state.batch_indicator(&inputs, None) {
        Ok(rows) => {
            let (outputs, output_lens, num_outputs) = pack_outputs(rows, None);
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

/// Frees a state handle returned by `min_indicator()` (or one of the
/// `states[i]` entries from `min_simd_by_assets()`/`min_simd_by_options()`).
/// Call this once you're done streaming (after your last `min_batch()` call).
///
/// # Safety
/// `state` must be a live pointer previously returned by `min_indicator()`
/// or found in a `CSimdResult::states` array, and must not be used again
/// after this call.
#[no_mangle]
pub unsafe extern "C" fn min_state_free(state: *mut c_void) {
    if !state.is_null() {
        drop(Box::from_raw(state as *mut MinStateHandle));
    }
}

/// Computes MIN for `N` assets simultaneously (SIMD), sharing a single
/// options array. `num_assets` must be 2, 4, 8, or 16 -- anything else
/// returns `InvalidInputs`.
///
/// `inputs` must point to `num_assets` pointers, each itself pointing to
/// `INPUTS` (1) pointer: `real`, each `data_len` `f64`s long. `options`
/// must point to `OPTIONS` (1) shared value: `period`.
///
/// Returns a `CSimdResult` with `num_assets` results, each with its own
/// output rows and its own ordinary `min` continuation state (reusable
/// with `min_batch()`/`min_state_free()`). Free each state via
/// `min_state_free()`, then free the rest via `tulip_ffi_simd_result_free()`.
///
/// # Safety
/// - `inputs` must point to `num_assets` valid pointers, each pointing to
///   `INPUTS` valid non-null `*const f64`s, each pointing to `data_len` valid
///   `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s.
#[no_mangle]
pub unsafe extern "C" fn min_simd_by_assets(
    inputs: *const *const *const f64,
    num_assets: usize,
    data_len: usize,
    options: *const f64,
    _optional_outputs: *const bool,
    _num_optional: usize,
) -> CSimdResult {
    match num_assets {
        2 => min_simd_by_assets_n::<2>(inputs, data_len, options),
        4 => min_simd_by_assets_n::<4>(inputs, data_len, options),
        8 => min_simd_by_assets_n::<8>(inputs, data_len, options),
        16 => min_simd_by_assets_n::<16>(inputs, data_len, options),
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn min_simd_by_assets_n<const N: usize>(
    inputs: *const *const *const f64,
    data_len: usize,
    options: *const f64,
) -> CSimdResult {
    // `owned` holds the per-asset input slices; `refs` borrows from it, so
    // both must live in this stack frame for the duration of the call.
    let owned = read_simd_assets_inputs::<N, INPUTS>(inputs, data_len);
    let refs: [&[&[f64]; INPUTS]; N] = std::array::from_fn(|i| &owned[i]);
    let options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);

    match Min::indicator_by_assets::<N>(&refs, &options, None) {
        Ok((results, states)) => {
            let (outputs, output_lens, num_outputs, num_results) = pack_simd_outputs(results, None);
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

/// Computes MIN for one asset with `N` different option sets
/// simultaneously (SIMD). `num_option_sets` must be 2, 4, 8, or 16 --
/// anything else returns `InvalidInputs`.
///
/// `inputs` must point to `INPUTS` (1) pointer: `real`, each `data_len` `f64`s long,
/// shared across all option sets. `options` must point to `num_option_sets`
/// pointers, each itself pointing to `OPTIONS` (1) value: `period`.
///
/// Returns a `CSimdResult` with `num_option_sets` results, each with its
/// own output rows and its own ordinary `min` continuation state
/// (reusable with `min_batch()`/`min_state_free()`). Free each state
/// via `min_state_free()`, then free the rest via
/// `tulip_ffi_simd_result_free()`.
///
/// # Safety
/// - `inputs` must point to `INPUTS` valid non-null `*const f64`s, each
///   pointing to `data_len` valid `f64`s.
/// - `options` must point to `num_option_sets` valid pointers, each
///   pointing to `OPTIONS` valid `f64`s.
#[no_mangle]
pub unsafe extern "C" fn min_simd_by_options(
    inputs: *const *const f64,
    data_len: usize,
    options: *const *const f64,
    num_option_sets: usize,
    _optional_outputs: *const bool,
    _num_optional: usize,
) -> CSimdResult {
    match num_option_sets {
        2 => min_simd_by_options_n::<2>(inputs, data_len, options),
        4 => min_simd_by_options_n::<4>(inputs, data_len, options),
        8 => min_simd_by_options_n::<8>(inputs, data_len, options),
        16 => min_simd_by_options_n::<16>(inputs, data_len, options),
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn min_simd_by_options_n<const N: usize>(
    inputs: *const *const f64,
    data_len: usize,
    options: *const *const f64,
) -> CSimdResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let options = read_simd_options::<N, OPTIONS>(options);

    match Min::indicator_by_options::<N>(&inputs, &options, None) {
        Ok((results, states)) => {
            let (outputs, output_lens, num_outputs, num_results) = pack_simd_outputs(results, None);
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
    fn test_min_indicator() {
        unsafe {
            use crate::common::test::build_synthetic_data;

            let data_len = 20;
            let real_arr: Vec<f64> = build_synthetic_data(data_len).map(|x| x * 100.0).collect();

            let inputs_arr: [*const f64; INPUTS] = [real_arr.as_ptr()];
            let inputs: *const *const f64 = inputs_arr.as_ptr();
            let options_arr: [f64; OPTIONS] = [5.0];
            let options: *const f64 = options_arr.as_ptr();

            let result = min_indicator(inputs, data_len, options, ptr::null(), 0);

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_outputs, 1); // min only
            assert!(!result.state.is_null());

            tulip_ffi_result_free(result);
        }
    }

    #[test]
    fn test_min_batch() {
        unsafe {
            use crate::common::test::build_synthetic_data;

            let data_len = 20;
            let real_arr: Vec<f64> = build_synthetic_data(data_len).map(|x| x * 100.0).collect();

            let inputs_arr: [*const f64; INPUTS] = [real_arr.as_ptr()];
            let inputs: *const *const f64 = inputs_arr.as_ptr();
            let options_arr: [f64; OPTIONS] = [5.0];
            let options: *const f64 = options_arr.as_ptr();

            let result = min_indicator(inputs, data_len, options, ptr::null(), 0);
            assert_eq!(result.error, CIndicatorError::Ok);

            // Second batch call with exactly 10 elements (batch length)
            let new_real_arr: Vec<f64> = build_synthetic_data(10).map(|x| x * 100.0).collect();

            let new_inputs_arr: [*const f64; INPUTS] = [new_real_arr.as_ptr()];
            let new_inputs: *const *const f64 = new_inputs_arr.as_ptr();

            let batch_result = min_batch(result.state, new_inputs, 10, ptr::null(), 0);
            assert_eq!(batch_result.error, CIndicatorError::Ok);

            tulip_ffi_batch_result_free(batch_result);
            min_state_free(result.state);
        }
    }

    #[test]
    fn test_min_simd_by_assets() {
        unsafe {
            use crate::common::test::build_synthetic_data;

            let data_len = 20;
            let num_assets = 2;

            // Create inputs for 2 assets
            let asset0_real: Vec<f64> = build_synthetic_data(data_len).map(|x| x * 100.0).collect();
            let asset1_real: Vec<f64> = build_synthetic_data(data_len).map(|x| x * 100.0).collect();

            let inputs_arr0: [*const f64; INPUTS] = [asset0_real.as_ptr()];
            let inputs_arr1: [*const f64; INPUTS] = [asset1_real.as_ptr()];

            let assets_inputs: [*const *const f64; 2] =
                [inputs_arr0.as_ptr(), inputs_arr1.as_ptr()];
            let inputs: *const *const *const f64 = assets_inputs.as_ptr();

            let options_arr: [f64; OPTIONS] = [5.0];
            let options: *const f64 = options_arr.as_ptr();

            let result = min_simd_by_assets(inputs, num_assets, data_len, options, ptr::null(), 0);

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, 2);
            assert_eq!(result.num_outputs, 1); // min only
            assert!(!result.states.is_null());

            tulip_ffi_simd_result_free(result);
        }
    }

    #[test]
    fn test_min_simd_by_options() {
        unsafe {
            use crate::common::test::build_synthetic_data;

            let data_len = 20;

            let real_arr: Vec<f64> = build_synthetic_data(data_len).map(|x| x * 100.0).collect();

            let inputs_arr: [*const f64; INPUTS] = [real_arr.as_ptr()];
            let inputs: *const *const f64 = inputs_arr.as_ptr();

            // Create 2 different option sets
            let options0: [f64; OPTIONS] = [5.0];
            let options1: [f64; OPTIONS] = [10.0];
            let options_arr: [*const f64; 2] = [options0.as_ptr(), options1.as_ptr()];
            let options: *const *const f64 = options_arr.as_ptr();

            let result = min_simd_by_options(inputs, data_len, options, 2, ptr::null(), 0);

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, 2);
            assert_eq!(result.num_outputs, 1); // min only
            assert!(!result.states.is_null());

            tulip_ffi_simd_result_free(result);
        }
    }

    #[test]
    fn test_min_info() {
        let info = min_info();
        assert!(info.inputs.len > 0);
        assert_eq!(info.options.len, OPTIONS);
        assert!(info.outputs.len > 0);
        assert_eq!(info.optional_outputs.len, 0);
    }

    #[test]
    fn test_min_min_data() {
        unsafe {
            let options: [f64; OPTIONS] = [5.0];
            let min = min_min_data(options.as_ptr());
            assert!(min > 0);
        }
    }
}
