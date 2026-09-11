//! extern "C" wrapper for `typprice`, mirroring the core `tulip_rs` crate's
//! `Typprice::indicator` / `IndicatorState::batch_indicator` interface with a
//! Tulip-Indicators-style C calling convention: `inputs` is an array of
//! `INPUTS` pointers (one per input series, each `data_len` long, in the same
//! order as `tulip_rs::indicators::typprice::Typprice::INFO.inputs`), `options`
//! is a flat array of `OPTIONS` values -- exactly like `ti_typprice`'s
//! `double const *const *inputs, double const *options`. Output allocation
//! stays inside the indicator function (never caller-supplied).
//!
//! Parameter order convention (kept consistent across every function in
//! this crate): each pointer parameter is immediately followed by the
//! count(s) that describe it, e.g. `inputs, data_len, options, ...,
//! optional_outputs, numoptional`.
//!
//! SIMD entry point:
//! - `typprice_simd_by_assets`: compute TYPPRICE for N assets simultaneously,
//!   sharing a single options array (empty). N must be 2, 4, 8, or 16.
//!
//! NOTE: OPTIONS == 0, no simd_by_options

use std::os::raw::c_void;

use tulip_rs::indicator_types::{Indicator, TIndicatorState};
use tulip_rs::indicators::typprice::{IndicatorState as TyppriceState, Typprice, INPUTS, OPTIONS};
use tulip_rs::types::IndicatorError;

use crate::common::{
    optional_outputs_slice, pack_info, pack_outputs, pack_simd_outputs, pack_states, read_inputs,
    read_simd_assets_inputs, CBatchResult, CIndicatorError, CIndicatorInfo, CIndicatorResult,
    CSimdResult,
};

/// Opaque state handle returned by `typprice_indicator()` and consumed by
/// `typprice_batch()` / `typprice_state_free()`.
pub type TyppriceStateHandle = TyppriceState;

/// Returns static metadata about the `typprice` indicator: its name, input
/// names, option names, and (mandatory/optional) output names, mirroring
/// `Typprice::INFO`.
///
/// The returned strings are leaked, process-lifetime C strings -- read them,
/// don't free them.
#[no_mangle]
pub extern "C" fn typprice_info() -> CIndicatorInfo {
    pack_info(&Typprice::INFO)
}

/// Returns the minimum number of bars `typprice` needs to produce any output at
/// all, given `options`.
///
/// # Safety
/// `options` must point to `OPTIONS` (0) valid `f64`s.
#[no_mangle]
pub unsafe extern "C" fn typprice_min_data(options: *const f64) -> usize {
    let options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    Typprice::min_data(&options)
}

