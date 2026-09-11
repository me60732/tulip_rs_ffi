//! extern "C" wrapper for `max`, mirroring the core `tulip_rs` crate's
//! `Max::indicator` / `IndicatorState::batch_indicator` interface with a
//! Tulip-Indicators-style C calling convention: `inputs` is an array of
//! `INPUTS` pointers (one per input series, each `data_len` long), `options`
//! is a flat array of `OPTIONS` values.
//!
//! Parameter order convention: each pointer parameter is immediately followed by the
//! count(s) that describe it.
//!
//! SIMD entry points:
//! - `max_simd_by_assets`: compute MAX for N assets simultaneously,
//!   sharing a single options array. N must be 2, 4, 8, or 16.
//! - `max_simd_by_options`: compute MAX for one asset with N different
//!   option sets simultaneously. N must be 2, 4, 8, or 16.

use std::os::raw::c_void;

use tulip_rs::indicator_types::{Indicator, IndicatorByOptions, TIndicatorState};
use tulip_rs::indicators::max::{IndicatorState as MaxState, Max, INPUTS, OPTIONS};
use tulip_rs::types::IndicatorError;

use crate::common::{
    optional_outputs_slice, pack_info, pack_outputs, pack_simd_outputs, pack_states, read_inputs,
    read_simd_assets_inputs, read_simd_options, CBatchResult, CIndicatorError, CIndicatorInfo,
    CIndicatorResult, CSimdResult,
};

/// Opaque state handle returned by `max_indicator()` and consumed by
/// `max_batch()` / `max_state_free()`.
pub type MaxStateHandle = MaxState;

/// Returns static metadata about the `max` indicator: its name, input
/// names, option names, and (mandatory/optional) output names, mirroring
/// `Max::INFO`.
///
/// The returned strings are leaked, process-lifetime C strings -- read them,
/// don't free them.
#[no_mangle]
pub extern "C" fn max_info() -> CIndicatorInfo {
    pack_info(&Max::INFO)
}

/// Returns the minimum number of bars `max` needs to produce any output at
/// all, given `options`.
///
/// # Safety
/// `options` must point to `OPTIONS` (1) valid `f64`s.
#[no_mangle]
pub unsafe extern "C" fn max_min_data(options: *const f64) -> usize {
    let options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    Max::min_data(&options)
}

