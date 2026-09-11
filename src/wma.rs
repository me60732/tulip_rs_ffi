//! extern "C" wrapper for `wma`, mirroring the core `tulip_rs` crate's
//! `Wma::indicator` / `IndicatorState::batch_indicator` interface with a
//! Tulip-Indicators-style C calling convention: `inputs` is an array of
//! `INPUTS` pointers (one per input series, each `data_len` long, in the same
//! order as `tulip_rs::indicators::wma::Wma::INFO.inputs`), `options`
//! is a flat array of `OPTIONS` values -- exactly like `ti_wma`'s
//! `double const *const *inputs, double const *options`. Output allocation
//! stays inside the indicator function (never caller-supplied).
//!
//! Parameter order convention (kept consistent across every function in
//! this crate): each pointer parameter is immediately followed by the
//! count(s) that describe it, e.g. `inputs, data_len, options, ...,
//! optional_outputs, numoptional`.
//!
//! SIMD entry points:
//! - `wma_simd_by_assets`: compute WMA for N assets simultaneously,
//!   sharing a single options array. N must be 2, 4, 8, or 16.
//! - `wma_simd_by_options`: compute WMA with multiple option sets
//!   simultaneously. N must be 2, 4, 8, or 16.

use std::os::raw::c_void;

use tulip_rs::indicator_types::{Indicator, IndicatorByOptions, TIndicatorState};
use tulip_rs::indicators::wma::{IndicatorState as WmaState, Wma, INPUTS, OPTIONS};
use tulip_rs::types::IndicatorError;

use crate::common::{
    optional_outputs_slice, pack_info, pack_outputs, pack_simd_outputs, pack_states, read_inputs,
    read_simd_assets_inputs, read_simd_options, CBatchResult, CIndicatorError, CIndicatorInfo,
    CIndicatorResult, CSimdResult,
};

/// Opaque state handle returned by `wma_indicator()` and consumed by
/// `wma_batch()` / `wma_state_free()`.
pub type WmaStateHandle = WmaState;

/// Returns static metadata about the `wma` indicator: its name, input
/// names, option names, and (mandatory/optional) output names, mirroring
/// `Wma::INFO`.
///
/// The returned strings are leaked, process-lifetime C strings -- read them,
/// don't free them.
#[no_mangle]
pub extern "C" fn wma_info() -> CIndicatorInfo {
    pack_info(&Wma::INFO)
}

/// Returns the minimum number of bars `wma` needs to produce any output at
/// all, given `options`.
///
/// # Safety
/// `options` must point to `OPTIONS` (1) valid `f64`s.
#[no_mangle]
pub unsafe extern "C" fn wma_min_data(options: *const f64) -> usize {
    let options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    Wma::min_data(&options)
}

