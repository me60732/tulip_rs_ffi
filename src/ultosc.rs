//! extern "C" wrapper for `ultosc`, mirroring the core `tulip_rs` crate's
//! `Ultosc::indicator` / `IndicatorState::batch_indicator` interface with a
//! Tulip-Indicators-style C calling convention: `inputs` is an array of
//! `INPUTS` pointers (one per input series, each `data_len` long, in the same
//! order as `tulip_rs::indicators::ultosc::Ultosc::INFO.inputs`), `options`
//! is a flat array of `OPTIONS` values -- exactly like `ti_ultosc`'s
//! `double const *const *inputs, double const *options`. Output allocation
//! stays inside the indicator function (never caller-supplied).
//!
//! Parameter order convention (kept consistent across every function in
//! this crate): each pointer parameter is immediately followed by the
//! count(s) that describe it, e.g. `inputs, data_len, options, ...,
//! optional_outputs, numoptional`.
//!
//! SIMD entry points:
//! - `ultosc_simd_by_assets`: compute ULTOSC for N assets simultaneously,
//!   sharing a single options array. N must be 2, 4, 8, or 16.
//! - `ultosc_simd_by_options`: compute ULTOSC for N option sets simultaneously,
//!   using the same inputs. N must be 2, 4, 8, or 16.
//!
//! Note: `ultosc` supports both SIMD by-assets and SIMD by-options.

use std::os::raw::c_void;

use tulip_rs::indicator_types::{Indicator, IndicatorByOptions, TIndicatorState};
use tulip_rs::indicators::ultosc::{IndicatorState as UltoscState, Ultosc, INPUTS, OPTIONS};
use tulip_rs::types::IndicatorError;

use crate::common::{
    optional_outputs_slice, pack_info, pack_outputs, pack_simd_outputs, pack_states, read_inputs,
    read_simd_assets_inputs, read_simd_options, CBatchResult, CIndicatorError, CIndicatorInfo,
    CIndicatorResult, CSimdResult,
};

/// Opaque state handle returned by `ultosc_indicator()` and consumed by
/// `ultosc_batch()` / `ultosc_state_free()`.
pub type UltoscStateHandle = UltoscState;

/// Returns static metadata about the `ultosc` indicator: its name, input
/// names, option names, and (mandatory/optional) output names, mirroring
/// `Ultosc::INFO`.
///
/// The returned strings are leaked, process-lifetime C strings -- read them,
/// don't free them.
#[no_mangle]
pub extern "C" fn ultosc_info() -> CIndicatorInfo {
    pack_info(&Ultosc::INFO)
}

/// Returns the minimum number of bars `ultosc` needs to produce any output at
/// all, given `options`.
#[no_mangle]
pub unsafe extern "C" fn ultosc_min_data(options: *const f64) -> usize {
    let options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    Ultosc::min_data(&options)
}

