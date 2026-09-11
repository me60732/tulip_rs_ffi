//! extern "C" wrapper for `tsf`, mirroring the core `tulip_rs` crate's
//! `Tsf::indicator` / `IndicatorState::batch_indicator` interface with a
//! Tulip-Indicators-style C calling convention: `inputs` is an array of
//! `INPUTS` pointers (one per input series, each `data_len` long, in the same
//! order as `tulip_rs::indicators::tsf::Tsf::INFO.inputs`), `options`
//! is a flat array of `OPTIONS` values -- exactly like `ti_tsf`'s
//! `double const *const *inputs, double const *options`. Output allocation
//! stays inside the indicator function (never caller-supplied).
//!
//! Parameter order convention (kept consistent across every function in
//! this crate): each pointer parameter is immediately followed by the
//! count(s) that describe it, e.g. `inputs, data_len, options, ...,
//! optional_outputs, numoptional`.
//!
//! SIMD entry points:
//! - `tsf_simd_by_assets`: compute TSF for N assets simultaneously,
//!   sharing a single options array. N must be 2, 4, 8, or 16.
//! - `tsf_simd_by_options`: compute TSF for N option sets simultaneously,
//!   using the same inputs. N must be 2, 4, 8, or 16.
//!
//! Note: `tsf` supports both SIMD by-assets and SIMD by-options.

use std::os::raw::c_void;

use tulip_rs::indicator_types::{Indicator, IndicatorByOptions, TIndicatorState};
use tulip_rs::indicators::tsf::{IndicatorState as TsfState, Tsf, INPUTS, OPTIONS};
use tulip_rs::types::IndicatorError;

use crate::common::{
    optional_outputs_slice, pack_info, pack_outputs, pack_simd_outputs, pack_states, read_inputs,
    read_simd_assets_inputs, read_simd_options, CBatchResult, CIndicatorError, CIndicatorInfo,
    CIndicatorResult, CSimdResult,
};

/// Opaque state handle returned by `tsf_indicator()` and consumed by
/// `tsf_batch()` / `tsf_state_free()`.
pub type TsfStateHandle = TsfState;

/// Returns static metadata about the `tsf` indicator: its name, input
/// names, option names, and (mandatory/optional) output names, mirroring
/// `Tsf::INFO`.
///
/// The returned strings are leaked, process-lifetime C strings -- read them,
/// don't free them.
#[no_mangle]
pub extern "C" fn tsf_info() -> CIndicatorInfo {
    pack_info(&Tsf::INFO)
}

/// Returns the minimum number of bars `tsf` needs to produce any output at
/// all, given `options`.
#[no_mangle]
pub unsafe extern "C" fn tsf_min_data(options: *const f64) -> usize {
    let options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    Tsf::min_data(&options)
}