/// Runs `typprice` over `data_len` bars.
///
/// `inputs` must point to `INPUTS` (3) pointers, in order:
/// `high, low, close`, each `data_len` `f64`s long.
/// `options` must point to `OPTIONS` (0) values (pass null).
///
/// Returns the mandatory `typprice` output plus any requested optional outputs
/// (none available) and a fresh continuation state.
///
/// # Safety
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s (or null if OPTIONS is 0).
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s (pass null + 0 to request no optional outputs).
#[no_mangle]
pub unsafe extern "C" fn typprice_indicator(
    inputs: *const *const f64,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CIndicatorResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match Typprice::indicator(&inputs, &options, optional) {
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

/// Continues a `typprice` calculation from `state`, mutating it in place so
/// it's ready for the next call. `state` must have come from
/// `typprice_indicator()` and not yet have been passed to `typprice_state_free()`.
///
/// `inputs` must point to `INPUTS` (3) pointers (`high, low, close`),
/// each `data_len` `f64`s long.
///
/// # Safety
/// - `state` must be a live pointer previously returned by
///   `typprice_indicator()`.
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn typprice_batch(
    state: *mut c_void,
    inputs: *const *const f64,
    data_len: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CBatchResult {
    if state.is_null() {
        return CBatchResult::err(IndicatorError::InvalidIndicatorState);
    }
    let state = &mut *(state as *mut TyppriceStateHandle);

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

/// Frees a state handle returned by `typprice_indicator()` (or one of the
/// `states[i]` entries from `typprice_simd_by_assets()`).
/// Call this once you're done streaming (after your last `typprice_batch()` call).
///
/// # Safety
/// `state` must be a live pointer previously returned by `typprice_indicator()`
/// or found in a `CSimdResult::states` array, and must not be used again
/// after this call.
#[no_mangle]
pub unsafe extern "C" fn typprice_state_free(state: *mut c_void) {
    if !state.is_null() {
        drop(Box::from_raw(state as *mut TyppriceStateHandle));
    }
}

/// Computes TYPPRICE for `N` assets simultaneously (SIMD), sharing a single
/// empty options array. `num_assets` must be 2, 4, 8, or 16 -- anything else
/// returns `InvalidInputs`.
///
/// `inputs` must point to `num_assets` pointers, each itself pointing to
/// `INPUTS` (3) pointers (`high, low, close`), each `data_len` `f64`s
/// long. `options` must point to `OPTIONS` (0) shared values.
///
/// Returns a `CSimdResult` with `num_assets` results, each with its own
/// output rows and its own ordinary `typprice` continuation state (reusable
/// with `typprice_batch()`/`typprice_state_free()`). Free each state via
/// `typprice_state_free()`, then free the rest via `tulip_ffi_simd_result_free()`.
///
/// # Safety
/// - `inputs` must point to `num_assets` valid pointers, each pointing to
///   `INPUTS` valid non-null `*const f64`s, each pointing to `data_len` valid
///   `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s (or null if OPTIONS is 0).
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn typprice_simd_by_assets(
    inputs: *const *const *const f64,
    num_assets: usize,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    match num_assets {
        2 => {
            typprice_simd_by_assets_n::<2>(inputs, data_len, options, optional_outputs, numoptional)
        }
        4 => {
            typprice_simd_by_assets_n::<4>(inputs, data_len, options, optional_outputs, numoptional)
        }
        8 => {
            typprice_simd_by_assets_n::<8>(inputs, data_len, options, optional_outputs, numoptional)
        }
        16 => typprice_simd_by_assets_n::<16>(
            inputs,
            data_len,
            options,
            optional_outputs,
            numoptional,
        ),
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn typprice_simd_by_assets_n<const N: usize>(
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
    let options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match Typprice::indicator_by_assets::<N>(&refs, &options, optional) {
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

// NOTE: `typprice` has `OPTIONS == 0` and the core `Typprice` does not
// implement `IndicatorByOptions`, so there is no `typprice_simd_by_options`
// entry point.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::test::build_synthetic_data;
    use crate::common::{
        tulip_ffi_batch_result_free, tulip_ffi_result_free, tulip_ffi_simd_result_free,
    };

    unsafe fn free_typprice_result(result: CIndicatorResult) {
        tulip_ffi_result_free(result);
    }

    unsafe fn free_typprice_batch_result(result: CBatchResult) {
        tulip_ffi_batch_result_free(result);
    }

    unsafe fn free_typprice_simd_result(result: CSimdResult) {
        tulip_ffi_simd_result_free(result);
    }

    #[test]
    fn test_typprice_info() {
        let info = typprice_info();
        assert!(info.inputs.len > 0);
        assert_eq!(info.options.len, OPTIONS);
        assert!(info.outputs.len > 0);
        assert_eq!(info.optional_outputs.len, 0);
    }

    #[test]
    fn test_typprice_min_data() {
        unsafe {
            let options: [f64; OPTIONS] = [];
            let min = typprice_min_data(options.as_ptr());
            assert!(min > 0);
        }
    }

    #[test]
    fn test_typprice_indicator() {
        unsafe {
            let data_len = 20;
            // Create test data: high, low, close prices
            let high: Vec<f64> = build_synthetic_data(data_len, 2);
            let low: Vec<f64> = build_synthetic_data(data_len, 0);
            let close: Vec<f64> = build_synthetic_data(data_len, 1);

            let inputs_array: [*const f64; INPUTS] = [high.as_ptr(), low.as_ptr(), close.as_ptr()];
            let options: [f64; OPTIONS] = [];

            let result = typprice_indicator(
                inputs_array.as_ptr(),
                data_len,
                options.as_ptr(),
                std::ptr::null(),
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_outputs, 1);

            free_typprice_result(result);
        }
    }

    #[test]
    fn test_typprice_batch() {
        unsafe {
            let data_len = 20;
            // Create test data: high, low, close prices
            let high: Vec<f64> = build_synthetic_data(data_len, 2);
            let low: Vec<f64> = build_synthetic_data(data_len, 0);
            let close: Vec<f64> = build_synthetic_data(data_len, 1);

            let inputs_array: [*const f64; INPUTS] = [high.as_ptr(), low.as_ptr(), close.as_ptr()];
            let options: [f64; OPTIONS] = [];

            let result = typprice_indicator(
                inputs_array.as_ptr(),
                data_len,
                options.as_ptr(),
                std::ptr::null(),
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            let state = result.state;

            tulip_ffi_result_free(result);

            // Second batch with more data
            let new_data_len = 10;
            let high2: Vec<f64> = build_synthetic_data(10, 3);
            let low2: Vec<f64> = build_synthetic_data(10, 1);
            let close2: Vec<f64> = build_synthetic_data(10, 2);

            let inputs_array2: [*const f64; INPUTS] =
                [high2.as_ptr(), low2.as_ptr(), close2.as_ptr()];

            let batch_result = typprice_batch(
                state,
                inputs_array2.as_ptr(),
                new_data_len,
                std::ptr::null(),
                0,
            );

            assert_eq!(batch_result.error, CIndicatorError::Ok);
            assert_eq!(batch_result.num_outputs, 1);

            free_typprice_batch_result(batch_result);
            typprice_state_free(state);
        }
    }

    #[test]
    fn test_typprice_simd_by_assets() {
        unsafe {
            const NUM_ASSETS: usize = 2;
            let data_len = 20;

            // Create test data for asset 1
            let high1: Vec<f64> = build_synthetic_data(data_len, 2);
            let low1: Vec<f64> = build_synthetic_data(data_len, 0);
            let close1: Vec<f64> = build_synthetic_data(data_len, 1);

            // Create test data for asset 2
            let high2: Vec<f64> = build_synthetic_data(20, 3);
            let low2: Vec<f64> = build_synthetic_data(20, 1);
            let close2: Vec<f64> = build_synthetic_data(20, 2);

            let asset1_inputs: [*const f64; INPUTS] =
                [high1.as_ptr(), low1.as_ptr(), close1.as_ptr()];
            let asset2_inputs: [*const f64; INPUTS] =
                [high2.as_ptr(), low2.as_ptr(), close2.as_ptr()];

            let assets_array: [*const *const f64; NUM_ASSETS] =
                [asset1_inputs.as_ptr(), asset2_inputs.as_ptr()];

            let options: [f64; OPTIONS] = [];

            let result = typprice_simd_by_assets(
                assets_array.as_ptr(),
                NUM_ASSETS,
                data_len,
                options.as_ptr(),
                std::ptr::null(),
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, NUM_ASSETS);
            assert_eq!(result.num_outputs, 1);

            for i in 0..NUM_ASSETS {
                let state_ptr = *(result.states.add(i));
                typprice_state_free(state_ptr);
            }

            free_typprice_simd_result(result);
        }
    }
}
