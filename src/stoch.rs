//! extern "C" wrapper for `stoch`, mirroring the core `tulip_rs` crate's
//! `Stoch::indicator` / `IndicatorState::batch_indicator` interface with a
//! Tulip-Indicators-style C calling convention: `inputs` is an array of
//! `INPUTS` pointers (one per input series, each `data_len` long, in the same
//! order as `tulip_rs::indicators::stoch::Stoch::INFO.inputs`), `options`
//! is a flat array of `OPTIONS` values -- exactly like `ti_stoch`'s
//! `double const *const *inputs, double const *options`. Output allocation
//! stays inside the indicator function (never caller-supplied).
//!
//! Parameter order convention (kept consistent across every function in
//! this crate): each pointer parameter is immediately followed by the
//! count(s) that describe it, e.g. `inputs, data_len, options, ...,
//! optional_outputs, numoptional`.
//!
//! SIMD entry points:
//! - `stoch_simd_by_assets`: compute Stochastic Oscillator for N assets simultaneously,
//!   sharing a single options array. N must be 2, 4, 8, or 16.
//! - `stoch_simd_by_options`: compute Stochastic Oscillator with N different option sets
//!   on a single asset. N must be 2, 4, 8, or 16.

use std::os::raw::c_void;

use tulip_rs::indicator_types::{Indicator, IndicatorByOptions, TIndicatorState};
use tulip_rs::indicators::stoch::{IndicatorState as StochState, Stoch, INPUTS, OPTIONS};
use tulip_rs::types::IndicatorError;

use crate::common::{
    optional_outputs_slice, pack_info, pack_outputs, pack_simd_outputs, pack_states, read_inputs,
    read_simd_assets_inputs, read_simd_options, CBatchResult, CIndicatorError, CIndicatorInfo,
    CIndicatorResult, CSimdResult,
};

/// Opaque state handle returned by `stoch_indicator()` and consumed by
/// `stoch_batch()` / `stoch_state_free()`.
pub type StochStateHandle = StochState;

/// Returns static metadata about the `stoch` indicator: its name, input
/// names, option names, and (mandatory/optional) output names, mirroring
/// `Stoch::INFO`.
///
/// The returned strings are leaked, process-lifetime C strings -- read them,
/// don't free them.
#[no_mangle]
pub extern "C" fn stoch_info() -> CIndicatorInfo {
    pack_info(&Stoch::INFO)
}

/// Returns the minimum number of bars `stoch` needs to produce any output at
/// all, given `options`.
///
/// # Safety
/// `options` must point to `OPTIONS` (3) valid `f64`s.
#[no_mangle]
pub unsafe extern "C" fn stoch_min_data(options: *const f64) -> usize {
    let options_arr: [f64; OPTIONS] = std::slice::from_raw_parts(options, OPTIONS)
        .try_into()
        .unwrap();
    Stoch::min_data(&options_arr)
}