/// Runs `tsf` over `data_len` bars.
///
/// `inputs` must point to `INPUTS` (1) pointer: `real`, `data_len` `f64`s long.
/// `options` must point to `OPTIONS` (1) value: `period`.
///
/// Returns the mandatory `tsf` output plus any requested optional outputs
/// (`linreg`, `linregslope`, `linregintercept`) and a fresh continuation state.
///
/// # Safety
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s (pass null + 0 to request no optional outputs).
#[no_mangle]
pub unsafe extern "C" fn tsf_indicator(
    inputs: *const *const f64,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CIndicatorResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match Tsf::indicator(&inputs, &options, optional) {
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

/// Continues a `tsf` calculation from `state`, mutating it in place so
/// it's ready for the next call. `state` must have come from
/// `tsf_indicator()` and not yet have been passed to `tsf_state_free()`.
///
/// `inputs` must point to `INPUTS` (1) pointer: `real`, `data_len` `f64`s long.
///
/// # Safety
/// - `state` must be a live pointer previously returned by
///   `tsf_indicator()`.
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn tsf_batch(
    state: *mut c_void,
    inputs: *const *const f64,
    data_len: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CBatchResult {
    if state.is_null() {
        return CBatchResult::err(IndicatorError::InvalidIndicatorState);
    }
    let state = &mut *(state as *mut TsfStateHandle);

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

/// Frees a state handle returned by `tsf_indicator()` (or one of the
/// `states[i]` entries from `tsf_simd_by_assets()`/`tsf_simd_by_options()`).
/// Call this once you're done streaming (after your last `tsf_batch()` call).
///
/// # Safety
/// `state` must be a live pointer previously returned by `tsf_indicator()`
/// or found in a `CSimdResult::states` array, and must not be used again
/// after this call.
#[no_mangle]
pub unsafe extern "C" fn tsf_state_free(state: *mut c_void) {
    if !state.is_null() {
        drop(Box::from_raw(state as *mut TsfStateHandle));
    }
}

/// Computes TSF for `N` assets simultaneously (SIMD), sharing a single
/// options array. `num_assets` must be 2, 4, 8, or 16 -- anything else
/// returns `InvalidInputs`.
///
/// `inputs` must point to `num_assets` pointers, each itself pointing to
/// `INPUTS` (1) pointers (`real`), each `data_len` `f64`s long.
/// `options` must point to `OPTIONS` (1) shared value: `period`.
///
/// Returns a `CSimdResult` with `num_assets` results, each with its own
/// output rows and its own ordinary `tsf` continuation state (reusable
/// with `tsf_batch()`/`tsf_state_free()`). Free each state via
/// `tsf_state_free()`, then free the rest via `tulip_ffi_simd_result_free()`.
///
/// # Safety
/// - `inputs` must point to `num_assets` valid pointers, each pointing to
///   `INPUTS` valid non-null `*const f64`s, each pointing to `data_len` valid
///   `f64`s.
#[no_mangle]
pub unsafe extern "C" fn tsf_simd_by_assets(
    inputs: *const *const *const f64,
    num_assets: usize,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    match num_assets {
        2 => tsf_simd_by_assets_n::<2>(inputs, data_len, options, optional_outputs, numoptional),
        4 => tsf_simd_by_assets_n::<4>(inputs, data_len, options, optional_outputs, numoptional),
        8 => tsf_simd_by_assets_n::<8>(inputs, data_len, options, optional_outputs, numoptional),
        16 => tsf_simd_by_assets_n::<16>(inputs, data_len, options, optional_outputs, numoptional),
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn tsf_simd_by_assets_n<const N: usize>(
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

    match Tsf::indicator_by_assets::<N>(&refs, &options, optional) {
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

/// Computes TSF for `N` option sets simultaneously (SIMD), using the same
/// inputs. `num_option_sets` must be 2, 4, 8, or 16 -- anything else
/// returns `InvalidInputs`.
///
/// `inputs` must point to `INPUTS` (1) pointers (`real`), each `data_len` `f64`s long.
/// `options` must point to `num_option_sets` pointers, each itself pointing
/// to `OPTIONS` (1) value: `period`.
///
/// Returns a `CSimdResult` with `num_option_sets` results, each with its own
/// output rows and its own ordinary `tsf` continuation state (reusable
/// with `tsf_batch()`/`tsf_state_free()`). Free each state via
/// `tsf_state_free()`, then free the rest via `tulip_ffi_simd_result_free()`.
///
/// # Safety
/// - `inputs` must point to `INPUTS` valid non-null `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `options` must point to `num_option_sets` valid pointers, each itself
///   pointing to `OPTIONS` valid `f64`s.
#[no_mangle]
pub unsafe extern "C" fn tsf_simd_by_options(
    inputs: *const *const f64,
    data_len: usize,
    options: *const *const f64,
    num_option_sets: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    match num_option_sets {
        2 => tsf_simd_by_options_n::<2>(inputs, data_len, options, optional_outputs, numoptional),
        4 => tsf_simd_by_options_n::<4>(inputs, data_len, options, optional_outputs, numoptional),
        8 => tsf_simd_by_options_n::<8>(inputs, data_len, options, optional_outputs, numoptional),
        16 => tsf_simd_by_options_n::<16>(inputs, data_len, options, optional_outputs, numoptional),
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn tsf_simd_by_options_n<const N: usize>(
    inputs: *const *const f64,
    data_len: usize,
    options: *const *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let options = read_simd_options::<N, OPTIONS>(options);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match Tsf::indicator_by_options::<N>(&inputs, &options, optional) {
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

    #[test]
    fn test_tsf_info() {
        let info = tsf_info();
        assert!(info.inputs.len > 0);
        assert_eq!(info.options.len, 1);
        assert!(info.outputs.len > 0);
        assert_eq!(info.optional_outputs.len, 3);
    }

    #[test]
    fn test_tsf_min_data() {
        unsafe {
            let options: [f64; OPTIONS] = [10.0];
            let min = tsf_min_data(options.as_ptr());
            assert!(min > 0);
        }
    }

    #[test]
    fn test_tsf_indicator() {
        unsafe {
            let data_len = 20;
            let real: Vec<f64> = build_synthetic_data(data_len, 0);

            // Create a proper array of input pointers
            let inputs_ptr: [*const f64; INPUTS] = [real.as_ptr()];
            let inputs = inputs_ptr.as_ptr();
            let options: [f64; OPTIONS] = [10.0];
            let options_ptr = options.as_ptr();
            let result = tsf_indicator(inputs, data_len, options_ptr, std::ptr::null(), 0);

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_outputs, 1);
            tulip_ffi_result_free(result);
        }
    }

    #[test]
    fn test_tsf_batch() {
        unsafe {
            let data_len = 20;
            let real: Vec<f64> = build_synthetic_data(data_len, 0);

            // Create a proper array of input pointers
            let inputs_ptr: [*const f64; INPUTS] = [real.as_ptr()];
            let inputs = inputs_ptr.as_ptr();
            let options: [f64; OPTIONS] = [10.0];
            let options_ptr = options.as_ptr();
            let result = tsf_indicator(inputs, data_len, options_ptr, std::ptr::null(), 0);

            assert_eq!(result.error, CIndicatorError::Ok);
            let state = result.state;

            // Second batch with more data
            let extra_data_len = 10;
            let real_extra: Vec<f64> = build_synthetic_data(extra_data_len, 0);

            let inputs_ptr_extra: [*const f64; INPUTS] = [real_extra.as_ptr()];
            let inputs_extra = inputs_ptr_extra.as_ptr();
            let batch_result = tsf_batch(state, inputs_extra, extra_data_len, std::ptr::null(), 0);

            assert_eq!(batch_result.error, CIndicatorError::Ok);
            assert_eq!(batch_result.num_outputs, 1);

            tulip_ffi_batch_result_free(batch_result);
            tsf_state_free(state);
        }
    }

    #[test]
    fn test_tsf_simd_by_assets() {
        unsafe {
            let data_len = 20;
            let real: Vec<f64> = build_synthetic_data(data_len, 0);
            let real2: Vec<f64> = build_synthetic_data(data_len, 1);

            // Two different "assets"
            let assets_array: [*const f64; INPUTS] = [real.as_ptr()];
            let assets_array2: [*const f64; INPUTS] = [real2.as_ptr()];

            // Create a proper nested array of asset pointers
            let assets_ptrs_arr: [*const *const f64; 2] =
                [assets_array.as_ptr(), assets_array2.as_ptr()];
            let assets_ptrs = assets_ptrs_arr.as_ptr();
            let options: [f64; OPTIONS] = [10.0];
            let options_ptr = options.as_ptr();

            let result =
                tsf_simd_by_assets(assets_ptrs, 2, data_len, options_ptr, std::ptr::null(), 0);

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, 2);
            assert_eq!(result.num_outputs, 1);

            tulip_ffi_simd_result_free(result);
        }
    }

    #[test]
    fn test_tsf_simd_by_options() {
        unsafe {
            // Must satisfy min_data for *all* option sets used below.
            let data_len = 40;
            let real: Vec<f64> = build_synthetic_data(data_len, 0);

            // Create a proper array of input pointers
            let inputs_ptr: [*const f64; INPUTS] = [real.as_ptr()];
            let inputs = inputs_ptr.as_ptr();

            // Two different option sets
            let options1: [f64; OPTIONS] = [10.0];
            let options2: [f64; OPTIONS] = [20.0];
            let options_arr: [*const f64; 2] = [options1.as_ptr(), options2.as_ptr()];
            let options = options_arr.as_ptr();

            let result = tsf_simd_by_options(inputs, data_len, options, 2, std::ptr::null(), 0);

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, 2);
            assert_eq!(result.num_outputs, 1);

            for i in 0..2 {
                tsf_state_free(*result.states.add(i));
            }
            tulip_ffi_simd_result_free(result);
        }
    }
}
