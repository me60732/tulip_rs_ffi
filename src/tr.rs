//! extern "C" wrapper for `tr`, mirroring the core `tulip_rs` crate's
//! `Tr::indicator` / `IndicatorState::batch_indicator` interface with a
//! Tulip-Indicators-style C calling convention: `inputs` is an array of
//! `INPUTS` pointers (one per input series, each `data_len` long, in the same
//! order as `tulip_rs::indicators::tr::Tr::INFO.inputs`), `options`
//! is a flat array of `OPTIONS` values -- exactly like `ti_tr`'s
//! `double const *const *inputs, double const *options`. Output allocation
//! stays inside the indicator function (never caller-supplied).
//!
//! Parameter order convention (kept consistent across every function in
//! this crate): each pointer parameter is immediately followed by the
//! count(s) that describe it, e.g. `inputs, data_len, options, ...,
//! optional_outputs, num_optional`.
//!
//! SIMD entry point:
//! - `tr_simd_by_assets`: compute TR for N assets simultaneously.
//!   Note: `tr` has no options, so there is no `simd_by_options` variant.

use std::os::raw::c_void;
use std::ptr;

use tulip_rs::indicator_types::{Indicator, TIndicatorState};
use tulip_rs::indicators::tr::{IndicatorState as TrState, Tr, INPUTS, OPTIONS};
use tulip_rs::types::IndicatorError;

use crate::common::{
    optional_outputs_slice, pack_info, pack_outputs, pack_simd_outputs, pack_states, read_inputs,
    read_simd_assets_inputs, CBatchResult, CIndicatorError, CIndicatorInfo, CIndicatorResult,
    CSimdResult,
};

/// Opaque state handle returned by `tr_indicator()` and consumed by
/// `tr_batch()` / `tr_state_free()`.
pub type TrStateHandle = TrState;

/// Returns static metadata about the `tr` indicator: its name, input
/// names, option names, and (mandatory/optional) output names, mirroring
/// `Tr::INFO`.
///
/// The returned strings are leaked, process-lifetime C strings -- read them,
/// don't free them.
#[no_mangle]
pub extern "C" fn tr_info() -> CIndicatorInfo {
    pack_info(&Tr::INFO)
}

/// Returns the minimum number of bars `tr` needs to produce any output at
/// all, given `options`.
///
/// # Safety
/// NOTE: OPTIONS == 0, no options needed. Pass null or any pointer.
#[no_mangle]
pub unsafe extern "C" fn tr_min_data(_options: *const f64) -> usize {
    Tr::min_data(&[])
}

