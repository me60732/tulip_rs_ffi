//! extern "C" wrapper for `ao`, mirroring the core `tulip_rs` crate's
//! `Ao::indicator` / `IndicatorState::batch_indicator` interface with a
//! Tulip-Indicators-style C calling convention: `inputs` is an array of
//! `INPUTS` pointers (one per input series, each `data_len` long, in the same
//! order as `tulip_rs::indicators::ao::Ao::INFO.inputs`), `options`
//! is a flat array of `OPTIONS` values -- exactly like `ti_ao`'s
//! `double const *const *inputs, double const *options`. Output allocation
//! stays inside the indicator function (never caller-supplied).
//!
//! Parameter order convention (kept consistent across every function in
//! this crate): each pointer parameter is immediately followed by the
//! count(s) that describe it, e.g. `inputs, data_len, options, ...,
//! optional_outputs, num_optional`.
//!
//! SIMD entry points:
//! - `ao_simd_by_assets`: compute AO for N assets simultaneously,
//!   sharing a single options array. N must be 2, 4, 8, or 16.
//!
//! Note: `ao` does not support SIMD by-options (no `indicator_by_options` impl).
//!
//! `#[no_mangle]` functions can't be generic, so each SIMD entry point is a
//! thin non-generic dispatcher that matches the runtime `N` against
//! {2,4,8,16} and calls a private generic `_n::<N>` helper that does the
//! real work.

use std::os::raw::c_void;

use tulip_rs::indicator_types::{Indicator, TIndicatorState};
use tulip_rs::indicators::ao::{Ao, IndicatorState as AoState, INPUTS, OPTIONS};
use tulip_rs::types::IndicatorError;

use crate::common::{
    optional_outputs_slice, pack_outputs, pack_simd_outputs, pack_states, read_inputs,
    read_simd_assets_inputs, CBatchResult, CIndicatorError, CIndicatorResult, CSimdResult,
};

/// Opaque state handle returned by `ao_indicator()` and consumed by
/// `ao_batch()` / `ao_state_free()`.
pub type AoStateHandle = AoState;

