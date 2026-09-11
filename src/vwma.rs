//! extern "C" wrapper for `vwma`, mirroring the core `tulip_rs` crate's
//! `Vwma::indicator` / `IndicatorState::batch_indicator` interface with a
//! Tulip-Indicators-style C calling convention: `inputs` is an array of
//! `INPUTS` pointers (one per input series, each `data_len` long, in the same
//! order as `tulip_rs::indicators::vwma::Vwma::INFO.inputs`), `options`
//! is a flat array of `OPTIONS` values -- exactly like `ti_vwma`'s
//! `double const *const *inputs, double const *options`. Output allocation
//! stays inside the indicator function (never caller-supplied).
//!
//! Parameter order convention (kept consistent across every function in
//! this crate): each pointer parameter is immediately followed by the
//! count(s) that describe it, e.g. `inputs, data_len, options, ...,
//! optional_outputs, numoptional`.
//!
//! SIMD entry points:
//! - `vwma_simd_by_assets`: compute VWMA for N assets simultaneously,
//!   sharing a single options array. N must be 2, 4, 8, or 16.
//! - `vwma_simd_by_options`: compute VWMA for N option sets simultaneously,
//!   using the same inputs data. N must be 2, 4, 8, or 16.

use std::os::raw::c_void;

use tulip_rs::indicator_types::{Indicator, IndicatorByOptions, TIndicatorState};
use tulip_rs::indicators::vwma::{IndicatorState as VwmaState, Vwma, INPUTS, OPTIONS};
use tulip_rs::types::IndicatorError;

use crate::common::{
    optional_outputs_slice, pack_info, pack_outputs, pack_simd_outputs, pack_states, read_inputs,
    read_simd_assets_inputs, read_simd_options, CBatchResult, CIndicatorError, CIndicatorInfo,
    CIndicatorResult, CSimdResult,
};

/// Opaque state handle returned by `vwma_indicator()` and consumed by
/// `vwma_batch()` / `vwma_state_free()`.
pub type VwmaStateHandle = VwmaState;

/// Returns static metadata about the `vwma` indicator: its name, input
/// names, option names, and (mandatory/optional) output names, mirroring
/// `Vwma::INFO`.
///
/// The returned strings are leaked, process-lifetime C strings -- read them,
/// don't free them.
#[no_mangle]
pub extern "C" fn vwma_info() -> CIndicatorInfo {
    pack_info(&Vwma::INFO)
}

/// Returns the minimum number of bars `vwma` needs to produce any output at
/// all, given `options`.
#[no_mangle]
pub unsafe extern "C" fn vwma_min_data(options: *const f64) -> usize {
    let _options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    Vwma::min_data(&_options)
}

