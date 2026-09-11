//! extern "C" wrapper for `roofingfilter`, mirroring the core `tulip_rs` crate's
//! `RoofingFilter::indicator` / `IndicatorState::batch_indicator` interface with a
//! Tulip-Indicators-style C calling convention: `inputs` is an array of
//! `INPUTS` pointers (one per input series, each `data_len` long, in the same
//! order as `tulip_rs::indicators::roofingfilter::RoofingFilter::INFO.inputs`), `options`
//! is a flat array of `OPTIONS` values -- exactly like `ti_roofingfilter`'s
//! `double const *const *inputs, double const *options`. Output allocation
//! stays inside the indicator function (never caller-supplied).
//!
//! Parameter order convention (kept consistent across every function in
//! this crate): each pointer parameter is immediately followed by the
//! count(s) that describe it, e.g. `inputs, data_len, options, ...,
//! optional_outputs, numoptional`.
//!
//! SIMD entry points:
//! - `roofingfilter_simd_by_assets`: compute RoofingFilter for N assets simultaneously,
//!   sharing a single options array. N must be 2, 4, 8, or 16.
//! - `roofingfilter_simd_by_options`: compute RoofingFilter for N option sets simultaneously
//!   across a single asset batch. N must be 2, 4, 8, or 16.

use std::os::raw::c_void;

use tulip_rs::indicator_types::{Indicator, IndicatorByOptions, TIndicatorState};
use tulip_rs::indicators::roofingfilter::{
    IndicatorState as RoofingFilterState, RoofingFilter, INPUTS, OPTIONS,
};
use tulip_rs::types::IndicatorError;

use crate::common::{
    optional_outputs_slice, pack_info, pack_outputs, pack_simd_outputs, pack_states, read_inputs,
    read_simd_assets_inputs, read_simd_options, CBatchResult, CIndicatorError, CIndicatorInfo,
    CIndicatorResult, CSimdResult,
};

/// Opaque state handle returned by `roofingfilter_indicator()` and consumed by
/// `roofingfilter_batch()` / `roofingfilter_state_free()`.
pub type RoofingFilterStateHandle = RoofingFilterState;

/// Returns static metadata about the `roofingfilter` indicator: its name, input
/// names, option names, and (mandatory/optional) output names, mirroring
/// `RoofingFilter::INFO`.
///
/// The returned strings are leaked, process-lifetime C strings -- read them,
/// don't free them.
#[no_mangle]
pub extern "C" fn roofingfilter_info() -> CIndicatorInfo {
    pack_info(&RoofingFilter::INFO)
}

/// Returns the minimum number of bars `roofingfilter` needs to produce any output at
/// all, given `options`.
///
/// # Safety
/// `options` must point to `OPTIONS` (2) valid `f64`s.
#[no_mangle]
pub unsafe extern "C" fn roofingfilter_min_data(options: *const f64) -> usize {
    let options = std::slice::from_raw_parts(options, OPTIONS);
    RoofingFilter::min_data(&options.try_into().unwrap())
}