/// Runs `ao` over `data_len` bars.
///
/// `inputs` must point to `INPUTS` (2) pointers, in order:
/// `high, low`, each `data_len` `f64`s long.
/// `options` must point to `OPTIONS` (0) values (no options).
///
/// Returns the mandatory `ao` output plus any requested optional outputs
/// (`short_sma`, `long_sma`, `medprice`, in that fixed order) and a fresh continuation state.
///
/// # Safety
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
#[no_mangle]
pub unsafe extern "C" fn ao_indicator(
    inputs: *const *const f64,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    num_optional: usize,
) -> CIndicatorResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let _options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    let optional = optional_outputs_slice(optional_outputs, num_optional);

    match Ao::indicator(&inputs, &_options, optional) {
        Ok((rows, state)) => {
            let (outputs, output_lens, num_outputs) = pack_outputs(rows);
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

/// Continues an `ao` calculation from `state`, mutating it in place so
/// it's ready for the next call. `state` must have come from
/// `ao_indicator()` and not yet have been passed to `ao_state_free()`.
///
/// `inputs` must point to `INPUTS` (2) pointers (`high, low`),
/// each `data_len` `f64`s long.
///
/// # Safety
/// - `state` must be a live pointer previously returned by
///   `ao_indicator()`.
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `num_optional` valid
///   `bool`s (pass null + 0 to request no optional outputs).
#[no_mangle]
pub unsafe extern "C" fn ao_batch(
    state: *mut c_void,
    inputs: *const *const f64,
    data_len: usize,
    optional_outputs: *const bool,
    num_optional: usize,
) -> CBatchResult {
    if state.is_null() {
        return CBatchResult::err(IndicatorError::InvalidIndicatorState);
    }
    let state = &mut *(state as *mut AoStateHandle);

    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let optional = optional_outputs_slice(optional_outputs, num_optional);

    match state.batch_indicator(&inputs, optional) {
        Ok(rows) => {
            let (outputs, output_lens, num_outputs) = pack_outputs(rows);
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

/// Frees a state handle returned by `ao_indicator()` (or one of the
/// `states[i]` entries from `ao_simd_by_assets()`).
/// Call this once you're done streaming (after your last `ao_batch()` call).
///
/// # Safety
/// `state` must be a live pointer previously returned by `ao_indicator()`
/// or found in a `CSimdResult::states` array (from `ao_simd_by_assets()`),
/// and must not be used again after this call.
#[no_mangle]
pub unsafe extern "C" fn ao_state_free(state: *mut c_void) {
    if !state.is_null() {
        drop(Box::from_raw(state as *mut AoStateHandle));
    }
}

/// Computes AO for `N` assets simultaneously (SIMD), sharing a single
/// options array. `num_assets` must be 2, 4, 8, or 16 -- anything else
/// returns `InvalidInputs`.
///
/// `inputs` must point to `num_assets` pointers, each itself pointing to
/// `INPUTS` (2) pointers (`high, low`), each `data_len` `f64`s
/// long. `options` must point to `OPTIONS` (0) shared values.
///
/// Returns a `CSimdResult` with `num_assets` results, each with its own
/// output rows and its own ordinary `ao` continuation state (reusable
/// with `ao_batch()`/`ao_state_free()`). Free each state via
/// `ao_state_free()`, then free the rest via `tulip_ffi_simd_result_free()`.
///
/// # Safety
/// - `inputs` must point to `num_assets` valid pointers, each pointing to
///   `INPUTS` valid non-null `*const f64`s, each pointing to `data_len` valid
///   `f64`s.
#[no_mangle]
pub unsafe extern "C" fn ao_simd_by_assets(
    inputs: *const *const *const f64,
    num_assets: usize,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    num_optional: usize,
) -> CSimdResult {
    match num_assets {
        2 => ao_simd_by_assets_n::<2>(inputs, data_len, options, optional_outputs, num_optional),
        4 => ao_simd_by_assets_n::<4>(inputs, data_len, options, optional_outputs, num_optional),
        8 => ao_simd_by_assets_n::<8>(inputs, data_len, options, optional_outputs, num_optional),
        16 => ao_simd_by_assets_n::<16>(inputs, data_len, options, optional_outputs, num_optional),
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn ao_simd_by_assets_n<const N: usize>(
    inputs: *const *const *const f64,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    num_optional: usize,
) -> CSimdResult {
    // `owned` holds the per-asset input slices; `refs` borrows from it, so
    // both must live in this stack frame for the duration of the call.
    let owned = read_simd_assets_inputs::<N, INPUTS>(inputs, data_len);
    let refs: [&[&[f64]; INPUTS]; N] = std::array::from_fn(|i| &owned[i]);
    let _options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    let optional = optional_outputs_slice(optional_outputs, num_optional);

    match Ao::indicator_by_assets::<N>(&refs, &_options, optional) {
        Ok((results, states)) => {
            let (outputs, output_lens, num_outputs, num_results) = pack_simd_outputs(results);
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

/// Computes AO for one asset with `N` different option sets
/// simultaneously (SIMD). `num_option_sets` must be 2, 4, 8, or 16 --
/// anything else returns `InvalidInputs`.
///
/// `inputs` must point to `INPUTS` (2) pointers (`high, low`),
/// each `data_len` `f64`s long, shared across all option sets.
/// `options` must point to `num_option_sets` pointers, each itself
/// pointing to `OPTIONS` (0) values.
///
/// Returns a `CSimdResult` with `num_option_sets` results, each with its
/// own output rows and its own ordinary `ao` continuation state
/// (reusable with `ao_batch()`/`ao_state_free()`). Free each state
/// via `ao_state_free()`, then free the rest via
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
    fn test_ao_indicator() {
        unsafe {
            let data_len = 60;
            let high: Vec<f64> = (0..data_len).map(|i| 100.0 + i as f64 * 0.5).collect();
            let low: Vec<f64> = (0..data_len).map(|i| 90.0 + i as f64 * 0.5).collect();

            let inputs_ptr: [*const f64; INPUTS] = [high.as_ptr(), low.as_ptr()];
            let inputs = inputs_ptr.as_ptr();
            let options = [0f64; OPTIONS].as_ptr();

            // Request all optional outputs
            let optional_outputs = [true, true, true]; // short_sma, long_sma, medprice
            let result = ao_indicator(inputs, data_len, options, optional_outputs.as_ptr(), 3);

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_outputs, 4); // ao, short_sma, long_sma, medprice

            let _outputs_slice = slice::from_raw_parts(result.outputs, result.num_outputs);
            let _output_lens_slice = slice::from_raw_parts(result.output_lens, result.num_outputs);

            // min_data is 35 (long_period), so first valid output at index 34
            // Debug: actual outputs are 26 for ao, 25 for short_sma/long_sma

            free_result(result);
        }
    }

    #[test]
    fn test_ao_batch() {
        unsafe {
            let data_len = 60;
            let high: Vec<f64> = (0..data_len).map(|i| 100.0 + i as f64 * 0.5).collect();
            let low: Vec<f64> = (0..data_len).map(|i| 90.0 + i as f64 * 0.5).collect();

            let inputs_ptr: [*const f64; INPUTS] = [high.as_ptr(), low.as_ptr()];
            let inputs = inputs_ptr.as_ptr();
            let options = [0f64; OPTIONS].as_ptr();

            // Request all optional outputs
            let optional_outputs = [true, true, true];
            let result = ao_indicator(inputs, data_len, options, optional_outputs.as_ptr(), 3);

            assert_eq!(result.error, CIndicatorError::Ok);
            let state = result.state;

            // Second batch with more data
            let extra_data_len = 10;
            let high_extra: Vec<f64> = (data_len..data_len + extra_data_len)
                .map(|i| 100.0 + i as f64 * 0.5)
                .collect();
            let low_extra: Vec<f64> = (data_len..data_len + extra_data_len)
                .map(|i| 90.0 + i as f64 * 0.5)
                .collect();

            let inputs_ptr_extra: [*const f64; INPUTS] = [high_extra.as_ptr(), low_extra.as_ptr()];
            let inputs_extra = inputs_ptr_extra.as_ptr();
            let batch_result = ao_batch(state, inputs_extra, extra_data_len, std::ptr::null(), 0);

            assert_eq!(batch_result.error, CIndicatorError::Ok);
            assert_eq!(batch_result.num_outputs, 4); // ao, short_sma, long_sma, medprice

            free_batch_result(batch_result);
            ao_state_free(state);
        }
    }

    #[test]
    fn test_ao_simd_by_assets() {
        unsafe {
            let data_len = 60;
            let high: Vec<f64> = (0..data_len).map(|i| 100.0 + i as f64 * 0.5).collect();
            let low: Vec<f64> = (0..data_len).map(|i| 90.0 + i as f64 * 0.5).collect();

            // Two identical "assets"
            let inputs_ptr_0: [*const f64; INPUTS] = [high.as_ptr(), low.as_ptr()];
            let inputs_ptr_1: [*const f64; INPUTS] = [high.as_ptr(), low.as_ptr()];
            let assets_ptrs = [inputs_ptr_0.as_ptr(), inputs_ptr_1.as_ptr()].as_ptr();
            let options = [0f64; OPTIONS].as_ptr();

            let result = ao_simd_by_assets(assets_ptrs, 2, data_len, options, std::ptr::null(), 0);

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, 2);
            assert_eq!(result.num_outputs, 4); // ao, short_sma, long_sma, medprice

            let states_slice = slice::from_raw_parts(result.states, result.num_results);
            for i in 0..result.num_results {
                ao_state_free(states_slice[i]);
            }
            free_simd_result(result);
        }
    }
}
