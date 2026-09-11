//! extern "C" wrapper for `medprice`, mirroring the core `tulip_rs` crate's
//! `MedPrice::indicator` / `IndicatorState::batch_indicator` interface with a
//! Tulip-Indicators-style C calling convention: `inputs` is an array of
//! `INPUTS` pointers (one per input series, each `data_len` long, in the same
//! order as `tulip_rs::indicators::medprice::MedPrice::INFO.inputs`), `options`
//! is a flat array of `OPTIONS` values -- exactly like `ti_medprice`'s
//! `double const *const *inputs, double const *options`. Output allocation
//! stays inside the indicator function (never caller-supplied).
//!
//! Parameter order convention (kept consistent across every function in
//! this crate): each pointer parameter is immediately followed by the
//! count(s) that describe it, e.g. `inputs, data_len, options, ...,
//! optional_outputs, numoptional`.
//!
//! SIMD entry points:
//! - `medprice_simd_by_assets`: compute MEDPRICE for N assets simultaneously,
//!   sharing a single options array. N must be 2, 4, 8, or 16.
//! - `medprice_simd_by_options`: compute MEDPRICE for one asset with N different
//!   option sets simultaneously. N must be 2, 4, 8, or 16.
//!
//! `#[no_mangle]` functions can't be generic, so each SIMD entry point is a
//! thin non-generic dispatcher that matches the runtime `N` against
//! {2,4,8,16} and calls a private generic `_n::<N>` helper that does the
//! real work.

use std::os::raw::c_void;

use tulip_rs::indicator_types::{Indicator, TIndicatorState};
use tulip_rs::indicators::medprice::{
    IndicatorState as MedPriceState, Medprice as MedPrice, INPUTS, OPTIONS,
};
use tulip_rs::types::IndicatorError;

use crate::common::{
    optional_outputs_slice, pack_info, pack_outputs, pack_simd_outputs, pack_states, read_inputs,
    read_simd_assets_inputs, CBatchResult, CIndicatorError, CIndicatorInfo, CIndicatorResult,
    CSimdResult,
};

/// Opaque state handle returned by `medprice_indicator()` and consumed by
/// `medprice_batch()` / `medprice_state_free()`.
pub type MedPriceStateHandle = MedPriceState;

/// Returns static metadata about the `medprice` indicator: its name, input
/// names, option names, and (mandatory/optional) output names, mirroring
/// `MedPrice::INFO`.
///
/// The returned strings are leaked, process-lifetime C strings -- read them,
/// don't free them.
#[no_mangle]
pub extern "C" fn medprice_info() -> CIndicatorInfo {
    pack_info(&MedPrice::INFO)
}

/// Returns the minimum number of bars `medprice` needs to produce any output at
/// all, given `options`.
///
/// # Safety
/// `options` must point to `OPTIONS` (0) valid `f64`s.
#[no_mangle]
pub unsafe extern "C" fn medprice_min_data(options: *const f64) -> usize {
    let options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    MedPrice::min_data(&options)
}