/// Runs `ultosc` over `data_len` bars.
///
/// `inputs` must point to `INPUTS` (3) pointers, in order:
/// `high, low, close`, each `data_len` `f64`s long.
/// `options` must point to `OPTIONS` (3) values: `short_period, medium_period, long_period`.
///
/// Returns the mandatory `ultosc` output plus any requested optional outputs
/// (`tr`, `bp`) and a fresh continuation state.
///
/// # Safety
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s (pass null + 0 to request no optional outputs).
#[no_mangle]
pub unsafe extern "C" fn ultosc_indicator(
    inputs: *const *const f64,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CIndicatorResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match Ultosc::indicator(&inputs, &options, optional) {
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

/// Continues a `ultosc` calculation from `state`, mutating it in place so
/// it's ready for the next call. `state` must have come from
/// `ultosc_indicator()` and not yet have been passed to `ultosc_state_free()`.
///
/// `inputs` must point to `INPUTS` (3) pointers (`high, low, close`),
/// each `data_len` `f64`s long.
///
/// # Safety
/// - `state` must be a live pointer previously returned by
///   `ultosc_indicator()`.
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn ultosc_batch(
    state: *mut c_void,
    inputs: *const *const f64,
    data_len: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CBatchResult {
    if state.is_null() {
        return CBatchResult::err(IndicatorError::InvalidIndicatorState);
    }
    let state = &mut *(state as *mut UltoscStateHandle);

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

/// Frees a state handle returned by `ultosc_indicator()` (or one of the
/// `states[i]` entries from `ultosc_simd_by_assets()`/`ultosc_simd_by_options()`).
/// Call this once you're done streaming (after your last `ultosc_batch()` call).
///
/// # Safety
/// `state` must be a live pointer previously returned by `ultosc_indicator()`
/// or found in a `CSimdResult::states` array, and must not be used again
/// after this call.
#[no_mangle]
pub unsafe extern "C" fn ultosc_state_free(state: *mut c_void) {
    if !state.is_null() {
        drop(Box::from_raw(state as *mut UltoscStateHandle));
    }
}

/// Computes ULTOSC for `N` assets simultaneously (SIMD), sharing a single
/// options array. `num_assets` must be 2, 4, 8, or 16 -- anything else
/// returns `InvalidInputs`.
///
/// `inputs` must point to `num_assets` pointers, each itself pointing to
/// `INPUTS` (3) pointers (`high, low, close`), each `data_len` `f64`s long.
/// `options` must point to `OPTIONS` (3) shared values.
///
/// Returns a `CSimdResult` with `num_assets` results, each with its own
/// output rows and its own ordinary `ultosc` continuation state (reusable
/// with `ultosc_batch()`/`ultosc_state_free()`). Free each state via
/// `ultosc_state_free()`, then free the rest via `tulip_ffi_simd_result_free()`.
///
/// # Safety
/// - `inputs` must point to `num_assets` valid pointers, each pointing to
///   `INPUTS` valid non-null `*const f64`s, each pointing to `data_len` valid
///   `f64`s.
#[no_mangle]
pub unsafe extern "C" fn ultosc_simd_by_assets(
    inputs: *const *const *const f64,
    num_assets: usize,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    match num_assets {
        2 => ultosc_simd_by_assets_n::<2>(inputs, data_len, options, optional_outputs, numoptional),
        4 => ultosc_simd_by_assets_n::<4>(inputs, data_len, options, optional_outputs, numoptional),
        8 => ultosc_simd_by_assets_n::<8>(inputs, data_len, options, optional_outputs, numoptional),
        16 => {
            ultosc_simd_by_assets_n::<16>(inputs, data_len, options, optional_outputs, numoptional)
        }
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn ultosc_simd_by_assets_n<const N: usize>(
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

    match Ultosc::indicator_by_assets::<N>(&refs, &options, optional) {
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

/// Computes ULTOSC for `N` option sets simultaneously (SIMD), using the same
/// inputs. `num_option_sets` must be 2, 4, 8, or 16 -- anything else
/// returns `InvalidInputs`.
///
/// `inputs` must point to `INPUTS` (3) pointers (`high, low, close`), each `data_len` `f64`s long.
/// `options` must point to `num_option_sets` pointers, each itself pointing
/// to `OPTIONS` (3) values.
///
/// Returns a `CSimdResult` with `num_option_sets` results, each with its own
/// output rows and its own ordinary `ultosc` continuation state (reusable
/// with `ultosc_batch()`/`ultosc_state_free()`). Free each state via
/// `ultosc_state_free()`, then free the rest via `tulip_ffi_simd_result_free()`.
///
/// # Safety
/// - `inputs` must point to `INPUTS` valid non-null `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `options` must point to `num_option_sets` valid pointers, each itself
///   pointing to `OPTIONS` valid `f64`s.
#[no_mangle]
pub unsafe extern "C" fn ultosc_simd_by_options(
    inputs: *const *const f64,
    data_len: usize,
    options: *const *const f64,
    num_option_sets: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    match num_option_sets {
        2 => {
            ultosc_simd_by_options_n::<2>(inputs, data_len, options, optional_outputs, numoptional)
        }
        4 => {
            ultosc_simd_by_options_n::<4>(inputs, data_len, options, optional_outputs, numoptional)
        }
        8 => {
            ultosc_simd_by_options_n::<8>(inputs, data_len, options, optional_outputs, numoptional)
        }
        16 => {
            ultosc_simd_by_options_n::<16>(inputs, data_len, options, optional_outputs, numoptional)
        }
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn ultosc_simd_by_options_n<const N: usize>(
    inputs: *const *const f64,
    data_len: usize,
    options: *const *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let options = read_simd_options::<N, OPTIONS>(options);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match Ultosc::indicator_by_options::<N>(&inputs, &options, optional) {
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
    use crate::common::{
        tulip_ffi_batch_result_free, tulip_ffi_result_free, tulip_ffi_simd_result_free,
    };

    #[test]
    fn test_ultosc_info() {
        let info = ultosc_info();
        assert!(info.inputs.len > 0);
        assert_eq!(info.options.len, 3);
        assert!(info.outputs.len > 0);
        assert_eq!(info.optional_outputs.len, 0);
    }

    #[test]
    fn test_ultosc_min_data() {
        unsafe {
            let options: [f64; OPTIONS] = [7.0, 14.0, 28.0];
            let min = ultosc_min_data(options.as_ptr());
            assert!(min > 0);
        }
    }

    #[test]
    fn test_ultosc_indicator() {
        unsafe {
            let data_len = 30;
            let high: Vec<f64> = (1..=data_len).map(|i| i as f64 + 1.0).collect();
            let low: Vec<f64> = (1..=data_len).map(|i| i as f64 - 1.0).collect();
            let close: Vec<f64> = (1..=data_len).map(|i| i as f64).collect();

            // Create a proper array of input pointers
            let inputs_ptr: [*const f64; INPUTS] = [high.as_ptr(), low.as_ptr(), close.as_ptr()];
            let inputs = inputs_ptr.as_ptr();
            let options: [f64; OPTIONS] = [7.0, 14.0, 28.0];
            let options_ptr = options.as_ptr();
            let result = ultosc_indicator(inputs, data_len, options_ptr, std::ptr::null(), 0);

            assert_eq!(result.error, CIndicatorError::Ok);
            // core returns ultosc/tr/bp rows, but tr/bp are only written when
            // enabled internally; pack_outputs(None) filters the empty rows.
            assert_eq!(result.num_outputs, 1);
            tulip_ffi_result_free(result);
        }
    }

    #[test]
    fn test_ultosc_batch() {
        unsafe {
            let data_len = 30;
            let high: Vec<f64> = (1..=data_len).map(|i| i as f64 + 1.0).collect();
            let low: Vec<f64> = (1..=data_len).map(|i| i as f64 - 1.0).collect();
            let close: Vec<f64> = (1..=data_len).map(|i| i as f64).collect();

            // Create a proper array of input pointers
            let inputs_ptr: [*const f64; INPUTS] = [high.as_ptr(), low.as_ptr(), close.as_ptr()];
            let inputs = inputs_ptr.as_ptr();
            let options: [f64; OPTIONS] = [7.0, 14.0, 28.0];
            let options_ptr = options.as_ptr();
            let result = ultosc_indicator(inputs, data_len, options_ptr, std::ptr::null(), 0);

            assert_eq!(result.error, CIndicatorError::Ok);
            let state = result.state;

            // Second batch with more data
            let extra_data_len = 10;
            let high_extra: Vec<f64> = (31..=40).map(|i| i as f64 + 1.0).collect();
            let low_extra: Vec<f64> = (31..=40).map(|i| i as f64 - 1.0).collect();
            let close_extra: Vec<f64> = (31..=40).map(|i| i as f64).collect();

            let inputs_ptr_extra: [*const f64; INPUTS] = [
                high_extra.as_ptr(),
                low_extra.as_ptr(),
                close_extra.as_ptr(),
            ];
            let inputs_extra = inputs_ptr_extra.as_ptr();
            let batch_result =
                ultosc_batch(state, inputs_extra, extra_data_len, std::ptr::null(), 0);

            assert_eq!(batch_result.error, CIndicatorError::Ok);
            assert_eq!(batch_result.num_outputs, 1);

            tulip_ffi_batch_result_free(batch_result);
            ultosc_state_free(state);
        }
    }

    #[test]
    fn test_ultosc_simd_by_assets() {
        unsafe {
            let data_len = 30;
            let high: Vec<f64> = (1..=data_len).map(|i| i as f64 + 1.0).collect();
            let low: Vec<f64> = (1..=data_len).map(|i| i as f64 - 1.0).collect();
            let close: Vec<f64> = (1..=data_len).map(|i| i as f64).collect();

            // Two different "assets"
            let high2: Vec<f64> = (31..=60).map(|i| i as f64 + 1.0).collect();
            let low2: Vec<f64> = (31..=60).map(|i| i as f64 - 1.0).collect();
            let close2: Vec<f64> = (31..=60).map(|i| i as f64).collect();

            // Create a proper nested array of asset pointers
            // For SIMD by assets with INPUTS=3:
            // - Each asset has 3 input pointers: [high, low, close]
            // - The inputs pointer is to an array of N asset arrays
            let assets_array: [*const f64; INPUTS] = [high.as_ptr(), low.as_ptr(), close.as_ptr()];
            let assets_array2: [*const f64; INPUTS] =
                [high2.as_ptr(), low2.as_ptr(), close2.as_ptr()];

            // Cast to the expected pointer type: an array of 2 pointers, each
            // pointing to one asset's array of INPUTS data pointers.
            let assets_ptrs_arr: [*const *const f64; 2] =
                [assets_array.as_ptr(), assets_array2.as_ptr()];
            let assets_ptrs = assets_ptrs_arr.as_ptr();
            let options: [f64; OPTIONS] = [7.0, 14.0, 28.0];
            let options_ptr = options.as_ptr();

            let result =
                ultosc_simd_by_assets(assets_ptrs, 2, data_len, options_ptr, std::ptr::null(), 0);

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, 2);
            assert_eq!(result.num_outputs, 1);

            tulip_ffi_simd_result_free(result);
        }
    }

    #[test]
    fn test_ultosc_simd_by_options() {
        unsafe {
            let data_len = 30;
            let high: Vec<f64> = (1..=data_len).map(|i| i as f64 + 1.0).collect();
            let low: Vec<f64> = (1..=data_len).map(|i| i as f64 - 1.0).collect();
            let close: Vec<f64> = (1..=data_len).map(|i| i as f64).collect();

            // Create a proper array of input pointers
            let inputs_ptr: [*const f64; INPUTS] = [high.as_ptr(), low.as_ptr(), close.as_ptr()];
            let inputs = inputs_ptr.as_ptr();

            // Two different option sets
            let options1: [f64; OPTIONS] = [7.0, 14.0, 28.0];
            let options2: [f64; OPTIONS] = [5.0, 10.0, 20.0];
            let options_arr: [*const f64; 2] = [options1.as_ptr(), options2.as_ptr()];
            let options_ptr = options_arr.as_ptr();

            let result =
                ultosc_simd_by_options(inputs, data_len, options_ptr, 2, std::ptr::null(), 0);

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, 2);
            assert_eq!(result.num_outputs, 1);

            tulip_ffi_simd_result_free(result);
        }
    }
}
