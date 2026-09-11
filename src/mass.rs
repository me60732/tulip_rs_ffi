//! extern "C" wrapper for `mass`, mirroring the core `tulip_rs` crate's
//! `Mass::indicator` / `IndicatorState::batch_indicator` interface with a
//! Tulip-Indicators-style C calling convention: `inputs` is an array of
//! `INPUTS` pointers (one per input series, each `data_len` long), `options`
//! is a flat array of `OPTIONS` values.
//!
//! Parameter order convention: each pointer parameter is immediately followed by the
//! count(s) that describe it.
//!
//! SIMD entry points:
//! - `mass_simd_by_assets`: compute MASS for N assets simultaneously,
//!   sharing a single options array. N must be 2, 4, 8, or 16.
//! - `mass_simd_by_options`: compute MASS for one asset with N different
//!   option sets simultaneously. N must be 2, 4, 8, or 16.

use std::os::raw::c_void;

use tulip_rs::indicator_types::{Indicator, IndicatorByOptions, TIndicatorState};
use tulip_rs::indicators::mass::{IndicatorState as MassState, Mass, INPUTS, OPTIONS};
use tulip_rs::types::IndicatorError;

use crate::common::{
    optional_outputs_slice, pack_info, pack_outputs, pack_simd_outputs, pack_states, read_inputs,
    read_simd_assets_inputs, read_simd_options, CBatchResult, CIndicatorError, CIndicatorInfo,
    CIndicatorResult, CSimdResult,
};

/// Opaque state handle returned by `mass_indicator()` and consumed by
/// `mass_batch()` / `mass_state_free()`.
pub type MassStateHandle = MassState;

/// Returns static metadata about the `mass` indicator: its name, input
/// names, option names, and (mandatory/optional) output names, mirroring
/// `Mass::INFO`.
///
/// The returned strings are leaked, process-lifetime C strings -- read them,
/// don't free them.
#[no_mangle]
pub extern "C" fn mass_info() -> CIndicatorInfo {
    pack_info(&Mass::INFO)
}

/// Returns the minimum number of bars `mass` needs to produce any output at
/// all, given `options`.
///
/// # Safety
/// `options` must point to `OPTIONS` (1) valid `f64`s.
#[no_mangle]
pub unsafe extern "C" fn mass_min_data(options: *const f64) -> usize {
    let options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    Mass::min_data(&options)
}