/// Runs `medprice` over `data_len` bars.
///
/// `inputs` must point to `INPUTS` (2) pointers, in order:
/// `high, low`, each `data_len` `f64`s long.
/// `options` must point to `OPTIONS` (0) values (pass null).
///
/// Returns the mandatory `medprice` output plus any requested optional outputs
/// (none available) and a fresh continuation state.
///
/// # Safety
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s (or null if OPTIONS is 0).
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s (pass null + 0 to request no optional outputs).
#[no_mangle]
pub unsafe extern "C" fn medprice_indicator(
    inputs: *const *const f64,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CIndicatorResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match MedPrice::indicator(&inputs, &options, optional) {
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

/// Continues a `medprice` calculation from `state`, mutating it in place so
/// it's ready for the next call. `state` must have come from
/// `medprice_indicator()` and not yet have been passed to `medprice_state_free()`.
///
/// `inputs` must point to `INPUTS` (2) pointers (`high, low`),
/// each `data_len` `f64`s long.
///
/// # Safety
/// - `state` must be a live pointer previously returned by
///   `medprice_indicator()`.
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn medprice_batch(
    state: *mut c_void,
    inputs: *const *const f64,
    data_len: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CBatchResult {
    if state.is_null() {
        return CBatchResult::err(IndicatorError::InvalidIndicatorState);
    }
    let state = &mut *(state as *mut MedPriceStateHandle);

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

/// Frees a state handle returned by `medprice_indicator()` (or one of the
/// `states[i]` entries from `medprice_simd_by_assets()`/`medprice_simd_by_options()`).
/// Call this once you're done streaming (after your last `medprice_batch()` call).
///
/// # Safety
/// `state` must be a live pointer previously returned by `medprice_indicator()`
/// or found in a `CSimdResult::states` array, and must not be used again
/// after this call.
#[no_mangle]
pub unsafe extern "C" fn medprice_state_free(state: *mut c_void) {
    if !state.is_null() {
        drop(Box::from_raw(state as *mut MedPriceStateHandle));
    }
}

/// Computes MEDPRICE for `N` assets simultaneously (SIMD), sharing a single
/// options array. `num_assets` must be 2, 4, 8, or 16 -- anything else
/// returns `InvalidInputs`.
///
/// `inputs` must point to `num_assets` pointers, each itself pointing to
/// `INPUTS` (2) pointers (`high, low`), each `data_len` `f64`s
/// long. `options` must point to `OPTIONS` (0) shared values.
///
/// Returns a `CSimdResult` with `num_assets` results, each with its own
/// output rows and its own ordinary `medprice` continuation state (reusable
/// with `medprice_batch()`/`medprice_state_free()`). Free each state via
/// `medprice_state_free()`, then free the rest via `tulip_ffi_simd_result_free()`.
///
/// # Safety
/// - `inputs` must point to `num_assets` valid pointers, each pointing to
///   `INPUTS` valid non-null `*const f64`s, each pointing to `data_len` valid
///   `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s (or null if OPTIONS is 0).
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn medprice_simd_by_assets(
    inputs: *const *const *const f64,
    num_assets: usize,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    match num_assets {
        2 => {
            medprice_simd_by_assets_n::<2>(inputs, data_len, options, optional_outputs, numoptional)
        }
        4 => {
            medprice_simd_by_assets_n::<4>(inputs, data_len, options, optional_outputs, numoptional)
        }
        8 => {
            medprice_simd_by_assets_n::<8>(inputs, data_len, options, optional_outputs, numoptional)
        }
        16 => medprice_simd_by_assets_n::<16>(
            inputs,
            data_len,
            options,
            optional_outputs,
            numoptional,
        ),
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn medprice_simd_by_assets_n<const N: usize>(
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

    match MedPrice::indicator_by_assets::<N>(&refs, &options, optional) {
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

// NOTE: `medprice` has `OPTIONS == 0` and the core `Medprice` does not
// implement `IndicatorByOptions`, so there is no `medprice_simd_by_options`
// entry point (same treatment as `homodynediscriminator`).

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::test::build_synthetic_data;
    use crate::{tulip_ffi_batch_result_free, tulip_ffi_result_free, tulip_ffi_simd_result_free};

    unsafe fn free_medprice_result(result: CIndicatorResult) {
        tulip_ffi_result_free(result);
    }

    unsafe fn free_medprice_batch_result(result: CBatchResult) {
        tulip_ffi_batch_result_free(result);
    }

    unsafe fn free_medprice_simd_result(result: CSimdResult) {
        tulip_ffi_simd_result_free(result);
    }

    #[test]
    fn test_medprice_indicator() {
        unsafe {
            let data_len = 60;
            let inputs: Vec<f64> = build_synthetic_data(data_len, 0);
            let high_ptr = inputs.as_ptr();
            let low_ptr = inputs.as_ptr();

            let inputs_array: [*const f64; INPUTS] = [high_ptr, low_ptr];
            let options: [f64; OPTIONS] = [];

            let result = medprice_indicator(
                inputs_array.as_ptr(),
                data_len,
                options.as_ptr(),
                std::ptr::null(),
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_outputs, 1);

            free_medprice_result(result);
        }
    }

    #[test]
    fn test_medprice_batch() {
        unsafe {
            let data_len = 60;
            let inputs: Vec<f64> = build_synthetic_data(data_len, 0);
            let high_ptr = inputs.as_ptr();
            let low_ptr = inputs.as_ptr();

            let inputs_array: [*const f64; INPUTS] = [high_ptr, low_ptr];
            let options: [f64; OPTIONS] = [];

            let result = medprice_indicator(
                inputs_array.as_ptr(),
                data_len,
                options.as_ptr(),
                std::ptr::null(),
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);

            let state = result.state;
            tulip_ffi_result_free(result);

            let new_data_len = 30;
            let new_inputs: Vec<f64> = build_synthetic_data(new_data_len, 0);
            let high_ptr2 = new_inputs.as_ptr();
            let low_ptr2 = new_inputs.as_ptr();

            let new_inputs_array: [*const f64; INPUTS] = [high_ptr2, low_ptr2];

            let batch_result = medprice_batch(
                state,
                new_inputs_array.as_ptr(),
                new_data_len,
                std::ptr::null(),
                0,
            );

            assert_eq!(batch_result.error, CIndicatorError::Ok);

            free_medprice_batch_result(batch_result);
            medprice_state_free(state);
        }
    }

    #[test]
    fn test_medprice_simd_by_assets() {
        unsafe {
            const NUM_ASSETS: usize = 2;
            let data_len = 60;

            let inputs1: Vec<f64> = build_synthetic_data(data_len, 0);
            let inputs2: Vec<f64> = build_synthetic_data(data_len, 0);

            let high1_ptr = inputs1.as_ptr();
            let low1_ptr = inputs1.as_ptr();
            let high2_ptr = inputs2.as_ptr();
            let low2_ptr = inputs2.as_ptr();

            let asset1_inputs: [*const f64; INPUTS] = [high1_ptr, low1_ptr];
            let asset2_inputs: [*const f64; INPUTS] = [high2_ptr, low2_ptr];

            let assets_array: [*const *const f64; NUM_ASSETS] =
                [asset1_inputs.as_ptr(), asset2_inputs.as_ptr()];

            let options: [f64; OPTIONS] = [];

            let result = medprice_simd_by_assets(
                assets_array.as_ptr(),
                NUM_ASSETS,
                data_len,
                options.as_ptr(),
                std::ptr::null(),
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, NUM_ASSETS);

            for i in 0..NUM_ASSETS {
                let state_ptr = *(result.states.add(i));
                medprice_state_free(state_ptr);
            }

            free_medprice_simd_result(result);
        }
    }

    #[test]
    fn test_medprice_info() {
        let info = medprice_info();
        assert!(info.inputs.len > 0);
        assert_eq!(info.options.len, OPTIONS);
        assert!(info.outputs.len > 0);
        assert_eq!(info.optional_outputs.len, 0);
    }

    #[test]
    fn test_medprice_min_data() {
        unsafe {
            let options: [f64; OPTIONS] = [];
            let min = medprice_min_data(options.as_ptr());
            assert!(min > 0);
        }
    }
}
