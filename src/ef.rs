//! extern "C" wrapper for `ef`, mirroring the core `tulip_rs` crate's
//! `Ef::indicator` / `IndicatorState::batch_indicator` interface with a
//! Tulip-Indicators-style C calling convention: `inputs` is an array of
//! `INPUTS` pointers (one per input series, each `data_len` long, in the same
//! order as `tulip_rs::indicators::ef::Ef::INFO.inputs`), `options`
//! is a flat array of `OPTIONS` values -- exactly like `ti_ef`'s
//! `double const *const *inputs, double const *options`. Output allocation
//! stays inside the indicator function (never caller-supplied).
//!
//! Parameter order convention (kept consistent across every function in
//! this crate): each pointer parameter is immediately followed by the
//! count(s) that describe it, e.g. `inputs, data_len, options, ...,
//! optional_outputs, numoptional`.
//!
//! SIMD entry points:
//! - `ef_simd_by_assets`: compute EF for N assets simultaneously,
//!   sharing a single options array. N must be 2, 4, 8, or 16.
//! - `ef_simd_by_options`: compute EF for one asset with N different
//!   option sets simultaneously. N must be 2, 4, 8, or 16.
//!
//! `#[no_mangle]` functions can't be generic, so each SIMD entry point is a
//! thin non-generic dispatcher that matches the runtime `N` against
//! {2,4,8,16} and calls a private generic `_n::<N>` helper that does the
//! real work.

use std::os::raw::c_void;

use tulip_rs::indicator_types::{Indicator, IndicatorByOptions, TIndicatorState};
use tulip_rs::indicators::ef::{Ef, IndicatorState as EfState, INPUTS, OPTIONS};
use tulip_rs::types::IndicatorError;

use crate::common::{
    optional_outputs_slice, pack_info, pack_outputs, pack_simd_outputs, pack_states, read_inputs,
    read_simd_assets_inputs, read_simd_options, CBatchResult, CIndicatorError, CIndicatorInfo,
    CIndicatorResult, CSimdResult,
};

/// Opaque state handle returned by `ef_indicator()` and consumed by
/// `ef_batch()` / `ef_state_free().
pub type EfStateHandle = EfState;

/// Returns static metadata about the `ef` indicator: its name, input
/// names, option names, and (mandatory/optional) output names, mirroring
/// `Ef::INFO`.
///
/// The returned strings are leaked, process-lifetime C strings -- read them,
/// don't free them.
#[no_mangle]
pub extern "C" fn ef_info() -> CIndicatorInfo {
    pack_info(&Ef::INFO)
}

/// Returns the minimum number of bars `ef` needs to produce any output at
/// all, given `options`.
///
/// # Safety
/// `options` must point to `OPTIONS` (1) valid `f64`s.
#[no_mangle]
pub unsafe extern "C" fn ef_min_data(options: *const f64) -> usize {
    let options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    Ef::min_data(&options)
}