/// Runs `tr` over `data_len` bars.
///
/// `inputs` must point to `INPUTS` (3) pointers: `high`, `low`, `close`, each `data_len` `f64`s long.
/// `options` must point to `OPTIONS` (0) values -- pass null or any pointer (ignored).
///
/// Returns the mandatory `tr` output plus any requested optional outputs
/// (none for tr) and a fresh continuation state.
///
/// # Safety
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `options` must be null or point to valid memory (ignored).
/// - `optional_outputs`, if non-null, must point to `num_optional` valid
///   `bool`s (pass null + 0 to request no optional outputs).
#[no_mangle]
pub unsafe extern "C" fn tr_indicator(
    inputs: *const *const f64,
    data_len: usize,
    _options: *const f64,
    optional_outputs: *const bool,
    num_optional: usize,
) -> CIndicatorResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    // NOTE: OPTIONS == 0, pass empty slice to core
    let _options: [f64; OPTIONS] = [];
    let optional = optional_outputs_slice(optional_outputs, num_optional);

    match Tr::indicator(&inputs, &[], optional.clone()) {
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

/// Continues an `tr` calculation from `state`, mutating it in place so
/// it's ready for the next call. `state` must have come from
/// `tr_indicator()` and not yet have been passed to `tr_state_free()`.
///
/// `inputs` must point to `INPUTS` (3) pointers: `high`, `low`, `close`, each `data_len` `f64`s long.
///
/// # Safety
/// - `state` must be a live pointer previously returned by
///   `tr_indicator()`.
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `num_optional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn tr_batch(
    state: *mut c_void,
    inputs: *const *const f64,
    data_len: usize,
    optional_outputs: *const bool,
    num_optional: usize,
) -> CBatchResult {
    if state.is_null() {
        return CBatchResult::err(IndicatorError::InvalidIndicatorState);
    }
    let state = &mut *(state as *mut TrStateHandle);

    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let optional = optional_outputs_slice(optional_outputs, num_optional);

    match state.batch_indicator(&inputs, optional.clone()) {
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

/// Frees a state handle returned by `tr_indicator()` (or one of the
/// `states[i]` entries from `tr_simd_by_assets()`).
/// Call this once you're done streaming (after your last `tr_batch()` call).
///
/// # Safety
/// `state` must be a live pointer previously returned by `tr_indicator()`
/// or found in a `CSimdResult::states` array, and must not be used again
/// after this call.
#[no_mangle]
pub unsafe extern "C" fn tr_state_free(state: *mut c_void) {
    if !state.is_null() {
        drop(Box::from_raw(state as *mut TrStateHandle));
    }
}

/// Computes TR for `N` assets simultaneously (SIMD). `num_assets` must be 2, 4, 8, or 16 --
/// anything else returns `InvalidInputs`.
///
/// `inputs` must point to `num_assets` pointers, each itself pointing to
/// `INPUTS` (3) pointers (`high`, `low`, `close`), `data_len` `f64`s long. Options are ignored.
///
/// Returns a `CSimdResult` with `num_assets` results, each with its own
/// output rows and its own ordinary `tr` continuation state (reusable
/// with `tr_batch()`/`tr_state_free()`). Free each state via
/// `tr_state_free()`, then free the rest via `tulip_ffi_simd_result_free()`.
///
/// # Safety
/// - `inputs` must point to `num_assets` valid pointers, each pointing to
///   `INPUTS` valid non-null `*const f64`s, each pointing to `data_len` valid
///   `f64`s.
/// - `options` must be null or point to valid memory (ignored).
/// - `optional_outputs`, if non-null, must point to `num_optional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn tr_simd_by_assets(
    inputs: *const *const *const f64,
    num_assets: usize,
    data_len: usize,
    _options: *const f64,
    optional_outputs: *const bool,
    num_optional: usize,
) -> CSimdResult {
    match num_assets {
        2 => tr_simd_by_assets_n::<2>(
            inputs,
            data_len,
            ptr::null(),
            optional_outputs,
            num_optional,
        ),
        4 => tr_simd_by_assets_n::<4>(
            inputs,
            data_len,
            ptr::null(),
            optional_outputs,
            num_optional,
        ),
        8 => tr_simd_by_assets_n::<8>(
            inputs,
            data_len,
            ptr::null(),
            optional_outputs,
            num_optional,
        ),
        16 => tr_simd_by_assets_n::<16>(
            inputs,
            data_len,
            ptr::null(),
            optional_outputs,
            num_optional,
        ),
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn tr_simd_by_assets_n<const N: usize>(
    inputs: *const *const *const f64,
    data_len: usize,
    _options: *const f64,
    optional_outputs: *const bool,
    num_optional: usize,
) -> CSimdResult {
    // `owned` holds the per-asset input slices; `refs` borrows from it, so
    // both must live in this stack frame for the duration of the call.
    let owned = read_simd_assets_inputs::<N, INPUTS>(inputs, data_len);
    let refs: [&[&[f64]; INPUTS]; N] = std::array::from_fn(|i| &owned[i]);
    // NOTE: OPTIONS == 0, pass empty slice to core
    let _options: [f64; OPTIONS] = [];
    let optional = optional_outputs_slice(optional_outputs, num_optional);

    match Tr::indicator_by_assets::<N>(&refs, &[], optional) {
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

// NOTE: OPTIONS == 0, no simd_by_options

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::test::build_synthetic_data;
    use crate::{tulip_ffi_batch_result_free, tulip_ffi_result_free, tulip_ffi_simd_result_free};
    use std::ptr;

    #[test]
    fn test_tr_indicator() {
        unsafe {
            let data_len = 20;
            // Create high, low, close prices
            let high_arr: Vec<f64> = build_synthetic_data(data_len, 0);
            let low_arr: Vec<f64> = build_synthetic_data(data_len, 0);
            let close_arr: Vec<f64> = build_synthetic_data(data_len, 0);

            let inputs_arr: [*const f64; INPUTS] =
                [high_arr.as_ptr(), low_arr.as_ptr(), close_arr.as_ptr()];
            let inputs: *const *const f64 = inputs_arr.as_ptr();
            // OPTIONS == 0, pass null
            let options: *const f64 = ptr::null();

            let result = tr_indicator(inputs, data_len, options, ptr::null(), 0);

            assert_eq!(result.error, CIndicatorError::Ok);
            // mandatory=tr + optional=[] = 1 total
            assert_eq!(result.num_outputs, 1);
            assert!(!result.state.is_null());

            tulip_ffi_result_free(result);
        }
    }

    #[test]
    fn test_tr_batch() {
        unsafe {
            let data_len = 20;
            // Create high, low, close prices
            let high_arr: Vec<f64> = build_synthetic_data(data_len, 0);
            let low_arr: Vec<f64> = build_synthetic_data(data_len, 0);
            let close_arr: Vec<f64> = build_synthetic_data(data_len, 0);

            let inputs_arr: [*const f64; INPUTS] =
                [high_arr.as_ptr(), low_arr.as_ptr(), close_arr.as_ptr()];
            let inputs: *const *const f64 = inputs_arr.as_ptr();
            // OPTIONS == 0, pass null
            let options: *const f64 = ptr::null();

            let result = tr_indicator(inputs, data_len, options, ptr::null(), 0);
            assert_eq!(result.error, CIndicatorError::Ok);

            let new_high_arr: Vec<f64> = build_synthetic_data(data_len, 0);
            let new_low_arr: Vec<f64> = build_synthetic_data(data_len, 0);
            let new_close_arr: Vec<f64> = build_synthetic_data(data_len, 0);

            let new_inputs_arr: [*const f64; INPUTS] = [
                new_high_arr.as_ptr(),
                new_low_arr.as_ptr(),
                new_close_arr.as_ptr(),
            ];
            let new_inputs: *const *const f64 = new_inputs_arr.as_ptr();
            // OPTIONS == 0, pass null (ignored)

            let batch_result = tr_batch(result.state, new_inputs, 10, ptr::null(), 0);
            assert_eq!(batch_result.error, CIndicatorError::Ok);

            tulip_ffi_batch_result_free(batch_result);
            tr_state_free(result.state);
        }
    }

    #[test]
    fn test_tr_simd_by_assets() {
        unsafe {
            let data_len = 20;
            let num_assets = 2;

            // Create inputs for 2 assets
            let asset0_high: Vec<f64> = build_synthetic_data(data_len, 0);
            let asset0_low: Vec<f64> = build_synthetic_data(data_len, 0);
            let asset0_close: Vec<f64> = build_synthetic_data(data_len, 0);

            let asset1_high: Vec<f64> = build_synthetic_data(data_len, 0);
            let asset1_low: Vec<f64> = build_synthetic_data(data_len, 0);
            let asset1_close: Vec<f64> = build_synthetic_data(data_len, 0);

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

            let inputs_arr_arr: [*const *const f64; 2] =
                [inputs_arr0.as_ptr(), inputs_arr1.as_ptr()];
            let inputs: *const *const *const f64 = inputs_arr_arr.as_ptr();
            // OPTIONS == 0, pass null
            let options: *const f64 = ptr::null();

            let result = tr_simd_by_assets(inputs, num_assets, data_len, options, ptr::null(), 0);

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, 2);
            // mandatory=tr + optional=[] = 1 total
            assert_eq!(result.num_outputs, 1);
            assert!(!result.states.is_null());

            tulip_ffi_simd_result_free(result);
        }
    }

    #[test]
    fn test_tr_info() {
        let info = tr_info();
        assert!(info.inputs.len > 0);
        // OPTIONS == 0
        assert_eq!(info.options.len, 0);
        assert!(info.outputs.len > 0);
        assert_eq!(info.optional_outputs.len, 0);
    }

    #[test]
    fn test_tr_min_data() {
        unsafe {
            let options: *const f64 = ptr::null();
            let min = tr_min_data(options);
            // TR needs at least 2 bars (data_len - 1 output)
            assert_eq!(min, 2);
        }
    }
}