/// Runs `mass` over `data_len` bars.
///
/// `inputs` must point to `INPUTS` (2) pointers, in order:
/// `high, low`, each `data_len` `f64`s long.
/// `options` must point to `OPTIONS` (1) value: `period`.
///
/// Returns the mandatory `mass` output plus any requested optional outputs
/// (none for this indicator) and a fresh continuation state.
///
/// # Safety
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s (pass null + 0 to request no optional outputs).
#[no_mangle]
pub unsafe extern "C" fn mass_indicator(
    inputs: *const *const f64,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CIndicatorResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match Mass::indicator(&inputs, &options, optional) {
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

/// Continues a `mass` calculation from `state`, mutating it in place so
/// it's ready for the next call. `state` must have come from
/// `mass_indicator()` and not yet have been passed to `mass_state_free()`.
///
/// `inputs` must point to `INPUTS` (2) pointers (`high, low`),
/// each `data_len` `f64`s long.
///
/// # Safety
/// - `state` must be a live pointer previously returned by
///   `mass_indicator()`.
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn mass_batch(
    state: *mut c_void,
    inputs: *const *const f64,
    data_len: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CBatchResult {
    if state.is_null() {
        return CBatchResult::err(IndicatorError::InvalidIndicatorState);
    }
    let state = &mut *(state as *mut MassStateHandle);

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

/// Frees a state handle returned by `mass_indicator()` (or one of the
/// `states[i]` entries from `mass_simd_by_assets()`/`mass_simd_by_options()`).
/// Call this once you're done streaming (after your last `mass_batch()` call).
///
/// # Safety
/// `state` must be a live pointer previously returned by `mass_indicator()`
/// or found in a `CSimdResult::states` array, and must not be used again
/// after this call.
#[no_mangle]
pub unsafe extern "C" fn mass_state_free(state: *mut c_void) {
    if !state.is_null() {
        drop(Box::from_raw(state as *mut MassStateHandle));
    }
}

/// Computes MASS for `N` assets simultaneously (SIMD), sharing a single
/// options array. `num_assets` must be 2, 4, 8, or 16 -- anything else
/// returns `InvalidInputs`.
///
/// `inputs` must point to `num_assets` pointers, each itself pointing to
/// `INPUTS` (2) pointers (`high, low`), `data_len` `f64`s long.
/// `options` must point to `OPTIONS` (1) shared value.
///
/// Returns a `CSimdResult` with `num_assets` results, each with its own
/// output rows and its own ordinary `mass` continuation state (reusable
/// with `mass_batch()`/`mass_state_free()`). Free each state via
/// `mass_state_free()`, then free the rest via `tulip_ffi_simd_result_free()`.
///
/// # Safety
/// - `inputs` must point to `num_assets` valid pointers, each pointing to
///   `INPUTS` valid non-null `*const f64`s, each pointing to `data_len` valid
///   `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn mass_simd_by_assets(
    inputs: *const *const *const f64,
    num_assets: usize,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    match num_assets {
        2 => mass_simd_by_assets_n::<2>(inputs, data_len, options, optional_outputs, numoptional),
        4 => mass_simd_by_assets_n::<4>(inputs, data_len, options, optional_outputs, numoptional),
        8 => mass_simd_by_assets_n::<8>(inputs, data_len, options, optional_outputs, numoptional),
        16 => {
            mass_simd_by_assets_n::<16>(inputs, data_len, options, optional_outputs, numoptional)
        }
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn mass_simd_by_assets_n<const N: usize>(
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

    match Mass::indicator_by_assets::<N>(&refs, &options, optional) {
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

/// Computes MASS for one asset with `N` different option sets
/// simultaneously (SIMD). `num_option_sets` must be 2, 4, 8, or 16 --
/// anything else returns `InvalidInputs`.
///
/// `inputs` must point to `INPUTS` (2) pointers (`high, low`),
/// each `data_len` `f64`s long, shared across all option sets.
/// `options` must point to `num_option_sets` pointers, each itself
/// pointing to `OPTIONS` (1) value.
///
/// Returns a `CSimdResult` with `num_option_sets` results, each with its
/// own output rows and its own ordinary `mass` continuation state
/// (reusable with `mass_batch()`/`mass_state_free()`). Free each state
/// via `mass_state_free()`, then free the rest via
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
pub unsafe extern "C" fn mass_simd_by_options(
    inputs: *const *const f64,
    data_len: usize,
    options: *const *const f64,
    num_option_sets: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    match num_option_sets {
        2 => mass_simd_by_options_n::<2>(inputs, data_len, options, optional_outputs, numoptional),
        4 => mass_simd_by_options_n::<4>(inputs, data_len, options, optional_outputs, numoptional),
        8 => mass_simd_by_options_n::<8>(inputs, data_len, options, optional_outputs, numoptional),
        16 => {
            mass_simd_by_options_n::<16>(inputs, data_len, options, optional_outputs, numoptional)
        }
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn mass_simd_by_options_n<const N: usize>(
    inputs: *const *const f64,
    data_len: usize,
    options: *const *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let options = read_simd_options::<N, OPTIONS>(options);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match Mass::indicator_by_options::<N>(&inputs, &options, optional) {
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
    fn test_mass_info() {
        let info = mass_info();
        assert!(info.inputs.len > 0);
        assert_eq!(info.options.len, 1);
        assert!(info.outputs.len > 0);
    }

    #[test]
    fn test_mass_min_data() {
        unsafe {
            let options: [f64; OPTIONS] = [20.0];
            let min = mass_min_data(options.as_ptr());
            assert!(min > 0);
        }
    }

    #[test]
    fn test_mass_indicator() {
        unsafe {
            let data_len = 60;

            // mass has INPUTS=2: high, low
            let high_arr = build_synthetic_data(data_len, 0);
            let low_arr = build_synthetic_data(data_len, 0);

            let high_ptr = high_arr.as_ptr();
            let low_ptr = low_arr.as_ptr();

            // Array of input pointers
            let inputs_array = [high_ptr, low_ptr];
            let inputs = inputs_array.as_ptr();

            let options: [f64; OPTIONS] = [20.0];
            let options_ptr = options.as_ptr();

            // No optional outputs for mass
            let result = mass_indicator(inputs, data_len, options_ptr, std::ptr::null(), 0);

            assert_eq!(result.error, CIndicatorError::Ok);
            // mass has 1 output: mass
            assert_eq!(result.num_outputs, 1);

            tulip_ffi_result_free(result);
        }
    }

    #[test]
    fn test_mass_batch() {
        unsafe {
            let data_len = 60;

            let high_arr = build_synthetic_data(data_len, 0);
            let low_arr = build_synthetic_data(data_len, 0);

            let high_ptr = high_arr.as_ptr();
            let low_ptr = low_arr.as_ptr();

            let inputs_array = [high_ptr, low_ptr];
            let inputs = inputs_array.as_ptr();

            let options: [f64; OPTIONS] = [20.0];
            let options_ptr = options.as_ptr();

            // First call to get state
            let result = mass_indicator(inputs, data_len, options_ptr, std::ptr::null(), 0);
            assert_eq!(result.error, CIndicatorError::Ok);

            let state = result.state;

            // Second call with batch
            let high_arr2 = build_synthetic_data(data_len, 0);
            let low_arr2 = build_synthetic_data(data_len, 0);

            let high_ptr2 = high_arr2.as_ptr();
            let low_ptr2 = low_arr2.as_ptr();

            let inputs_array2 = [high_ptr2, low_ptr2];
            let inputs2 = inputs_array2.as_ptr();

            let batch_result = mass_batch(state, inputs2, data_len, std::ptr::null(), 0);
            assert_eq!(batch_result.error, CIndicatorError::Ok);

            tulip_ffi_batch_result_free(batch_result);
            mass_state_free(state);
        }
    }

    #[test]
    fn test_mass_simd_by_assets() {
        unsafe {
            let num_assets = 2;
            let data_len = 60;

            // Build inputs for 2 assets
            let high_arr1 = build_synthetic_data(data_len, 0);
            let low_arr1 = build_synthetic_data(data_len, 0);

            let high_arr2 = build_synthetic_data(data_len, 0);
            let low_arr2 = build_synthetic_data(data_len, 0);

            // Asset 1 inputs
            let high_ptr1 = high_arr1.as_ptr();
            let low_ptr1 = low_arr1.as_ptr();
            let asset1_inputs_array = [high_ptr1, low_ptr1];
            let asset1_inputs = asset1_inputs_array.as_ptr();

            // Asset 2 inputs
            let high_ptr2 = high_arr2.as_ptr();
            let low_ptr2 = low_arr2.as_ptr();
            let asset2_inputs_array = [high_ptr2, low_ptr2];
            let asset2_inputs = asset2_inputs_array.as_ptr();

            // Array of asset pointers
            let assets_array = [asset1_inputs, asset2_inputs];
            let inputs = assets_array.as_ptr();

            let options: [f64; OPTIONS] = [20.0];
            let options_ptr = options.as_ptr();

            let result = mass_simd_by_assets(
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
                mass_state_free(state_ptr);
            }

            tulip_ffi_simd_result_free(result);
        }
    }

    #[test]
    fn test_mass_simd_by_options() {
        unsafe {
            let num_option_sets = 2;
            let data_len = 60;

            // Build inputs
            let high_arr = build_synthetic_data(data_len, 0);
            let low_arr = build_synthetic_data(data_len, 0);

            let high_ptr = high_arr.as_ptr();
            let low_ptr = low_arr.as_ptr();

            let inputs_array = [high_ptr, low_ptr];
            let inputs = inputs_array.as_ptr();

            // Build 2 different option sets
            let options1: [f64; OPTIONS] = [20.0];
            let options2: [f64; OPTIONS] = [30.0];

            let options_ptr1 = options1.as_ptr();
            let options_ptr2 = options2.as_ptr();

            // Array of option set pointers
            let options_array = [options_ptr1, options_ptr2];
            let options = options_array.as_ptr();

            let result = mass_simd_by_options(
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
                mass_state_free(state_ptr);
            }

            tulip_ffi_simd_result_free(result);
        }
    }
}