/// Runs `ef` over `data_len` bars.
///
/// `inputs` must point to `INPUTS` (1) pointer: `real`, `data_len` `f64`s long.
/// `options` must point to `OPTIONS` (1) value: `period`.
///
/// Returns the mandatory `ef` output plus any requested optional outputs
/// (none for ef) and a fresh continuation state.
///
/// # Safety
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s (pass null + 0 to request no optional outputs).
#[no_mangle]
pub unsafe extern "C" fn ef_indicator(
    inputs: *const *const f64,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CIndicatorResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match Ef::indicator(&inputs, &options, optional) {
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

/// Continues an `ef` calculation from `state`, mutating it in place so
/// it's ready for the next call. `state` must have come from
/// `ef_indicator()` and not yet have been passed to `ef_state_free()`.
///
/// `inputs` must point to `INPUTS` (1) pointer: `real`, `data_len` `f64`s long.
///
/// # Safety
/// - `state` must be a live pointer previously returned by
///   `ef_indicator()`.
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn ef_batch(
    state: *mut c_void,
    inputs: *const *const f64,
    data_len: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CBatchResult {
    if state.is_null() {
        return CBatchResult::err(IndicatorError::InvalidIndicatorState);
    }
    let state = &mut *(state as *mut EfStateHandle);

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

/// Frees a state handle returned by `ef_indicator()` (or one of the
/// `states[i]` entries from `ef_simd_by_assets()`/`ef_simd_by_options()`).
/// Call this once you're done streaming (after your last `ef_batch()` call).
///
/// # Safety
/// `state` must be a live pointer previously returned by `ef_indicator()`
/// or found in a `CSimdResult::states` array, and must not be used again
/// after this call.
#[no_mangle]
pub unsafe extern "C" fn ef_state_free(state: *mut c_void) {
    if !state.is_null() {
        drop(Box::from_raw(state as *mut EfStateHandle));
    }
}

/// Computes EF for `N` assets simultaneously (SIMD), sharing a single
/// options array. `num_assets` must be 2, 4, 8, or 16 -- anything else
/// returns `InvalidInputs`.
///
/// `inputs` must point to `num_assets` pointers, each itself pointing to
/// `INPUTS` (1) pointer (`real`), `data_len` `f64`s long. `options` must point
/// to `OPTIONS` (1) shared values.
///
/// Returns a `CSimdResult` with `num_assets` results, each with its own
/// output rows and its own ordinary `ef` continuation state (reusable
/// with `ef_batch()`/`ef_state_free()`). Free each state via
/// `ef_state_free()`, then free the rest via `tulip_ffi_simd_result_free()`.
///
/// # Safety
/// - `inputs` must point to `num_assets` valid pointers, each pointing to
///   `INPUTS` valid non-null `*const f64`s, each pointing to `data_len` valid
///   `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn ef_simd_by_assets(
    inputs: *const *const *const f64,
    num_assets: usize,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    match num_assets {
        2 => ef_simd_by_assets_n::<2>(inputs, data_len, options, optional_outputs, numoptional),
        4 => ef_simd_by_assets_n::<4>(inputs, data_len, options, optional_outputs, numoptional),
        8 => ef_simd_by_assets_n::<8>(inputs, data_len, options, optional_outputs, numoptional),
        16 => ef_simd_by_assets_n::<16>(inputs, data_len, options, optional_outputs, numoptional),
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn ef_simd_by_assets_n<const N: usize>(
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

    match Ef::indicator_by_assets::<N>(&refs, &options, optional) {
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

/// Computes EF for one asset with `N` different option sets
/// simultaneously (SIMD). `num_option_sets` must be 2, 4, 8, or 16 --
/// anything else returns `InvalidInputs`.
///
/// `inputs` must point to `INPUTS` (1) pointer (`real`), `data_len` `f64`s
/// long, shared across all option sets. `options` must point to
/// `num_option_sets` pointers, each itself pointing to `OPTIONS` (1)
/// values.
///
/// Returns a `CSimdResult` with `num_option_sets` results, each with its
/// own output rows and its own ordinary `ef` continuation state
/// (reusable with `ef_batch()`/`ef_state_free()`). Free each state via
/// `ef_state_free()`, then free the rest via
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
pub unsafe extern "C" fn ef_simd_by_options(
    inputs: *const *const f64,
    data_len: usize,
    options: *const *const f64,
    num_option_sets: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    match num_option_sets {
        2 => ef_simd_by_options_n::<2>(inputs, data_len, options, optional_outputs, numoptional),
        4 => ef_simd_by_options_n::<4>(inputs, data_len, options, optional_outputs, numoptional),
        8 => ef_simd_by_options_n::<8>(inputs, data_len, options, optional_outputs, numoptional),
        16 => ef_simd_by_options_n::<16>(inputs, data_len, options, optional_outputs, numoptional),
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn ef_simd_by_options_n<const N: usize>(
    inputs: *const *const f64,
    data_len: usize,
    options: *const *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let options = read_simd_options::<N, OPTIONS>(options);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match Ef::indicator_by_options::<N>(&inputs, &options, optional) {
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
    use crate::{tulip_ffi_batch_result_free, tulip_ffi_result_free, tulip_ffi_simd_result_free};
    use std::ptr;

    #[test]
    fn test_ef_info() {
        let info = ef_info();
        assert!(info.inputs.len > 0);
        assert_eq!(info.options.len, 1);
        assert!(info.outputs.len > 0);
        assert_eq!(info.optional_outputs.len, 0);
    }

    #[test]
    fn test_ef_min_data() {
        unsafe {
            let options: [f64; OPTIONS] = [5.0];
            let min = ef_min_data(options.as_ptr());
            assert!(min > 0);
        }
    }

    #[test]
    fn test_ef_indicator() {
        unsafe {
            let data_len = 20;
            let inputs_arr: Vec<f64> = (1..=data_len).map(|x| x as f64).collect();
            let inputs: *const *const f64 = &inputs_arr.as_ptr();
            let options_arr: [f64; OPTIONS] = [5.0];
            let options: *const f64 = options_arr.as_ptr();

            let result = ef_indicator(inputs, data_len, options, ptr::null(), 0);

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_outputs, 1);
            assert!(!result.state.is_null());

            tulip_ffi_result_free(result);
        }
    }

    #[test]
    fn test_ef_batch() {
        unsafe {
            let data_len = 20;
            let inputs_arr: Vec<f64> = (1..=data_len).map(|x| x as f64).collect();
            let inputs: *const *const f64 = &inputs_arr.as_ptr();
            let options_arr: [f64; OPTIONS] = [5.0];
            let options: *const f64 = options_arr.as_ptr();

            let result = ef_indicator(inputs, data_len, options, ptr::null(), 0);
            assert_eq!(result.error, CIndicatorError::Ok);

            let new_inputs_arr: Vec<f64> = (21..=30).map(|x| x as f64).collect();
            let new_inputs: *const *const f64 = &new_inputs_arr.as_ptr();

            let batch_result = ef_batch(result.state, new_inputs, 10, ptr::null(), 0);
            assert_eq!(batch_result.error, CIndicatorError::Ok);

            tulip_ffi_batch_result_free(batch_result);
            ef_state_free(result.state);
        }
    }

    #[test]
    fn test_ef_simd_by_assets() {
        unsafe {
            let data_len = 20;
            let num_assets = 2;

            // Create inputs for 2 assets
            let asset0_inputs: Vec<f64> = (1..=data_len).map(|x| x as f64).collect();
            let asset1_inputs: Vec<f64> = (21..=40).map(|x| x as f64).collect();

            let inputs_arr0: [*const f64; INPUTS] = [asset0_inputs.as_ptr()];
            let inputs_arr1: [*const f64; INPUTS] = [asset1_inputs.as_ptr()];

            let assets_ptrs: [*const *const f64; 2] = [inputs_arr0.as_ptr(), inputs_arr1.as_ptr()];
            let inputs: *const *const *const f64 = assets_ptrs.as_ptr();

            let options_arr: [f64; OPTIONS] = [5.0];
            let options: *const f64 = options_arr.as_ptr();

            let result = ef_simd_by_assets(inputs, num_assets, data_len, options, ptr::null(), 0);

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, 2);
            assert_eq!(result.num_outputs, 1);
            assert!(!result.states.is_null());

            for i in 0..num_assets {
                ef_state_free(*result.states.add(i));
            }
            tulip_ffi_simd_result_free(result);
        }
    }

    #[test]
    fn test_ef_simd_by_options() {
        unsafe {
            let data_len = 20;

            let inputs_arr: Vec<f64> = (1..=data_len).map(|x| x as f64).collect();
            let input_ptrs: [*const f64; INPUTS] = [inputs_arr.as_ptr()];
            let inputs: *const *const f64 = input_ptrs.as_ptr();

            // Create 2 different option sets
            let options0: [f64; OPTIONS] = [5.0];
            let options1: [f64; OPTIONS] = [10.0];
            let options_arr: [*const f64; 2] = [options0.as_ptr(), options1.as_ptr()];
            let options: *const *const f64 = options_arr.as_ptr();

            let result = ef_simd_by_options(inputs, data_len, options, 2, ptr::null(), 0);

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, 2);
            assert_eq!(result.num_outputs, 1);
            assert!(!result.states.is_null());

            for i in 0..2 {
                ef_state_free(*result.states.add(i));
            }
            tulip_ffi_simd_result_free(result);
        }
    }
}
