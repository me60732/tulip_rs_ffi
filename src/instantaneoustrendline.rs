//! extern "C" wrapper for `instantaneoustrendline`, mirroring the core `tulip_rs` crate's
//! `InstantaneousTrendline::indicator` / `IndicatorState::batch_indicator` interface with a
//! Tulip-Indicators-style C calling convention: `inputs` is an array of
//! `INPUTS` pointers (one per input series, each `data_len` long, in the same
//! order as `tulip_rs::indicators::instantaneoustrendline::InstantaneousTrendline::INFO.inputs`),
//! `options` is a flat array of `OPTIONS` values. Output allocation stays inside
//! the indicator function (never caller-supplied).
//!
//! Parameter order convention (kept consistent across every function in
//! this crate): each pointer parameter is immediately followed by the
//! count(s) that describe it, e.g. `inputs, data_len, options, ...,
//! optional_outputs, numoptional`.
//!
//! SIMD entry points:
//! - `instantaneoustrendline_simd_by_assets`: compute InstantaneousTrendline for N assets simultaneously,
//!   sharing a single options array. N must be 2, 4, 8, or 16.
//!
//! NOTE: `instantaneoustrendline` has OPTIONS=0, so there is no
//! `instantaneoustrendline_simd_by_options` function.
//!
//! `#[no_mangle]` functions can't be generic, so each SIMD entry point is a
//! thin non-generic dispatcher that matches the runtime `N` against
//! {2,4,8,16} and calls a private generic `_n::<N>` helper that does the
//! real work.

use std::os::raw::c_void;

use tulip_rs::indicator_types::{Indicator, TIndicatorState};
use tulip_rs::indicators::instantaneoustrendline::{
    IndicatorState as InstantaneousTrendlineState, InstantaneousTrendline, INPUTS, OPTIONS,
};
use tulip_rs::types::IndicatorError;

use crate::common::{
    optional_outputs_slice, pack_info, pack_outputs, pack_simd_outputs, pack_states, read_inputs,
    read_simd_assets_inputs, CBatchResult, CIndicatorError, CIndicatorInfo, CIndicatorResult,
    CSimdResult,
};

/// Opaque state handle returned by `instantaneoustrendline_indicator()` and consumed by
/// `instantaneoustrendline_batch()` / `instantaneoustrendline_state_free()`.
pub type InstantaneousTrendlineStateHandle = InstantaneousTrendlineState;

/// Returns static metadata about the `instantaneoustrendline` indicator: its name, input
/// names, option names, and (mandatory/optional) output names, mirroring
/// `InstantaneousTrendline::INFO`.
///
/// The returned strings are leaked, process-lifetime C strings -- read them,
/// don't free them.
#[no_mangle]
pub extern "C" fn instantaneoustrendline_info() -> CIndicatorInfo {
    pack_info(&InstantaneousTrendline::INFO)
}

/// Returns the minimum number of bars `instantaneoustrendline` needs to produce any output at
/// all, given `options`.
#[no_mangle]
pub extern "C" fn instantaneoustrendline_min_data(_options: *const f64) -> usize {
    InstantaneousTrendline::min_data(&[])
}