/// Runs `wma` over `data_len` bars.
///
/// `inputs` must point to `INPUTS` (1) pointer: `real`, `data_len` `f64`s long.
/// `options` must point to `OPTIONS` (1) value: `period`.
///
/// Returns the mandatory `wma` output plus any requested optional outputs
/// (`sma`) and a fresh continuation state.
///
/// # Safety
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s (pass null + 0 to request no optional outputs).
#[no_mangle]
pub unsafe extern "C" fn wma_indicator(
    inputs: *const *const f64,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CIndicatorResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match Wma::indicator(&inputs, &options, optional) {
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

/// Continues a `wma` calculation from `state`, mutating it in place so
/// it's ready for the next call. `state` must have come from
/// `wma_indicator()` and not yet have been passed to `wma_state_free()`.
///
/// `inputs` must point to `INPUTS` (1) pointer (`real`), `data_len` `f64`s long.
///
/// # Safety
/// - `state` must be a live pointer previously returned by
///   `wma_indicator()`.
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
#[no_mangle]
pub unsafe extern "C" fn wma_batch(
    state: *mut c_void,
    inputs: *const *const f64,
    data_len: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CBatchResult {
    if state.is_null() {
        return CBatchResult::err(IndicatorError::InvalidIndicatorState);
    }
    let state = &mut *(state as *mut WmaStateHandle);

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

/// Frees a state handle returned by `wma_indicator()` (or one of the
/// `states[i]` entries from `wma_simd_by_assets()` or `wma_simd_by_options()`).
/// Call this once you're done streaming (after your last `wma_batch()` call).
///
/// # Safety
/// `state` must be a live pointer previously returned by `wma_indicator()`
/// or found in a `CSimdResult::states` array, and must not be used again
/// after this call.
#[no_mangle]
pub unsafe extern "C" fn wma_state_free(state: *mut c_void) {
    if !state.is_null() {
        drop(Box::from_raw(state as *mut WmaStateHandle));
    }
}

/// Computes WMA for `N` assets simultaneously (SIMD), sharing a single
/// options array. `num_assets` must be 2, 4, 8, or 16 -- anything else
/// returns `InvalidInputs`.
///
/// `inputs` must point to `num_assets` pointers, each itself pointing to
/// `INPUTS` (1) pointer (`real`), each `data_len` `f64`s long.
/// `options` must point to `OPTIONS` (1) shared values.
///
/// Returns a `CSimdResult` with `num_assets` results, each with its own
/// output rows and its own ordinary `wma` continuation state (reusable
/// with `wma_batch()`/`wma_state_free()`). Free each state via
/// `wma_state_free()`, then free the rest via `tulip_ffi_simd_result_free()`.
///
/// # Safety
/// - `inputs` must point to `num_assets` valid pointers, each pointing to
///   `INPUTS` valid non-null `*const f64`s, each pointing to `data_len` valid
///   `f64`s.
#[no_mangle]
pub unsafe extern "C" fn wma_simd_by_assets(
    inputs: *const *const *const f64,
    num_assets: usize,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    match num_assets {
        2 => wma_simd_by_assets_n::<2>(inputs, data_len, options, optional_outputs, numoptional),
        4 => wma_simd_by_assets_n::<4>(inputs, data_len, options, optional_outputs, numoptional),
        8 => wma_simd_by_assets_n::<8>(inputs, data_len, options, optional_outputs, numoptional),
        16 => wma_simd_by_assets_n::<16>(inputs, data_len, options, optional_outputs, numoptional),
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn wma_simd_by_assets_n<const N: usize>(
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

    match Wma::indicator_by_assets::<N>(&refs, &options, optional) {
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

/// Computes WMA with `num_option_sets` different option sets simultaneously (SIMD).
/// `num_option_sets` must be 2, 4, 8, or 16 -- anything else returns `InvalidInputs`.
///
/// `inputs` must point to `INPUTS` (1) pointer (`real`), `data_len` `f64`s long.
/// `options` must point to `num_option_sets` pointers, each itself pointing to
/// `OPTIONS` (1) value(s). Each option set produces a separate result.
///
/// Returns a `CSimdResult` with `num_option_sets` results. Free states via
/// `wma_state_free()`, then free the rest via `tulip_ffi_simd_result_free()`.
///
/// # Safety
/// - `inputs` must point to a valid non-null `*const f64` pointing to `data_len` valid `f64`s.
/// - `options` must point to `num_option_sets` valid pointers, each pointing to `OPTIONS` valid `f64`s.
#[no_mangle]
pub unsafe extern "C" fn wma_simd_by_options(
    inputs: *const *const f64,
    data_len: usize,
    options: *const *const f64,
    num_option_sets: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    match num_option_sets {
        2 => wma_simd_by_options_n::<2>(inputs, data_len, options, optional_outputs, numoptional),
        4 => wma_simd_by_options_n::<4>(inputs, data_len, options, optional_outputs, numoptional),
        8 => wma_simd_by_options_n::<8>(inputs, data_len, options, optional_outputs, numoptional),
        16 => wma_simd_by_options_n::<16>(inputs, data_len, options, optional_outputs, numoptional),
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn wma_simd_by_options_n<const N: usize>(
    inputs: *const *const f64,
    data_len: usize,
    options: *const *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let options = read_simd_options::<N, OPTIONS>(options);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match Wma::indicator_by_options::<N>(&inputs, &options, optional) {
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
    use crate::common::{
        tulip_ffi_batch_result_free, tulip_ffi_result_free, tulip_ffi_simd_result_free,
    };

    unsafe fn min_data(options: &[f64; OPTIONS]) -> usize {
        Wma::min_data(&[options[0]])
    }

    #[test]
    fn test_wma_info() {
        let info = wma_info();
        assert!(info.inputs.len > 0);
        assert_eq!(info.options.len, 1);
        assert!(info.outputs.len > 0);
        assert!(info.optional_outputs.len > 0);
    }

    #[test]
    fn test_wma_min_data() {
        unsafe {
            let options: [f64; OPTIONS] = [10.0];
            let min = wma_min_data(options.as_ptr());
            assert!(min > 0);
        }
    }

    #[test]
    fn test_wma_indicator() {
        unsafe {
            let options = [10.0];
            let len = min_data(&options);
            let real = build_synthetic_data(len, 0);

            let inputs: [*const f64; INPUTS] = [real.as_ptr()];

            let result = wma_indicator(inputs.as_ptr(), len, options.as_ptr(), std::ptr::null(), 0);

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_outputs, 1); // only wma (mandatory)

            tulip_ffi_result_free(result);
        }
    }

    #[test]
    fn test_wma_batch() {
        unsafe {
            let options = [10.0];
            let len1 = min_data(&options);
            let real1 = build_synthetic_data(len1, 0);

            let inputs1: [*const f64; INPUTS] = [real1.as_ptr()];

            let result = wma_indicator(
                inputs1.as_ptr(),
                len1,
                options.as_ptr(),
                std::ptr::null(),
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            let state = result.state;

            let len2 = 30;
            let real2 = build_synthetic_data(len2, 0);

            let inputs2: [*const f64; INPUTS] = [real2.as_ptr()];

            let batch_result = wma_batch(state, inputs2.as_ptr(), len2, std::ptr::null(), 0);

            assert_eq!(batch_result.error, CIndicatorError::Ok);
            tulip_ffi_batch_result_free(batch_result);
            wma_state_free(state);
        }
    }

    #[test]
    fn test_wma_simd_by_assets() {
        const NUM_ASSETS: usize = 2;
        unsafe {
            let options = [10.0];
            let len = min_data(&options);

            let real1 = build_synthetic_data(len, 0);
            let real2 = build_synthetic_data(len, 0);

            let inputs_array: [*const f64; INPUTS] = [real1.as_ptr()];
            let inputs_array2: [*const f64; INPUTS] = [real2.as_ptr()];

            let inputs: [*const *const f64; NUM_ASSETS] =
                [inputs_array.as_ptr(), inputs_array2.as_ptr()];

            let result = wma_simd_by_assets(
                inputs.as_ptr(),
                NUM_ASSETS,
                len,
                options.as_ptr(),
                std::ptr::null(),
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, NUM_ASSETS);

            for i in 0..NUM_ASSETS {
                let state_ptr = result.states.wrapping_add(i);
                wma_state_free(*state_ptr);
            }
            tulip_ffi_simd_result_free(result);
        }
    }

    #[test]
    fn test_wma_simd_by_options() {
        const NUM_OPTION_SETS: usize = 2;
        unsafe {
            // Must satisfy min_data for *all* option sets used below.
            let len = min_data(&[10.0]).max(min_data(&[20.0]));

            let real = build_synthetic_data(len, 0);

            let inputs: [*const f64; INPUTS] = [real.as_ptr()];

            // Bind option values to locals before taking pointers
            let options1_val: [f64; OPTIONS] = [10.0];
            let options2_val: [f64; OPTIONS] = [20.0];
            let options1_ptr: *const f64 = options1_val.as_ptr();
            let options2_ptr: *const f64 = options2_val.as_ptr();

            // Create array of option pointers - bind to local first
            let options_array_boxed: Box<[*const f64; NUM_OPTION_SETS]> =
                Box::new([options1_ptr, options2_ptr]);
            let options_ptr: *mut [*const f64; NUM_OPTION_SETS] =
                Box::into_raw(options_array_boxed);

            let result = wma_simd_by_options(
                inputs.as_ptr(),
                len,
                options_ptr as *const *const f64,
                NUM_OPTION_SETS,
                std::ptr::null(),
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, NUM_OPTION_SETS);

            for i in 0..NUM_OPTION_SETS {
                let state_ptr = result.states.wrapping_add(i);
                wma_state_free(*state_ptr);
            }
            tulip_ffi_simd_result_free(result);
        }
    }
}