/// Runs `roofingfilter` over `data_len` bars.
///
/// `inputs` must point to `INPUTS` (1) pointer: `real`, each `data_len` `f64`s long.
/// `options` must point to `OPTIONS` (2) valid `f64`s: `ss_period, hp_period`.
///
/// Returns the mandatory `roofingfilter` output (`roofing`) and optional output (`highpass`),
/// plus a fresh continuation state.
///
/// # Safety
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s (pass null + 0 to request no optional outputs).
#[no_mangle]
pub unsafe extern "C" fn roofingfilter_indicator(
    inputs: *const *const f64,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CIndicatorResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let options_slice = std::slice::from_raw_parts(options, OPTIONS);
    let options_array: [f64; OPTIONS] = options_slice.try_into().unwrap();
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match RoofingFilter::indicator(&inputs, &options_array, optional) {
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

/// Continues a `roofingfilter` calculation from `state`, mutating it in place so
/// it's ready for the next call. `state` must have come from
/// `roofingfilter_indicator()` and not yet have been passed to `roofingfilter_state_free()`.
///
/// `inputs` must point to `INPUTS` (1) pointer (`real`), each `data_len` `f64`s long.
///
/// # Safety
/// - `state` must be a live pointer previously returned by
///   `roofingfilter_indicator()`.
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn roofingfilter_batch(
    state: *mut c_void,
    inputs: *const *const f64,
    data_len: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CBatchResult {
    if state.is_null() {
        return CBatchResult::err(IndicatorError::InvalidIndicatorState);
    }
    let state = &mut *(state as *mut RoofingFilterStateHandle);

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

/// Frees a state handle returned by `roofingfilter_indicator()` (or one of the
/// `states[i]` entries from `roofingfilter_simd_by_assets()` or `roofingfilter_simd_by_options()`).
/// Call this once you're done streaming (after your last `roofingfilter_batch()` call).
///
/// # Safety
/// `state` must be a live pointer previously returned by `roofingfilter_indicator()`
/// or found in a `CSimdResult::states` array, and must not be used again
/// after this call.
#[no_mangle]
pub unsafe extern "C" fn roofingfilter_state_free(state: *mut c_void) {
    if !state.is_null() {
        drop(Box::from_raw(state as *mut RoofingFilterStateHandle));
    }
}

/// Computes RoofingFilter for `N` assets simultaneously (SIMD), sharing a single
/// options array. `num_assets` must be 2, 4, 8, or 16 -- anything else
/// returns `InvalidInputs`.
///
/// `inputs` must point to `num_assets` pointers, each itself pointing to
/// `INPUTS` (1) pointer (`real`), each `data_len` `f64`s long.
///
/// Returns a `CSimdResult` with `num_assets` results, each with its own
/// output rows and its own ordinary `roofingfilter` continuation state (reusable
/// with `roofingfilter_batch()`/`roofingfilter_state_free()`). Free each state via
/// `roofingfilter_state_free()`, then free the rest via `tulip_ffi_simd_result_free()`.
///
/// # Safety
/// - `inputs` must point to `num_assets` valid pointers, each pointing to
///   `INPUTS` valid non-null `*const f64`s, each pointing to `data_len` valid
///   `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn roofingfilter_simd_by_assets(
    inputs: *const *const *const f64,
    num_assets: usize,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    match num_assets {
        2 => roofingfilter_simd_by_assets_n::<2>(
            inputs,
            data_len,
            options,
            optional_outputs,
            numoptional,
        ),
        4 => roofingfilter_simd_by_assets_n::<4>(
            inputs,
            data_len,
            options,
            optional_outputs,
            numoptional,
        ),
        8 => roofingfilter_simd_by_assets_n::<8>(
            inputs,
            data_len,
            options,
            optional_outputs,
            numoptional,
        ),
        16 => roofingfilter_simd_by_assets_n::<16>(
            inputs,
            data_len,
            options,
            optional_outputs,
            numoptional,
        ),
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn roofingfilter_simd_by_assets_n<const N: usize>(
    inputs: *const *const *const f64,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    let owned = read_simd_assets_inputs::<N, INPUTS>(inputs, data_len);
    let refs: [&[&[f64]; INPUTS]; N] = std::array::from_fn(|i| &owned[i]);

    let options_slice = std::slice::from_raw_parts(options, OPTIONS);
    let options_array: [f64; OPTIONS] = options_slice.try_into().unwrap();
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match RoofingFilter::indicator_by_assets::<N>(&refs, &options_array, optional) {
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

/// Computes RoofingFilter for `N` option sets simultaneously (SIMD-by-options).
/// `num_option_sets` must be 2, 4, 8, or 16 -- anything else returns `InvalidInputs`.
///
/// `inputs` must point to `INPUTS` (1) pointer (`real`), each `data_len` `f64`s long.
/// `options` must point to `num_option_sets` pointers, each itself pointing to
/// `OPTIONS` (2) valid `f64`s (the `ss_period` and `hp_period` values).
///
/// Returns a `CSimdResult` with `num_option_sets` results. Free states via
/// `roofingfilter_state_free()`, then free the rest via `tulip_ffi_simd_result_free()`.
///
/// # Safety
/// - `inputs` must point to `INPUTS` valid non-null `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `options` must point to `num_option_sets` valid pointers, each pointing to
///   `OPTIONS` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn roofingfilter_simd_by_options(
    inputs: *const *const f64,
    data_len: usize,
    options: *const *const f64,
    num_option_sets: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    match num_option_sets {
        2 => roofingfilter_simd_by_options_n::<2>(
            inputs,
            data_len,
            options,
            optional_outputs,
            numoptional,
        ),
        4 => roofingfilter_simd_by_options_n::<4>(
            inputs,
            data_len,
            options,
            optional_outputs,
            numoptional,
        ),
        8 => roofingfilter_simd_by_options_n::<8>(
            inputs,
            data_len,
            options,
            optional_outputs,
            numoptional,
        ),
        16 => roofingfilter_simd_by_options_n::<16>(
            inputs,
            data_len,
            options,
            optional_outputs,
            numoptional,
        ),
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn roofingfilter_simd_by_options_n<const N: usize>(
    inputs: *const *const f64,
    data_len: usize,
    options: *const *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let simd_options = read_simd_options::<N, OPTIONS>(options);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match RoofingFilter::indicator_by_options::<N>(&inputs, &simd_options, optional) {
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
    fn test_roofingfilter_indicator() {
        unsafe {
            let data_len = 60;
            let real_arr: Vec<f64> = build_synthetic_data(data_len);

            let inputs_arr: [*const f64; INPUTS] = [real_arr.as_ptr()];
            let inputs: *const *const f64 = inputs_arr.as_ptr();

            // ss_period=10, hp_period=5
            let options_arr: [f64; OPTIONS] = [10.0, 5.0];
            let options: *const f64 = options_arr.as_ptr();

            // Request optional output (highpass)
            let optional_arr: [bool; 1] = [true];
            let optional: *const bool = optional_arr.as_ptr();

            let result = roofingfilter_indicator(inputs, data_len, options, optional, 1);

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_outputs, 2); // roofing + highpass
            assert!(!result.state.is_null());

            tulip_ffi_result_free(result);
        }
    }

    #[test]
    fn test_roofingfilter_indicator_nooptional() {
        unsafe {
            let data_len = 60;
            let real_arr: Vec<f64> = build_synthetic_data(data_len);

            let inputs_arr: [*const f64; INPUTS] = [real_arr.as_ptr()];
            let inputs: *const *const f64 = inputs_arr.as_ptr();

            // ss_period=10, hp_period=5
            let options_arr: [f64; OPTIONS] = [10.0, 5.0];
            let options: *const f64 = options_arr.as_ptr();

            let result = roofingfilter_indicator(inputs, data_len, options, ptr::null(), 0);

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_outputs, 1); // only roofing
            assert!(!result.state.is_null());

            tulip_ffi_result_free(result);
        }
    }

    #[test]
    fn test_roofingfilter_batch() {
        unsafe {
            let data_len = 60;
            let real_arr: Vec<f64> = build_synthetic_data(data_len);

            let inputs_arr: [*const f64; INPUTS] = [real_arr.as_ptr()];
            let inputs: *const *const f64 = inputs_arr.as_ptr();

            // ss_period=10, hp_period=5
            let options_arr: [f64; OPTIONS] = [10.0, 5.0];
            let options: *const f64 = options_arr.as_ptr();

            let result = roofingfilter_indicator(inputs, data_len, options, ptr::null(), 0);
            assert_eq!(result.error, CIndicatorError::Ok);

            let new_inputs_arr: Vec<f64> = build_synthetic_data(10);

            let new_inputs_arr_ptr: [*const f64; INPUTS] = [new_inputs_arr.as_ptr()];
            let new_inputs: *const *const f64 = new_inputs_arr_ptr.as_ptr();

            let batch_result = roofingfilter_batch(result.state, new_inputs, 10, ptr::null(), 0);
            assert_eq!(batch_result.error, CIndicatorError::Ok);

            tulip_ffi_batch_result_free(batch_result);
            roofingfilter_state_free(result.state);
        }
    }

    #[test]
    fn test_roofingfilter_simd_by_assets() {
        unsafe {
            let data_len = 60;
            let num_assets = 4;

            // Create inputs for 4 assets
            let asset0_real: Vec<f64> = build_synthetic_data(data_len);
            let asset1_real: Vec<f64> = build_synthetic_data(data_len).map(|x| x * 1.5).collect();
            let asset2_real: Vec<f64> = build_synthetic_data(data_len).map(|x| x * 2.0).collect();
            let asset3_real: Vec<f64> = build_synthetic_data(data_len).map(|x| x * 2.5).collect();

            let inputs_arr0: [*const f64; INPUTS] = [asset0_real.as_ptr()];
            let inputs_arr1: [*const f64; INPUTS] = [asset1_real.as_ptr()];
            let inputs_arr2: [*const f64; INPUTS] = [asset2_real.as_ptr()];
            let inputs_arr3: [*const f64; INPUTS] = [asset3_real.as_ptr()];

            let assets_inputs: [*const *const f64; 4] = [
                inputs_arr0.as_ptr(),
                inputs_arr1.as_ptr(),
                inputs_arr2.as_ptr(),
                inputs_arr3.as_ptr(),
            ];
            let inputs: *const *const *const f64 = assets_inputs.as_ptr();

            // ss_period=10, hp_period=5
            let options_arr: [f64; OPTIONS] = [10.0, 5.0];
            let options: *const f64 = options_arr.as_ptr();

            let result =
                roofingfilter_simd_by_assets(inputs, num_assets, data_len, options, ptr::null(), 0);

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, 4);
            assert_eq!(result.num_outputs, 1);
            assert!(!result.states.is_null());

            tulip_ffi_simd_result_free(result);
        }
    }

    #[test]
    fn test_roofingfilter_simd_by_options() {
        unsafe {
            let data_len = 60;
            let num_option_sets = 2;

            let real_arr: Vec<f64> = build_synthetic_data(data_len);

            let inputs_arr: [*const f64; INPUTS] = [real_arr.as_ptr()];
            let inputs: *const *const f64 = inputs_arr.as_ptr();

            // Two different option sets
            // Set 0: ss_period=10, hp_period=5
            let options0_arr: [f64; OPTIONS] = [10.0, 5.0];
            // Set 1: ss_period=15, hp_period=8
            let options1_arr: [f64; OPTIONS] = [15.0, 8.0];

            let options_arr: [*const f64; 2] = [options0_arr.as_ptr(), options1_arr.as_ptr()];
            let options: *const *const f64 = options_arr.as_ptr();

            // Request optional output for both
            let optional_arr: [bool; 2] = [true, true];
            let optional: *const bool = optional_arr.as_ptr();

            let result = roofingfilter_simd_by_options(
                inputs,
                data_len,
                options,
                num_option_sets,
                optional,
                2,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, 2);
            assert_eq!(result.num_outputs, 2); // roofing + highpass
            assert!(!result.states.is_null());

            tulip_ffi_simd_result_free(result);
        }
    }

    #[test]
    fn test_roofingfilter_info() {
        let info = roofingfilter_info();
        assert!(info.inputs.len > 0);
        assert_eq!(info.options.len, OPTIONS);
        assert!(info.outputs.len > 0);
        assert!(info.optional_outputs.len > 0);
    }

    #[test]
    fn test_roofingfilter_min_data() {
        unsafe {
            let options: [f64; OPTIONS] = [20.0, 0.5];
            let min = roofingfilter_min_data(options.as_ptr());
            assert!(min > 0);
        }
    }
}