/// Runs `instantaneoustrendline` over `data_len` bars.
///
/// `inputs` must point to `INPUTS` (1) pointer: `real`, `data_len` `f64`s long.
/// `options` must point to `OPTIONS` (0) values (empty array).
///
/// Returns the mandatory `instantaneoustrendline` output plus any requested optional outputs
/// (`trigger, dc_period, alpha`) and a fresh continuation state.
///
/// # Safety
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s (for OPTIONS=0, pass a null pointer or any pointer).
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s (pass null + 0 to request no optional outputs).
#[no_mangle]
pub unsafe extern "C" fn instantaneoustrendline_indicator(
    inputs: *const *const f64,
    data_len: usize,
    _options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CIndicatorResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    // OPTIONS=0: nothing to read; empty array literal (dereferencing a null
    // pointer would be UB even for a zero-sized type).
    let options: [f64; OPTIONS] = [];
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match InstantaneousTrendline::indicator(&inputs, &options, optional) {
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

/// Continues an `instantaneoustrendline` calculation from `state`, mutating it in place so
/// it's ready for the next call. `state` must have come from
/// `instantaneoustrendline_indicator()` and not yet have been passed to `instantaneoustrendline_state_free()`.
///
/// `inputs` must point to `INPUTS` (1) pointer: `real`, `data_len` `f64`s long.
///
/// # Safety
/// - `state` must be a live pointer previously returned by
///   `instantaneoustrendline_indicator()`.
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn instantaneoustrendline_batch(
    state: *mut c_void,
    inputs: *const *const f64,
    data_len: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CBatchResult {
    if state.is_null() {
        return CBatchResult::err(IndicatorError::InvalidIndicatorState);
    }
    let state = &mut *(state as *mut InstantaneousTrendlineStateHandle);

    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    // OPTIONS=0: use empty array reference
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

/// Frees a state handle returned by `instantaneoustrendline_indicator()` (or one of the
/// `states[i]` entries from `instantaneoustrendline_simd_by_assets()`).
/// Call this once you're done streaming (after your last `instantaneoustrendline_batch()` call).
///
/// # Safety
/// `state` must be a live pointer previously returned by `instantaneoustrendline_indicator()`
/// or found in a `CSimdResult::states` array, and must not be used again
/// after this call.
#[no_mangle]
pub unsafe extern "C" fn instantaneoustrendline_state_free(state: *mut c_void) {
    if !state.is_null() {
        drop(Box::from_raw(
            state as *mut InstantaneousTrendlineStateHandle,
        ));
    }
}

/// Computes InstantaneousTrendline for `N` assets simultaneously (SIMD), sharing a single
/// options array. `num_assets` must be 2, 4, 8, or 16 -- anything else
/// returns `InvalidInputs`.
///
/// `inputs` must point to `num_assets` pointers, each itself pointing to
/// `INPUTS` (1) pointer (`real`), each `data_len` `f64`s long. `options` must point
/// to `OPTIONS` (0) shared values.
///
/// Returns a `CSimdResult` with `num_assets` results, each with its own
/// output rows and its own ordinary `instantaneoustrendline` continuation state (reusable
/// with `instantaneoustrendline_batch()`/`instantaneoustrendline_state_free()`). Free each state via
/// `instantaneoustrendline_state_free()`, then free the rest via `tulip_ffi_simd_result_free()`.
///
/// # Safety
/// - `inputs` must point to `num_assets` valid pointers, each pointing to
///   `INPUTS` valid non-null `*const f64`s, each pointing to `data_len` valid
///   `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s (for OPTIONS=0, pass a null pointer or any pointer).
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn instantaneoustrendline_simd_by_assets(
    inputs: *const *const *const f64,
    num_assets: usize,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    match num_assets {
        2 => instantaneoustrendline_simd_by_assets_n::<2>(
            inputs,
            data_len,
            options,
            optional_outputs,
            numoptional,
        ),
        4 => instantaneoustrendline_simd_by_assets_n::<4>(
            inputs,
            data_len,
            options,
            optional_outputs,
            numoptional,
        ),
        8 => instantaneoustrendline_simd_by_assets_n::<8>(
            inputs,
            data_len,
            options,
            optional_outputs,
            numoptional,
        ),
        16 => instantaneoustrendline_simd_by_assets_n::<16>(
            inputs,
            data_len,
            options,
            optional_outputs,
            numoptional,
        ),
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn instantaneoustrendline_simd_by_assets_n<const N: usize>(
    inputs: *const *const *const f64,
    data_len: usize,
    _options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    // `owned` holds the per-asset input slices; `refs` borrows from it, so
    // both must live in this stack frame for the duration of the call.
    let owned = read_simd_assets_inputs::<N, INPUTS>(inputs, data_len);
    let refs: [&[&[f64]; INPUTS]; N] = std::array::from_fn(|i| &owned[i]);
    // OPTIONS=0: nothing to read; empty array literal (dereferencing a null
    // pointer would be UB even for a zero-sized type).
    let _options: [f64; OPTIONS] = [];
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match InstantaneousTrendline::indicator_by_assets::<N>(&refs, &_options, optional) {
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

    fn min_data(_options: &[f64; OPTIONS]) -> usize {
        InstantaneousTrendline::min_data(&[0.0; OPTIONS])
    }

    #[test]
    fn test_instantaneoustrendline_info() {
        let info = instantaneoustrendline_info();
        assert!(info.inputs.len > 0);
        assert_eq!(info.options.len, 0);
        assert!(info.outputs.len > 0);
        assert!(info.optional_outputs.len > 0);
    }

    #[test]
    fn test_instantaneoustrendline_min_data() {
        let min = instantaneoustrendline_min_data(std::ptr::null());
        assert!(min > 0);
    }

    #[test]
    fn test_instantaneoustrendline_indicator() {
        unsafe {
            let len = min_data(&[0.0; OPTIONS]);
            let real = build_synthetic_data(len);

            let inputs: [*const f64; INPUTS] = [real.as_ptr()];

            // For OPTIONS=0, we pass a null pointer for options
            let result = instantaneoustrendline_indicator(
                inputs.as_ptr(),
                len,
                std::ptr::null(),
                std::ptr::null(),
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_outputs, 1); // only trendline (mandatory)

            tulip_ffi_result_free(result);
        }
    }

    #[test]
    fn test_instantaneoustrendline_batch() {
        unsafe {
            let len1 = min_data(&[0.0; OPTIONS]);
            let real1 = build_synthetic_data(len1);

            let inputs1: [*const f64; INPUTS] = [real1.as_ptr()];

            // For OPTIONS=0, we pass a null pointer for options
            let result = instantaneoustrendline_indicator(
                inputs1.as_ptr(),
                len1,
                std::ptr::null(),
                std::ptr::null(),
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            let state = result.state;

            let len2 = 30;
            let real2 = build_synthetic_data(len2);

            let inputs2: [*const f64; INPUTS] = [real2.as_ptr()];

            // For OPTIONS=0, we pass a null pointer for options
            let batch_result =
                instantaneoustrendline_batch(state, inputs2.as_ptr(), len2, std::ptr::null(), 0);

            assert_eq!(batch_result.error, CIndicatorError::Ok);
            tulip_ffi_batch_result_free(batch_result);
            instantaneoustrendline_state_free(state);
        }
    }

    #[test]
    fn test_instantaneoustrendline_simd_by_assets() {
        const NUM_ASSETS: usize = 2;
        unsafe {
            let len = min_data(&[0.0; OPTIONS]);

            let real1 = build_synthetic_data(len);
            let real2 = build_synthetic_data(len);

            let inputs_array: [*const f64; INPUTS] = [real1.as_ptr()];
            let inputs_array2: [*const f64; INPUTS] = [real2.as_ptr()];

            let inputs: [*const *const f64; NUM_ASSETS] =
                [inputs_array.as_ptr(), inputs_array2.as_ptr()];

            // For OPTIONS=0, we pass a null pointer for options
            let result = instantaneoustrendline_simd_by_assets(
                inputs.as_ptr(),
                NUM_ASSETS,
                len,
                std::ptr::null(),
                std::ptr::null(),
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, NUM_ASSETS);

            for i in 0..NUM_ASSETS {
                let state_ptr = result.states.wrapping_add(i);
                instantaneoustrendline_state_free(*state_ptr);
            }
            tulip_ffi_simd_result_free(result);
        }
    }
}