/// Runs `max` over `data_len` bars.
///
/// `inputs` must point to `INPUTS` (1) pointer: `real`, `data_len` `f64`s long.
/// `options` must point to `OPTIONS` (1) value: `period`.
///
/// Returns the mandatory `max` output plus any requested optional outputs
/// (none for this indicator) and a fresh continuation state.
///
/// # Safety
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s (pass null + 0 to request no optional outputs).
#[no_mangle]
pub unsafe extern "C" fn max_indicator(
    inputs: *const *const f64,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CIndicatorResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match Max::indicator(&inputs, &options, optional) {
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

/// Continues a `max` calculation from `state`, mutating it in place so
/// it's ready for the next call. `state` must have come from
/// `max_indicator()` and not yet have been passed to `max_state_free()`.
///
/// `inputs` must point to `INPUTS` (1) pointer: `real`, `data_len` `f64`s long.
///
/// # Safety
/// - `state` must be a live pointer previously returned by
///   `max_indicator()`.
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn max_batch(
    state: *mut c_void,
    inputs: *const *const f64,
    data_len: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CBatchResult {
    if state.is_null() {
        return CBatchResult::err(IndicatorError::InvalidIndicatorState);
    }
    let state = &mut *(state as *mut MaxStateHandle);

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

/// Frees a state handle returned by `max_indicator()` (or one of the
/// `states[i]` entries from `max_simd_by_assets()`/`max_simd_by_options()`).
/// Call this once you're done streaming (after your last `max_batch()` call).
///
/// # Safety
/// `state` must be a live pointer previously returned by `max_indicator()`
/// or found in a `CSimdResult::states` array, and must not be used again
/// after this call.
#[no_mangle]
pub unsafe extern "C" fn max_state_free(state: *mut c_void) {
    if !state.is_null() {
        drop(Box::from_raw(state as *mut MaxStateHandle));
    }
}

/// Computes MAX for `N` assets simultaneously (SIMD), sharing a single
/// options array. `num_assets` must be 2, 4, 8, or 16 -- anything else
/// returns `InvalidInputs`.
///
/// `inputs` must point to `num_assets` pointers, each itself pointing to
/// `INPUTS` (1) pointer (`real`), `data_len` `f64`s long. `options` must point
/// to `OPTIONS` (1) shared value.
///
/// Returns a `CSimdResult` with `num_assets` results, each with its own
/// output rows and its own ordinary `max` continuation state (reusable
/// with `max_batch()`/`max_state_free()`). Free each state via
/// `max_state_free()`, then free the rest via `tulip_ffi_simd_result_free()`.
///
/// # Safety
/// - `inputs` must point to `num_assets` valid pointers, each pointing to
///   `INPUTS` valid non-null `*const f64`s, each pointing to `data_len` valid
///   `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn max_simd_by_assets(
    inputs: *const *const *const f64,
    num_assets: usize,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    match num_assets {
        2 => max_simd_by_assets_n::<2>(inputs, data_len, options, optional_outputs, numoptional),
        4 => max_simd_by_assets_n::<4>(inputs, data_len, options, optional_outputs, numoptional),
        8 => max_simd_by_assets_n::<8>(inputs, data_len, options, optional_outputs, numoptional),
        16 => max_simd_by_assets_n::<16>(inputs, data_len, options, optional_outputs, numoptional),
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn max_simd_by_assets_n<const N: usize>(
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

    match Max::indicator_by_assets::<N>(&refs, &options, optional) {
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

/// Computes MAX for one asset with `N` different option sets
/// simultaneously (SIMD). `num_option_sets` must be 2, 4, 8, or 16 --
/// anything else returns `InvalidInputs`.
///
/// `inputs` must point to `INPUTS` (1) pointer (`real`), `data_len` `f64`s
/// long, shared across all option sets. `options` must point to
/// `num_option_sets` pointers, each itself pointing to `OPTIONS` (1) value.
///
/// Returns a `CSimdResult` with `num_option_sets` results, each with its
/// own output rows and its own ordinary `max` continuation state
/// (reusable with `max_batch()`/`max_state_free()`). Free each state
/// via `max_state_free()`, then free the rest via
/// `tulip_ffi_simd_result_free()`.
///
/// # Safety
/// - `inputs` must point to `INPUTS` valid non-null `*const f64`s, each
///   pointing to `data_len` valid `f64`s.
/// - `options` must point to `num_option_sets` valid pointers, each
///   pointing to `OPTIONS` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn max_simd_by_options(
    inputs: *const *const f64,
    data_len: usize,
    options: *const *const f64,
    num_option_sets: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    match num_option_sets {
        2 => max_simd_by_options_n::<2>(inputs, data_len, options, optional_outputs, numoptional),
        4 => max_simd_by_options_n::<4>(inputs, data_len, options, optional_outputs, numoptional),
        8 => max_simd_by_options_n::<8>(inputs, data_len, options, optional_outputs, numoptional),
        16 => {
            max_simd_by_options_n::<16>(inputs, data_len, options, optional_outputs, numoptional)
        }
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn max_simd_by_options_n<const N: usize>(
    inputs: *const *const f64,
    data_len: usize,
    options: *const *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let options = read_simd_options::<N, OPTIONS>(options);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match Max::indicator_by_options::<N>(&inputs, &options, optional) {
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
    use crate::common::{
        tulip_ffi_batch_result_free, tulip_ffi_result_free, tulip_ffi_simd_result_free,
    };

    #[test]
    fn test_max_info() {
        let info = max_info();
        assert!(info.inputs.len > 0);
        assert_eq!(info.options.len, 1);
        assert!(info.outputs.len > 0);
    }

    #[test]
    fn test_max_min_data() {
        unsafe {
            let options: [f64; OPTIONS] = [20.0];
            let min = max_min_data(options.as_ptr());
            assert!(min > 0);
        }
    }

    #[test]
    fn test_max_indicator() {
        unsafe {
            let data_len = 60;
            let inputs_arr = build_synthetic_data(data_len);
            let inputs_ptr = inputs_arr.as_ptr();
            let inputs = &inputs_ptr as *const *const f64;

            let options: [f64; OPTIONS] = [20.0];
            let options_ptr = options.as_ptr();

            // No optional outputs for max
            let result = max_indicator(inputs, data_len, options_ptr, std::ptr::null(), 0);

            assert_eq!(result.error, CIndicatorError::Ok);
            // max has 1 output: max
            assert_eq!(result.num_outputs, 1);

            tulip_ffi_result_free(result);
        }
    }

    #[test]
    fn test_max_batch() {
        unsafe {
            let data_len = 60;
            let inputs_arr = build_synthetic_data(data_len);
            let inputs_ptr = inputs_arr.as_ptr();
            let inputs = &inputs_ptr as *const *const f64;

            let options: [f64; OPTIONS] = [20.0];
            let options_ptr = options.as_ptr();

            // First call to get state
            let result = max_indicator(inputs, data_len, options_ptr, std::ptr::null(), 0);
            assert_eq!(result.error, CIndicatorError::Ok);

            let state = result.state;

            // Second call with batch
            let inputs_arr2 = build_synthetic_data(data_len);
            let inputs_ptr2 = inputs_arr2.as_ptr();
            let inputs2 = &inputs_ptr2 as *const *const f64;

            let batch_result = max_batch(state, inputs2, data_len, std::ptr::null(), 0);
            assert_eq!(batch_result.error, CIndicatorError::Ok);

            tulip_ffi_batch_result_free(batch_result);
            max_state_free(state);
        }
    }

    #[test]
    fn test_max_simd_by_assets() {
        unsafe {
            let num_assets = 2;
            let data_len = 60;

            // Build inputs for 2 assets: [asset1_inputs, asset2_inputs]
            let inputs_arr1 = build_synthetic_data(data_len);
            let inputs_arr2 = build_synthetic_data(data_len);

            let inputs_ptr1 = inputs_arr1.as_ptr();
            let inputs_ptr2 = inputs_arr2.as_ptr();

            // Each asset has INPUTS=1 pointer
            let asset1_inputs = &inputs_ptr1 as *const *const f64;
            let asset2_inputs = &inputs_ptr2 as *const *const f64;

            // Array of asset pointers
            let assets_array = [asset1_inputs, asset2_inputs];
            let inputs = assets_array.as_ptr();

            let options: [f64; OPTIONS] = [20.0];
            let options_ptr = options.as_ptr();

            let result = max_simd_by_assets(
                inputs,
                num_assets,
                data_len,
                options_ptr,
                std::ptr::null(),
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, num_assets);

            // Free each state
            for i in 0..num_assets {
                let state_ptr = *(result.states.add(i));
                max_state_free(state_ptr);
            }

            tulip_ffi_simd_result_free(result);
        }
    }

    #[test]
    fn test_max_simd_by_options() {
        unsafe {
            let num_option_sets = 2;
            let data_len = 60;

            let inputs_arr = build_synthetic_data(data_len);
            let inputs_ptr = inputs_arr.as_ptr();
            let inputs = &inputs_ptr as *const *const f64;

            // Build 2 different option sets
            let options1: [f64; OPTIONS] = [20.0];
            let options2: [f64; OPTIONS] = [30.0];

            let options_ptr1 = options1.as_ptr();
            let options_ptr2 = options2.as_ptr();

            // Array of option set pointers
            let options_array = [options_ptr1, options_ptr2];
            let options = options_array.as_ptr();

            let result = max_simd_by_options(
                inputs,
                data_len,
                options,
                num_option_sets,
                std::ptr::null(),
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, num_option_sets);

            // Free each state
            for i in 0..num_option_sets {
                let state_ptr = *(result.states.add(i));
                max_state_free(state_ptr);
            }

            tulip_ffi_simd_result_free(result);
        }
    }
}
