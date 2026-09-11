//! extern "C" wrapper for `adxr`, mirroring the core `tulip_rs` crate's
//! `Adxr::indicator` / `IndicatorState::batch_indicator` interface with a
//! Tulip-Indicators-style C calling convention: `inputs` is an array of
//! `INPUTS` pointers (one per input series, each `data_len` long, in the same
//! order as `tulip_rs::indicators::adxr::Adxr::INFO.inputs`), `options`
//! is a flat array of `OPTIONS` values -- exactly like `ti_adxr`'s
//! `double const *const *inputs, double const *options`. Output allocation
//! stays inside the indicator function (never caller-supplied).
//!
//! Parameter order convention (kept consistent across every function in
//! this crate): each pointer parameter is immediately followed by the
//! count(s) that describe it, e.g. `inputs, data_len, options, ...,
//! optional_outputs, numoptional`.
//!
//! SIMD entry points:
//! - `adxr_simd_by_assets`: compute ADXR for N assets simultaneously,
//!   sharing a single options array. N must be 2, 4, 8, or 16.
//! - `adxr_simd_by_options`: compute ADXR for one asset with N different
//!   option sets simultaneously. N must be 2, 4, 8, or 16.
//!
//! `#[no_mangle]` functions can't be generic, so each SIMD entry point is a
//! thin non-generic dispatcher that matches the runtime `N` against
//! {2,4,8,16} and calls a private generic `_n::<N>` helper that does the
//! real work.

use std::os::raw::c_void;

use tulip_rs::indicator_types::{Indicator, IndicatorByOptions, TIndicatorState};
use tulip_rs::indicators::adxr::{Adxr, IndicatorState as AdxrState, INPUTS, OPTIONS};
use tulip_rs::types::IndicatorError;

use crate::common::{
    optional_outputs_slice, pack_info, pack_outputs, pack_simd_outputs, pack_states, read_inputs,
    read_simd_assets_inputs, read_simd_options, CBatchResult, CIndicatorError, CIndicatorInfo,
    CIndicatorResult, CSimdResult,
};

/// Opaque state handle returned by `adxr_indicator()` and consumed by
/// `adxr_batch()` / `adxr_state_free()`.
pub type AdxrStateHandle = AdxrState;

/// Returns static metadata about the `adxr` indicator: its name, input
/// names, option names, and (mandatory/optional) output names, mirroring
/// `Adxr::INFO`.
///
/// The returned strings are leaked, process-lifetime C strings -- read them,
/// don't free them.
#[no_mangle]
pub extern "C" fn adxr_info() -> CIndicatorInfo {
    pack_info(&Adxr::INFO)
}

/// Returns the minimum number of bars `adxr` needs to produce any output at
/// all, given `options`.
///
/// # Safety
/// `options` must point to `OPTIONS` (1) valid `f64`s.
#[no_mangle]
pub unsafe extern "C" fn adxr_min_data(options: *const f64) -> usize {
    let options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    Adxr::min_data(&options)
}

