//! extern "C" wrapper for `marketfi`, mirroring the core `tulip_rs` crate's
//! `Marketfi::indicator` / `IndicatorState::batch_indicator` interface with a
//! Tulip-Indicators-style C calling convention: `inputs` is an array of
//! `INPUTS` pointers (one per input series, each `data_len` long), `options`
//! is a flat array of `OPTIONS` values.
//!
//! Parameter order convention: each pointer parameter is immediately followed by the
//! count(s) that describe it.
//!
//! SIMD entry points:
//! - `marketfi_simd_by_assets`: compute MARKETFI for N assets simultaneously,
//!   sharing a single options array. N must be 2, 4, 8, or 16.

use std::os::raw::c_void;

use tulip_rs::indicator_types::{Indicator, TIndicatorState};
use tulip_rs::indicators::marketfi::{IndicatorState as MarketFiState, Marketfi, INPUTS, OPTIONS};
use tulip_rs::types::IndicatorError;

use crate::common::{
    optional_outputs_slice, pack_info, pack_outputs, pack_simd_outputs, pack_states, read_inputs,
    read_simd_assets_inputs, CBatchResult, CIndicatorError, CIndicatorInfo, CIndicatorResult,
    CSimdResult,
};

/// Opaque state handle returned by `marketfi_indicator()` and consumed by
/// `marketfi_batch()` / `marketfi_state_free()`.
pub type MarketFiStateHandle = MarketFiState;

/// Returns static metadata about the `marketfi` indicator: its name, input
/// names, option names, and (mandatory/optional) output names, mirroring
/// `Marketfi::INFO`.
///
/// The returned strings are leaked, process-lifetime C strings -- read them,
/// don't free them.
#[no_mangle]
pub extern "C" fn marketfi_info() -> CIndicatorInfo {
    pack_info(&Marketfi::INFO)
}

/// Returns the minimum number of bars `marketfi` needs to produce any output at
/// all. This indicator has no options, so it always returns 1.
#[no_mangle]
pub extern "C" fn marketfi_min_data(_options: *const f64) -> usize {
    Marketfi::min_data(&[])
}