/// Runs `vwma` over `data_len` bars.
///
/// `inputs` must point to `INPUTS` (2) pointers, in order:
/// `close, volume`, each `data_len` `f64`s long.
/// `options` must point to `OPTIONS` (1) values: `period`.
///
/// Returns the mandatory `vwma` output plus any requested optional outputs
/// (none for this indicator) and a fresh continuation state.
///
/// # Safety
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
#[no_mangle]
pub unsafe extern "C" fn vwma_indicator(
    inputs: *const *const f64,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CIndicatorResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let _options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match Vwma::indicator(&inputs, &_options, optional) {
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

/// Continues a `vwma` calculation from `state`, mutating it in place so
/// it's ready for the next call. `state` must have come from
/// `vwma_indicator()` and not yet have been passed to `vwma_state_free()`.
///
/// `inputs` must point to `INPUTS` (2) pointers (`close, volume`), each `data_len` `f64`s long.
///
/// # Safety
/// - `state` must be a live pointer previously returned by
///   `vwma_indicator()`.
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
#[no_mangle]
pub unsafe extern "C" fn vwma_batch(
    state: *mut c_void,
    inputs: *const *const f64,
    data_len: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CBatchResult {
    if state.is_null() {
        return CBatchResult::err(IndicatorError::InvalidIndicatorState);
    }
    let state = &mut *(state as *mut VwmaStateHandle);

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

/// Frees a state handle returned by `vwma_indicator()` (or one of the
/// `states[i]` entries from `vwma_simd_by_assets()` or `vwma_simd_by_options()`).
/// Call this once you're done streaming (after your last `vwma_batch()` call).
///
/// # Safety
/// `state` must be a live pointer previously returned by `vwma_indicator()`
/// or found in a `CSimdResult::states` array, and must not be used again
/// after this call.
#[no_mangle]
pub unsafe extern "C" fn vwma_state_free(state: *mut c_void) {
    if !state.is_null() {
        drop(Box::from_raw(state as *mut VwmaStateHandle));
    }
}

/// Computes VWMA for `N` assets simultaneously (SIMD), sharing a single
/// options array. `num_assets` must be 2, 4, 8, or 16 -- anything else
/// returns `InvalidInputs`.
///
/// `inputs` must point to `num_assets` pointers, each itself pointing to
/// `INPUTS` (2) pointers (`close, volume`), each `data_len` `f64`s long.
/// `options` must point to `OPTIONS` (1) shared values: `period`.
///
/// Returns a `CSimdResult` with `num_assets` results, each with its own
/// output rows and its own ordinary `vwma` continuation state (reusable
/// with `vwma_batch()`/`vwma_state_free()`). Free each state via
/// `vwma_state_free()`, then free the rest via `tulip_ffi_simd_result_free()`.
///
/// # Safety
/// - `inputs` must point to `num_assets` valid pointers, each pointing to
///   `INPUTS` valid non-null `*const f64`s, each pointing to `data_len` valid
///   `f64`s.
#[no_mangle]
pub unsafe extern "C" fn vwma_simd_by_assets(
    inputs: *const *const *const f64,
    num_assets: usize,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    match num_assets {
        2 => vwma_simd_by_assets_n::<2>(inputs, data_len, options, optional_outputs, numoptional),
        4 => vwma_simd_by_assets_n::<4>(inputs, data_len, options, optional_outputs, numoptional),
        8 => vwma_simd_by_assets_n::<8>(inputs, data_len, options, optional_outputs, numoptional),
        16 => {
            vwma_simd_by_assets_n::<16>(inputs, data_len, options, optional_outputs, numoptional)
        }
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn vwma_simd_by_assets_n<const N: usize>(
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
    let _options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match Vwma::indicator_by_assets::<N>(&refs, &_options, optional) {
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

/// Computes VWMA for `N` option sets simultaneously (SIMD), using the same
/// inputs data. `num_option_sets` must be 2, 4, 8, or 16 -- anything else
/// returns `InvalidInputs`.
///
/// `inputs` must point to `INPUTS` (2) pointers (`close, volume`), each `data_len` `f64`s long.
/// `options` must point to `num_option_sets` arrays of `OPTIONS` (1) values: `period`.
///
/// Returns a `CSimdResult` with `num_option_sets` results, each with its own
/// output rows and its own ordinary `vwma` continuation state (reusable
/// with `vwma_batch()`/`vwma_state_free()`). Free each state via
/// `vwma_state_free()`, then free the rest via `tulip_ffi_simd_result_free()`.
///
/// # Safety
/// - `inputs` must point to `INPUTS` valid non-null `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `options` must point to `num_option_sets` valid arrays of `OPTIONS` `f64`s.
#[no_mangle]
pub unsafe extern "C" fn vwma_simd_by_options(
    inputs: *const *const f64,
    data_len: usize,
    options: *const *const f64,
    num_option_sets: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    match num_option_sets {
        2 => vwma_simd_by_options_n::<2>(inputs, data_len, options, optional_outputs, numoptional),
        4 => vwma_simd_by_options_n::<4>(inputs, data_len, options, optional_outputs, numoptional),
        8 => vwma_simd_by_options_n::<8>(inputs, data_len, options, optional_outputs, numoptional),
        16 => {
            vwma_simd_by_options_n::<16>(inputs, data_len, options, optional_outputs, numoptional)
        }
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn vwma_simd_by_options_n<const N: usize>(
    inputs: *const *const f64,
    data_len: usize,
    options: *const *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let options_arr: [&[f64; OPTIONS]; N] = read_simd_options::<N, OPTIONS>(options);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match Vwma::indicator_by_options::<N>(&inputs, &options_arr, optional) {
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

    unsafe fn min_data(options: &[f64; OPTIONS]) -> usize {
        Vwma::min_data(&[options[0]])
    }

    #[test]
    fn test_vwma_info() {
        let info = vwma_info();
        assert!(info.inputs.len > 0);
        assert_eq!(info.options.len, 1);
        assert!(info.outputs.len > 0);
        assert_eq!(info.optional_outputs.len, 0);
    }

    #[test]
    fn test_vwma_min_data() {
        unsafe {
            let options: [f64; OPTIONS] = [10.0];
            let min = vwma_min_data(options.as_ptr());
            assert!(min > 0);
        }
    }

    #[test]
    fn test_vwma_indicator() {
        unsafe {
            let options = [10.0];
            let len = min_data(&options);
            let close = build_synthetic_data(len);
            let volume = build_synthetic_data(len);

            let inputs: [*const f64; INPUTS] = [close.as_ptr(), volume.as_ptr()];

            let result =
                vwma_indicator(inputs.as_ptr(), len, options.as_ptr(), std::ptr::null(), 0);

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_outputs, 1);

            tulip_ffi_result_free(result);
        }
    }

    #[test]
    fn test_vwma_batch() {
        unsafe {
            let options = [10.0];
            let len1 = min_data(&options);
            let close1 = build_synthetic_data(len1);
            let volume1 = build_synthetic_data(len1);

            let inputs1: [*const f64; INPUTS] = [close1.as_ptr(), volume1.as_ptr()];

            let result = vwma_indicator(
                inputs1.as_ptr(),
                len1,
                options.as_ptr(),
                std::ptr::null(),
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            let state = result.state;

            let len2 = 30;
            let close2 = build_synthetic_data(len2);
            let volume2 = build_synthetic_data(len2);

            let inputs2: [*const f64; INPUTS] = [close2.as_ptr(), volume2.as_ptr()];

            let batch_result = vwma_batch(state, inputs2.as_ptr(), len2, std::ptr::null(), 0);

            assert_eq!(batch_result.error, CIndicatorError::Ok);
            tulip_ffi_batch_result_free(batch_result);
            vwma_state_free(state);
        }
    }

    #[test]
    fn test_vwma_simd_by_assets() {
        const NUM_ASSETS: usize = 2;
        unsafe {
            let options = [10.0];
            let len = min_data(&options);

            let close1 = build_synthetic_data(len);
            let volume1 = build_synthetic_data(len);

            let close2 = build_synthetic_data(len);
            let volume2 = build_synthetic_data(len);

            let inputs_array: [*const f64; INPUTS] = [close1.as_ptr(), volume1.as_ptr()];
            let inputs_array2: [*const f64; INPUTS] = [close2.as_ptr(), volume2.as_ptr()];

            let inputs: [*const *const f64; NUM_ASSETS] =
                [inputs_array.as_ptr(), inputs_array2.as_ptr()];

            let result = vwma_simd_by_assets(
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
                vwma_state_free(*state_ptr);
            }
            tulip_ffi_simd_result_free(result);
        }
    }

    #[test]
    fn test_vwma_simd_by_options() {
        const NUM_OPTION_SETS: usize = 2;
        unsafe {
            // Must satisfy min_data for *all* option sets used below.
            let len = min_data(&[10.0]).max(min_data(&[20.0]));

            let close = build_synthetic_data(len);
            let volume = build_synthetic_data(len);

            let inputs: [*const f64; INPUTS] = [close.as_ptr(), volume.as_ptr()];

            let options1: [f64; OPTIONS] = [10.0];
            let options2: [f64; OPTIONS] = [20.0];

            // Bind each option set to a local before taking pointers
            let options1_ptr: *const f64 = options1.as_ptr();
            let options2_ptr: *const f64 = options2.as_ptr();
            let options_array_boxed: Box<[*const f64; NUM_OPTION_SETS]> =
                Box::new([options1_ptr, options2_ptr]);
            let options_ptr: *mut [*const f64; NUM_OPTION_SETS] =
                Box::into_raw(options_array_boxed);
            let result = vwma_simd_by_options(
                inputs.as_ptr(),
                len,
                options_ptr as *const *const f64,
                2,
                std::ptr::null(),
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, NUM_OPTION_SETS);

            for i in 0..NUM_OPTION_SETS {
                let state_ptr = result.states.wrapping_add(i);
                vwma_state_free(*state_ptr);
            }
            tulip_ffi_simd_result_free(result);
        }
    }
}