/// Runs `stoch` over `data_len` bars.
///
/// `inputs` must point to `INPUTS` (3) pointers, in order:
/// `high, low, close`, each `data_len` `f64`s long.
/// `options` must point to `OPTIONS` (3) valid `f64`s: `k_period, k_slow, d_period`.
///
/// Returns the mandatory `stoch` outputs (`stoch_k, stoch_d`) and a fresh continuation state.
///
/// # Safety
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s (pass null + 0 to request no optional outputs).
#[no_mangle]
pub unsafe extern "C" fn stoch_indicator(
    inputs: *const *const f64,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CIndicatorResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match Stoch::indicator(&inputs, &options, optional) {
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

/// Continues a `stoch` calculation from `state`, mutating it in place so
/// it's ready for the next call. `state` must have come from
/// `stoch_indicator()` and not yet have been passed to `stoch_state_free()`.
///
/// `inputs` must point to `INPUTS` (3) pointers (`high, low, close`), each `data_len` `f64`s long.
///
/// # Safety
/// - `state` must be a live pointer previously returned by
///   `stoch_indicator()`.
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn stoch_batch(
    state: *mut c_void,
    inputs: *const *const f64,
    data_len: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CBatchResult {
    if state.is_null() {
        return CBatchResult::err(IndicatorError::InvalidIndicatorState);
    }
    let state = &mut *(state as *mut StochStateHandle);

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

/// Frees a state handle returned by `stoch_indicator()` (or one of the
/// `states[i]` entries from `stoch_simd_by_assets()` or `stoch_simd_by_options()`).
/// Call this once you're done streaming (after your last `stoch_batch()` call).
///
/// # Safety
/// `state` must be a live pointer previously returned by `stoch_indicator()`
/// or found in a `CSimdResult::states` array, and must not be used again
/// after this call.
#[no_mangle]
pub unsafe extern "C" fn stoch_state_free(state: *mut c_void) {
    if !state.is_null() {
        drop(Box::from_raw(state as *mut StochStateHandle));
    }
}

/// Computes Stochastic Oscillator for `N` assets simultaneously (SIMD), sharing a single
/// options array. `num_assets` must be 2, 4, 8, or 16 -- anything else
/// returns `InvalidInputs`.
///
/// `inputs` must point to `num_assets` pointers, each itself pointing to
/// `INPUTS` (3) pointers (`high, low, close`), each `data_len` `f64`s long.
///
/// Returns a `CSimdResult` with `num_assets` results, each with its own
/// output rows and its own ordinary `stoch` continuation state (reusable
/// with `stoch_batch()`/`stoch_state_free()`). Free each state via
/// `stoch_state_free()`, then free the rest via `tulip_ffi_simd_result_free()`.
///
/// # Safety
/// - `inputs` must point to `num_assets` valid pointers, each pointing to
///   `INPUTS` valid non-null `*const f64`s, each pointing to `data_len` valid
///   `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn stoch_simd_by_assets(
    inputs: *const *const *const f64,
    num_assets: usize,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    match num_assets {
        2 => stoch_simd_by_assets_n::<2>(inputs, data_len, options, optional_outputs, numoptional),
        4 => stoch_simd_by_assets_n::<4>(inputs, data_len, options, optional_outputs, numoptional),
        8 => stoch_simd_by_assets_n::<8>(inputs, data_len, options, optional_outputs, numoptional),
        16 => {
            stoch_simd_by_assets_n::<16>(inputs, data_len, options, optional_outputs, numoptional)
        }
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn stoch_simd_by_assets_n<const N: usize>(
    inputs: *const *const *const f64,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    let owned = read_simd_assets_inputs::<N, INPUTS>(inputs, data_len);
    let refs: [&[&[f64]; INPUTS]; N] = std::array::from_fn(|i| &owned[i]);
    let options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match Stoch::indicator_by_assets::<N>(&refs, &options, optional) {
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

/// Computes Stochastic Oscillator with `N` different option sets on a single asset (SIMD).
/// `num_option_sets` must be 2, 4, 8, or 16 -- anything else returns `InvalidInputs`.
///
/// `inputs` must point to `INPUTS` (3) pointers (`high, low, close`), each `data_len` `f64`s long.
/// `options` must point to `num_option_sets` pointers, each itself pointing to
/// `OPTIONS` (3) valid `f64`s.
///
/// Returns a `CSimdResult` with `num_option_sets` results. Free states via
/// `stoch_state_free()`, then free the rest via `tulip_ffi_simd_result_free()`.
///
/// # Safety
/// - `inputs` must point to `INPUTS` valid non-null `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `options` must point to `num_option_sets` valid pointers, each pointing to
///   `OPTIONS` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn stoch_simd_by_options(
    inputs: *const *const f64,
    data_len: usize,
    options: *const *const f64,
    num_option_sets: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    match num_option_sets {
        2 => stoch_simd_by_options_n::<2>(inputs, data_len, options, optional_outputs, numoptional),
        4 => stoch_simd_by_options_n::<4>(inputs, data_len, options, optional_outputs, numoptional),
        8 => stoch_simd_by_options_n::<8>(inputs, data_len, options, optional_outputs, numoptional),
        16 => {
            stoch_simd_by_options_n::<16>(inputs, data_len, options, optional_outputs, numoptional)
        }
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn stoch_simd_by_options_n<const N: usize>(
    inputs: *const *const f64,
    data_len: usize,
    options: *const *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let simd_options = read_simd_options::<N, OPTIONS>(options);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match Stoch::indicator_by_options::<N>(&inputs, &simd_options, optional) {
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
    fn test_stoch_indicator() {
        unsafe {
            let data_len = 60;
            let high_arr: Vec<f64> = build_synthetic_data(data_len).map(|x| x * 1.1).collect();
            let low_arr: Vec<f64> = build_synthetic_data(data_len).map(|x| x * 0.9).collect();
            let close_arr: Vec<f64> = build_synthetic_data(data_len);

            let inputs_arr: [*const f64; INPUTS] =
                [high_arr.as_ptr(), low_arr.as_ptr(), close_arr.as_ptr()];
            let inputs: *const *const f64 = inputs_arr.as_ptr();

            let options_arr: [f64; OPTIONS] = [14.0, 3.0, 3.0];
            let options: *const f64 = options_arr.as_ptr();

            let result = stoch_indicator(inputs, data_len, options, ptr::null(), 0);

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_outputs, 2); // stoch_k, stoch_d
            assert!(!result.state.is_null());

            tulip_ffi_result_free(result);
        }
    }

    #[test]
    fn test_stoch_batch() {
        unsafe {
            let data_len = 60;
            let high_arr: Vec<f64> = build_synthetic_data(data_len).map(|x| x * 1.1).collect();
            let low_arr: Vec<f64> = build_synthetic_data(data_len).map(|x| x * 0.9).collect();
            let close_arr: Vec<f64> = build_synthetic_data(data_len);

            let inputs_arr: [*const f64; INPUTS] =
                [high_arr.as_ptr(), low_arr.as_ptr(), close_arr.as_ptr()];
            let inputs: *const *const f64 = inputs_arr.as_ptr();

            let options_arr: [f64; OPTIONS] = [14.0, 3.0, 3.0];
            let options: *const f64 = options_arr.as_ptr();

            let result = stoch_indicator(inputs, data_len, options, ptr::null(), 0);
            assert_eq!(result.error, CIndicatorError::Ok);

            let new_high_arr: Vec<f64> = build_synthetic_data(10).map(|x| x * 1.15).collect();
            let new_low_arr: Vec<f64> = build_synthetic_data(10).map(|x| x * 0.95).collect();
            let new_close_arr: Vec<f64> = build_synthetic_data(10);

            let new_inputs_arr_ptr: [*const f64; INPUTS] = [
                new_high_arr.as_ptr(),
                new_low_arr.as_ptr(),
                new_close_arr.as_ptr(),
            ];
            let new_inputs: *const *const f64 = new_inputs_arr_ptr.as_ptr();

            let batch_result = stoch_batch(result.state, new_inputs, 10, ptr::null(), 0);
            assert_eq!(batch_result.error, CIndicatorError::Ok);

            tulip_ffi_batch_result_free(batch_result);
            stoch_state_free(result.state);
        }
    }

    #[test]
    fn test_stoch_simd_by_assets() {
        unsafe {
            let data_len = 60;
            let num_assets = 4;

            // Create inputs for 4 assets
            let asset0_high: Vec<f64> = build_synthetic_data(data_len).map(|x| x * 1.1).collect();
            let asset0_low: Vec<f64> = build_synthetic_data(data_len).map(|x| x * 0.9).collect();
            let asset0_close: Vec<f64> = build_synthetic_data(data_len);

            let asset1_high: Vec<f64> = build_synthetic_data(data_len).map(|x| x * 1.5).collect();
            let asset1_low: Vec<f64> = build_synthetic_data(data_len).map(|x| x * 1.3).collect();
            let asset1_close: Vec<f64> = build_synthetic_data(data_len);

            let asset2_high: Vec<f64> = build_synthetic_data(data_len).map(|x| x * 2.0).collect();
            let asset2_low: Vec<f64> = build_synthetic_data(data_len).map(|x| x * 1.8).collect();
            let asset2_close: Vec<f64> = build_synthetic_data(data_len);

            let asset3_high: Vec<f64> = build_synthetic_data(data_len).map(|x| x * 2.5).collect();
            let asset3_low: Vec<f64> = build_synthetic_data(data_len).map(|x| x * 2.3).collect();
            let asset3_close: Vec<f64> = build_synthetic_data(data_len);

            let inputs_arr0: [*const f64; INPUTS] = [
                asset0_high.as_ptr(),
                asset0_low.as_ptr(),
                asset0_close.as_ptr(),
            ];
            let inputs_arr1: [*const f64; INPUTS] = [
                asset1_high.as_ptr(),
                asset1_low.as_ptr(),
                asset1_close.as_ptr(),
            ];
            let inputs_arr2: [*const f64; INPUTS] = [
                asset2_high.as_ptr(),
                asset2_low.as_ptr(),
                asset2_close.as_ptr(),
            ];
            let inputs_arr3: [*const f64; INPUTS] = [
                asset3_high.as_ptr(),
                asset3_low.as_ptr(),
                asset3_close.as_ptr(),
            ];

            let assets_inputs: [*const *const f64; 4] = [
                inputs_arr0.as_ptr(),
                inputs_arr1.as_ptr(),
                inputs_arr2.as_ptr(),
                inputs_arr3.as_ptr(),
            ];
            let inputs: *const *const *const f64 = assets_inputs.as_ptr();

            let options_arr: [f64; OPTIONS] = [14.0, 3.0, 3.0];
            let options: *const f64 = options_arr.as_ptr();

            let result =
                stoch_simd_by_assets(inputs, num_assets, data_len, options, ptr::null(), 0);

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, 4);
            assert_eq!(result.num_outputs, 2); // stoch_k, stoch_d
            assert!(!result.states.is_null());

            tulip_ffi_simd_result_free(result);
        }
    }

    #[test]
    fn test_stoch_simd_by_options() {
        unsafe {
            const NUM_OPTION_SETS: usize = 2;

            let data_len = 60;

            let high_arr: Vec<f64> = build_synthetic_data(data_len).map(|x| x * 1.1).collect();
            let low_arr: Vec<f64> = build_synthetic_data(data_len).map(|x| x * 0.9).collect();
            let close_arr: Vec<f64> = build_synthetic_data(data_len);

            let inputs_arr: [*const f64; INPUTS] =
                [high_arr.as_ptr(), low_arr.as_ptr(), close_arr.as_ptr()];
            let inputs: *const *const f64 = inputs_arr.as_ptr();

            // Two different option sets
            let options1: [f64; OPTIONS] = [14.0, 3.0, 3.0];
            let options2: [f64; OPTIONS] = [25.0, 5.0, 5.0];
            let options_arr: [*const f64; NUM_OPTION_SETS] = [options1.as_ptr(), options2.as_ptr()];
            let options: *const *const f64 = options_arr.as_ptr();

            let result =
                stoch_simd_by_options(inputs, data_len, options, NUM_OPTION_SETS, ptr::null(), 0);

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, 2);
            assert_eq!(result.num_outputs, 2); // stoch_k, stoch_d
            assert!(!result.states.is_null());

            tulip_ffi_simd_result_free(result);
        }
    }

    #[test]
    fn test_stoch_info() {
        let info = stoch_info();
        assert!(info.inputs.len > 0);
        assert_eq!(info.options.len, OPTIONS);
        assert!(info.outputs.len > 0);
        assert_eq!(info.optional_outputs.len, 0);
    }

    #[test]
    fn test_stoch_min_data() {
        unsafe {
            let options: [f64; OPTIONS] = [14.0, 3.0, 3.0];
            let min = stoch_min_data(options.as_ptr());
            assert!(min > 0);
        }
    }
}