/// Runs `marketfi` over `data_len` bars.
///
/// `inputs` must point to `INPUTS` (3) pointers, in order:
/// `high, low, volume`, each `data_len` `f64`s long.
/// `options` must point to `OPTIONS` (0) values (no options).
///
/// Returns the mandatory `marketfi` output plus any requested optional outputs
/// (none for this indicator) and a fresh continuation state.
///
/// # Safety
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s (pass null or any pointer for OPTIONS=0).
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s (pass null + 0 to request no optional outputs).
#[no_mangle]
pub unsafe extern "C" fn marketfi_indicator(
    inputs: *const *const f64,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CIndicatorResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let _options: [f64; OPTIONS] = unsafe { *(options as *const [f64; OPTIONS]) };
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match Marketfi::indicator(&inputs, &_options, optional) {
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

/// Continues a `marketfi` calculation from `state`, mutating it in place so
/// it's ready for the next call. `state` must have come from
/// `marketfi_indicator()` and not yet have been passed to `marketfi_state_free()`.
///
/// `inputs` must point to `INPUTS` (3) pointers (`high, low, volume`),
/// each `data_len` `f64`s long.
///
/// # Safety
/// - `state` must be a live pointer previously returned by
///   `marketfi_indicator()`.
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn marketfi_batch(
    state: *mut c_void,
    inputs: *const *const f64,
    data_len: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CBatchResult {
    if state.is_null() {
        return CBatchResult::err(IndicatorError::InvalidIndicatorState);
    }
    let state = &mut *(state as *mut MarketFiStateHandle);

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

/// Frees a state handle returned by `marketfi_indicator()` (or one of the
/// `states[i]` entries from `marketfi_simd_by_assets()`).
/// Call this once you're done streaming (after your last `marketfi_batch()` call).
///
/// # Safety
/// `state` must be a live pointer previously returned by `marketfi_indicator()`
/// or found in a `CSimdResult::states` array, and must not be used again
/// after this call.
#[no_mangle]
pub unsafe extern "C" fn marketfi_state_free(state: *mut c_void) {
    if !state.is_null() {
        drop(Box::from_raw(state as *mut MarketFiStateHandle));
    }
}

/// Computes MARKETFI for `N` assets simultaneously (SIMD), sharing a single
/// options array. `num_assets` must be 2, 4, 8, or 16 -- anything else
/// returns `InvalidInputs`.
///
/// `inputs` must point to `num_assets` pointers, each itself pointing to
/// `INPUTS` (3) pointers (`high, low, volume`), `data_len` `f64`s long.
/// `options` must point to `OPTIONS` (0) shared values.
///
/// Returns a `CSimdResult` with `num_assets` results, each with its own
/// output rows and its own ordinary `marketfi` continuation state (reusable
/// with `marketfi_batch()`/`marketfi_state_free()`). Free each state via
/// `marketfi_state_free()`, then free the rest via `tulip_ffi_simd_result_free()`.
///
/// # Safety
/// - `inputs` must point to `num_assets` valid pointers, each pointing to
///   `INPUTS` valid non-null `*const f64`s, each pointing to `data_len` valid
///   `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s (pass null or any pointer for OPTIONS=0).
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn marketfi_simd_by_assets(
    inputs: *const *const *const f64,
    num_assets: usize,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    match num_assets {
        2 => marketfi_simd_by_assets_n::<2>(
            inputs,
            data_len,
            options,
            optional_outputs,
            numoptional,
        ),
        4 => marketfi_simd_by_assets_n::<4>(
            inputs,
            data_len,
            options,
            optional_outputs,
            numoptional,
        ),
        8 => marketfi_simd_by_assets_n::<8>(
            inputs,
            data_len,
            options,
            optional_outputs,
            numoptional,
        ),
        16 => marketfi_simd_by_assets_n::<16>(
            inputs,
            data_len,
            options,
            optional_outputs,
            numoptional,
        ),
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn marketfi_simd_by_assets_n<const N: usize>(
    inputs: *const *const *const f64,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    let owned = read_simd_assets_inputs::<N, INPUTS>(inputs, data_len);
    let refs: [&[&[f64]; INPUTS]; N] = std::array::from_fn(|i| &owned[i]);
    let _options: [f64; OPTIONS] = unsafe { *(options as *const [f64; OPTIONS]) };
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match Marketfi::indicator_by_assets::<N>(&refs, &_options, optional) {
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
    fn test_marketfi_info() {
        let info = marketfi_info();
        assert!(info.inputs.len > 0);
        assert_eq!(info.options.len, 0);
        assert!(info.outputs.len > 0);
    }

    #[test]
    fn test_marketfi_min_data() {
        let min = marketfi_min_data(std::ptr::null());
        assert_eq!(min, 1);
    }

    #[test]
    fn test_marketfi_indicator() {
        unsafe {
            let data_len = 60;

            // marketfi has INPUTS=3: high, low, volume
            let high_arr = build_synthetic_data(data_len, 0);
            let low_arr = build_synthetic_data(data_len, 0);
            let volume_arr = build_synthetic_data(data_len, 0);

            let high_ptr = high_arr.as_ptr();
            let low_ptr = low_arr.as_ptr();
            let volume_ptr = volume_arr.as_ptr();

            // Array of input pointers
            let inputs_array = [high_ptr, low_ptr, volume_ptr];
            let inputs = inputs_array.as_ptr();

            // OPTIONS=0, so we pass a null pointer
            let result =
                marketfi_indicator(inputs, data_len, std::ptr::null(), std::ptr::null(), 0);

            assert_eq!(result.error, CIndicatorError::Ok);
            // marketfi has 1 output: marketfi
            assert_eq!(result.num_outputs, 1);

            tulip_ffi_result_free(result);
        }
    }

    #[test]
    fn test_marketfi_batch() {
        unsafe {
            let data_len = 60;

            let high_arr = build_synthetic_data(data_len, 0);
            let low_arr = build_synthetic_data(data_len, 0);
            let volume_arr = build_synthetic_data(data_len, 0);

            let high_ptr = high_arr.as_ptr();
            let low_ptr = low_arr.as_ptr();
            let volume_ptr = volume_arr.as_ptr();

            let inputs_array = [high_ptr, low_ptr, volume_ptr];
            let inputs = inputs_array.as_ptr();

            // First call to get state
            let result =
                marketfi_indicator(inputs, data_len, std::ptr::null(), std::ptr::null(), 0);
            assert_eq!(result.error, CIndicatorError::Ok);

            let state = result.state;

            // Second call with batch
            let high_arr2 = build_synthetic_data(data_len, 0);
            let low_arr2 = build_synthetic_data(data_len, 0);
            let volume_arr2 = build_synthetic_data(data_len, 0);

            let high_ptr2 = high_arr2.as_ptr();
            let low_ptr2 = low_arr2.as_ptr();
            let volume_ptr2 = volume_arr2.as_ptr();

            let inputs_array2 = [high_ptr2, low_ptr2, volume_ptr2];
            let inputs2 = inputs_array2.as_ptr();

            let batch_result = marketfi_batch(state, inputs2, data_len, std::ptr::null(), 0);
            assert_eq!(batch_result.error, CIndicatorError::Ok);

            tulip_ffi_batch_result_free(batch_result);
            marketfi_state_free(state);
        }
    }

    #[test]
    fn test_marketfi_simd_by_assets() {
        unsafe {
            let num_assets = 2;
            let data_len = 60;

            // Build inputs for 2 assets
            let high_arr1 = build_synthetic_data(data_len, 0);
            let low_arr1 = build_synthetic_data(data_len, 0);
            let volume_arr1 = build_synthetic_data(data_len, 0);

            let high_arr2 = build_synthetic_data(data_len, 0);
            let low_arr2 = build_synthetic_data(data_len, 0);
            let volume_arr2 = build_synthetic_data(data_len, 0);

            // Asset 1 inputs
            let high_ptr1 = high_arr1.as_ptr();
            let low_ptr1 = low_arr1.as_ptr();
            let volume_ptr1 = volume_arr1.as_ptr();
            let asset1_inputs_array = [high_ptr1, low_ptr1, volume_ptr1];
            let asset1_inputs = asset1_inputs_array.as_ptr();

            // Asset 2 inputs
            let high_ptr2 = high_arr2.as_ptr();
            let low_ptr2 = low_arr2.as_ptr();
            let volume_ptr2 = volume_arr2.as_ptr();
            let asset2_inputs_array = [high_ptr2, low_ptr2, volume_ptr2];
            let asset2_inputs = asset2_inputs_array.as_ptr();

            // Array of asset pointers
            let assets_array = [asset1_inputs, asset2_inputs];
            let inputs = assets_array.as_ptr();

            // OPTIONS=0
            let result = marketfi_simd_by_assets(
                inputs,
                num_assets,
                data_len,
                std::ptr::null(),
                std::ptr::null(),
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, num_assets);

            // Free each state
            for i in 0..num_assets {
                let state_ptr = *(result.states.add(i));
                marketfi_state_free(state_ptr);
            }

            tulip_ffi_simd_result_free(result);
        }
    }
}