/// Runs `adxr` over `data_len` bars.
///
/// `inputs` must point to `INPUTS` (3) pointers, in order:
/// `high, low, close`, each `data_len` `f64`s long.
/// `options` must point to `OPTIONS` (1) values: `period`.
///
/// Returns the mandatory `adxr` output plus any requested optional outputs
/// (`adx`, `dx`, `atr`, `tr`, in that fixed order) and a fresh continuation state.
///
/// # Safety
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s (pass null + 0 to request no optional outputs).
#[no_mangle]
pub unsafe extern "C" fn adxr_indicator(
    inputs: *const *const f64,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CIndicatorResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match Adxr::indicator(&inputs, &options, optional) {
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

/// Continues an `adxr` calculation from `state`, mutating it in place so
/// it's ready for the next call. `state` must have come from
/// `adxr_indicator()` and not yet have been passed to `adxr_state_free()`.
///
/// `inputs` must point to `INPUTS` (3) pointers (`high, low, close`),
/// each `data_len` `f64`s long.
///
/// # Safety
/// - `state` must be a live pointer previously returned by
///   `adxr_indicator()`.
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn adxr_batch(
    state: *mut c_void,
    inputs: *const *const f64,
    data_len: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CBatchResult {
    if state.is_null() {
        return CBatchResult::err(IndicatorError::InvalidIndicatorState);
    }
    let state = &mut *(state as *mut AdxrStateHandle);

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

/// Frees a state handle returned by `adxr_indicator()` (or one of the
/// `states[i]` entries from `adxr_simd_by_assets()`/`adxr_simd_by_options()`).
/// Call this once you're done streaming (after your last `adxr_batch()` call).
///
/// # Safety
/// `state` must be a live pointer previously returned by `adxr_indicator()`
/// or found in a `CSimdResult::states` array, and must not be used again
/// after this call.
#[no_mangle]
pub unsafe extern "C" fn adxr_state_free(state: *mut c_void) {
    if !state.is_null() {
        drop(Box::from_raw(state as *mut AdxrStateHandle));
    }
}

/// Computes ADXR for `N` assets simultaneously (SIMD), sharing a single
/// options array. `num_assets` must be 2, 4, 8, or 16 -- anything else
/// returns `InvalidInputs`.
///
/// `inputs` must point to `num_assets` pointers, each itself pointing to
/// `INPUTS` (3) pointers (`high, low, close`), each `data_len` `f64`s
/// long. `options` must point to `OPTIONS` (1) shared values.
///
/// Returns a `CSimdResult` with `num_assets` results, each with its own
/// output rows and its own ordinary `adxr` continuation state (reusable
/// with `adxr_batch()`/`adxr_state_free()`). Free each state via
/// `adxr_state_free()`, then free the rest via `tulip_ffi_simd_result_free()`.
///
/// # Safety
/// - `inputs` must point to `num_assets` valid pointers, each pointing to
///   `INPUTS` valid non-null `*const f64`s, each pointing to `data_len` valid
///   `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn adxr_simd_by_assets(
    inputs: *const *const *const f64,
    num_assets: usize,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    match num_assets {
        2 => adxr_simd_by_assets_n::<2>(inputs, data_len, options, optional_outputs, numoptional),
        4 => adxr_simd_by_assets_n::<4>(inputs, data_len, options, optional_outputs, numoptional),
        8 => adxr_simd_by_assets_n::<8>(inputs, data_len, options, optional_outputs, numoptional),
        16 => adxr_simd_by_assets_n::<16>(inputs, data_len, options, optional_outputs, numoptional),
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn adxr_simd_by_assets_n<const N: usize>(
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

    match Adxr::indicator_by_assets::<N>(&refs, &options, optional) {
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

/// Computes ADXR for one asset with `N` different option sets
/// simultaneously (SIMD). `num_option_sets` must be 2, 4, 8, or 16 --
/// anything else returns `InvalidInputs`.
///
/// `inputs` must point to `INPUTS` (3) pointers (`high, low, close`),
/// each `data_len` `f64`s long, shared across all option sets.
/// `options` must point to `num_option_sets` pointers, each itself
/// pointing to `OPTIONS` (1) values.
///
/// Returns a `CSimdResult` with `num_option_sets` results, each with its
/// own output rows and its own ordinary `adxr` continuation state
/// (reusable with `adxr_batch()`/`adxr_state_free()`). Free each state
/// via `adxr_state_free()`, then free the rest via
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
pub unsafe extern "C" fn adxr_simd_by_options(
    inputs: *const *const f64,
    data_len: usize,
    options: *const *const f64,
    num_option_sets: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    match num_option_sets {
        2 => adxr_simd_by_options_n::<2>(inputs, data_len, options, optional_outputs, numoptional),
        4 => adxr_simd_by_options_n::<4>(inputs, data_len, options, optional_outputs, numoptional),
        8 => adxr_simd_by_options_n::<8>(inputs, data_len, options, optional_outputs, numoptional),
        16 => {
            adxr_simd_by_options_n::<16>(inputs, data_len, options, optional_outputs, numoptional)
        }
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn adxr_simd_by_options_n<const N: usize>(
    inputs: *const *const f64,
    data_len: usize,
    options: *const *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let options = read_simd_options::<N, OPTIONS>(options);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match Adxr::indicator_by_options::<N>(&inputs, &options, optional) {
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
    fn test_adxr_info() {
        let info = adxr_info();
        assert!(info.inputs.len > 0);
        assert_eq!(info.options.len, 1);
        assert!(info.outputs.len > 0);
        assert_eq!(info.optional_outputs.len, 4);
    }

    #[test]
    fn test_adxr_min_data() {
        unsafe {
            let options: [f64; OPTIONS] = [14.0];
            let min = adxr_min_data(options.as_ptr());
            assert!(min > 0);
        }
    }

    #[test]
    fn test_adxr_indicator() {
        unsafe {
            let data_len = 60;
            let period = 14;
            let high: Vec<f64> = build_synthetic_data(data_len, 2);
            let low: Vec<f64> = build_synthetic_data(data_len, 0);
            let close: Vec<f64> = build_synthetic_data(data_len, 1);

            let inputs_ptr: [*const f64; INPUTS] = [high.as_ptr(), low.as_ptr(), close.as_ptr()];
            let inputs = inputs_ptr.as_ptr();
            let options_arr: [f64; OPTIONS] = [period as f64];
            let options = options_arr.as_ptr();

            // Request all optional outputs
            let optional_outputs = [true, true, true, true]; // adx, dx, atr, tr
            let result = adxr_indicator(inputs, data_len, options, optional_outputs.as_ptr(), 4);

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_outputs, 5); // adxr, adx, dx, atr, tr

            let _outputs_slice = slice::from_raw_parts(result.outputs, result.num_outputs);
            let output_lens_slice = slice::from_raw_parts(result.output_lens, result.num_outputs);

            // adxr_line: min_data=(period-1)*3+1, output_length=data_len-min_data = 60-40 = 20
            // adx_line and dx_line: use Adx::output_length which is data_len - period*2 + 1 = 33
            // atr_line: same as dx_line (Dx::output_length)
            // tr_line: uses Tr::output_length = data_len - 1 = 59
            let adxr_min_data = (period - 1) * 3 + 1;
            assert_eq!(output_lens_slice[0], data_len - adxr_min_data + 1);
            assert_eq!(output_lens_slice[1], data_len - period * 2 + 1);
            let dx_min_data = 14 + 1; // default min_data for single-period indicators
            assert_eq!(output_lens_slice[2], data_len - dx_min_data + 1);

            assert_eq!(output_lens_slice[3], data_len - dx_min_data + 1); // atr same as dx
            assert_eq!(output_lens_slice[4], data_len - 1); // tr uses Tr::output_length
            free_result(result);
        }
    }

    #[test]
    fn test_adxr_batch() {
        unsafe {
            let data_len = 60;
            let period = 14;
            let high: Vec<f64> = build_synthetic_data(data_len, 2);
            let low: Vec<f64> = build_synthetic_data(data_len, 0);
            let close: Vec<f64> = build_synthetic_data(data_len, 1);

            let inputs_ptr: [*const f64; INPUTS] = [high.as_ptr(), low.as_ptr(), close.as_ptr()];
            let inputs = inputs_ptr.as_ptr();
            let options_arr: [f64; OPTIONS] = [period as f64];
            let options = options_arr.as_ptr();

            // Request all optional outputs
            let optional_outputs = [true, true, true];
            let result = adxr_indicator(inputs, data_len, options, optional_outputs.as_ptr(), 3);

            assert_eq!(result.error, CIndicatorError::Ok);
            // With all optional outputs requested: all rows returned (adxr, adx, dx, atr, tr)
            assert_eq!(result.num_outputs, 5);
            let state = result.state;

            // Second batch with more data
            let extra_data_len = 10;
            let high_extra: Vec<f64> = build_synthetic_data(extra_data_len, 2);
            let low_extra: Vec<f64> = build_synthetic_data(extra_data_len, 0);
            let close_extra: Vec<f64> = build_synthetic_data(extra_data_len, 1);

            let inputs_ptr_extra: [*const f64; INPUTS] = [
                high_extra.as_ptr(),
                low_extra.as_ptr(),
                close_extra.as_ptr(),
            ];
            let inputs_extra = inputs_ptr_extra.as_ptr();
            let batch_result = adxr_batch(state, inputs_extra, extra_data_len, std::ptr::null(), 0);

            assert_eq!(batch_result.error, CIndicatorError::Ok);
            // Without optional outputs: only mandatory rows returned (adxr)
            assert_eq!(batch_result.num_outputs, 1);

            free_batch_result(batch_result);
            adxr_state_free(state);
        }
    }

    #[test]
    fn test_adxr_simd_by_assets() {
        unsafe {
            let data_len = 60;
            let period = 14;
            let high: Vec<f64> = build_synthetic_data(data_len, 2);
            let low: Vec<f64> = build_synthetic_data(data_len, 0);
            let close: Vec<f64> = build_synthetic_data(data_len, 1);

            // Two identical "assets"
            let inputs_ptr_0: [*const f64; INPUTS] = [high.as_ptr(), low.as_ptr(), close.as_ptr()];
            let inputs_ptr_1: [*const f64; INPUTS] = [high.as_ptr(), low.as_ptr(), close.as_ptr()];
            let assets_arr: [*const *const f64; 2] = [inputs_ptr_0.as_ptr(), inputs_ptr_1.as_ptr()];
            let assets_ptrs = assets_arr.as_ptr();
            let options_arr: [f64; OPTIONS] = [period as f64];
            let options = options_arr.as_ptr();

            let result =
                adxr_simd_by_assets(assets_ptrs, 2, data_len, options, std::ptr::null(), 0);

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, 2);
            // Without optional outputs: only mandatory rows returned (adxr)
            assert_eq!(result.num_outputs, 1);

            let states_slice = slice::from_raw_parts(result.states, result.num_results);
            for i in 0..result.num_results {
                adxr_state_free(states_slice[i]);
            }
            free_simd_result(result);
        }
    }

    #[test]
    fn test_adxr_simd_by_options() {
        unsafe {
            let data_len = 60;
            let high: Vec<f64> = build_synthetic_data(data_len, 2);
            let low: Vec<f64> = build_synthetic_data(data_len, 0);
            let close: Vec<f64> = build_synthetic_data(data_len, 1);

            let inputs_ptr: [*const f64; INPUTS] = [high.as_ptr(), low.as_ptr(), close.as_ptr()];
            let inputs = inputs_ptr.as_ptr();
            // Two option sets with different periods
            let options_arr_0: [f64; OPTIONS] = [14.0];
            let options_arr_1: [f64; OPTIONS] = [20.0];
            let options_ptrs_arr: [*const f64; 2] =
                [options_arr_0.as_ptr(), options_arr_1.as_ptr()];
            let options_ptrs = options_ptrs_arr.as_ptr();

            let result =
                adxr_simd_by_options(inputs, data_len, options_ptrs, 2, std::ptr::null(), 0);

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, 2);
            // Without optional outputs: only mandatory rows returned (adxr)
            assert_eq!(result.num_outputs, 1);

            let states_slice = slice::from_raw_parts(result.states, result.num_results);
            for i in 0..result.num_results {
                adxr_state_free(states_slice[i]);
            }
            free_simd_result(result);
        }
    }
}
