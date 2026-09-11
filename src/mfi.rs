//! extern "C" wrapper for `mfi`, mirroring the core `tulip_rs` crate's
//! `Mfi::indicator` / `IndicatorState::batch_indicator` interface with a
//! Tulip-Indicators-style C calling convention: `inputs` is an array of
//! `INPUTS` pointers (one per input series, each `data_len` long, in the same
//! order as `tulip_rs::indicators::mfi::Mfi::INFO.inputs`), `options`
//! is a flat array of `OPTIONS` values -- exactly like `ti_mfi`'s
//! `double const *const *inputs, double const *options`. Output allocation
//! stays inside the indicator function (never caller-supplied).
//!
//! Parameter order convention (kept consistent across every function in
//! this crate): each pointer parameter is immediately followed by the
//! count(s) that describe it, e.g. `inputs, data_len, options, ...,
//! optional_outputs, numoptional`.
//!
//! SIMD entry points:
//! - `mfi_simd_by_assets`: compute MFI for N assets simultaneously,
//!   sharing a single options array. N must be 2, 4, 8, or 16.
//! - `mfi_simd_by_options`: compute MFI for one asset with N different
//!   option sets simultaneously. N must be 2, 4, 8, or 16.
//!
//! `#[no_mangle]` functions can't be generic, so each SIMD entry point is a
//! thin non-generic dispatcher that matches the runtime `N` against
//! {2,4,8,16} and calls a private generic `_n::<N>` helper that does the
//! real work.

use std::os::raw::c_void;

use tulip_rs::indicator_types::{Indicator, IndicatorByOptions, TIndicatorState};
use tulip_rs::indicators::mfi::{IndicatorState as MfiState, Mfi, INPUTS, OPTIONS};
use tulip_rs::types::IndicatorError;

use crate::common::{
    optional_outputs_slice, pack_info, pack_outputs, pack_simd_outputs, pack_states, read_inputs,
    read_simd_assets_inputs, read_simd_options, CBatchResult, CIndicatorError, CIndicatorInfo,
    CIndicatorResult, CSimdResult,
};

/// Opaque state handle returned by `mfi_indicator()` and consumed by
/// `mfi_batch()` / `mfi_state_free()`.
pub type MfiStateHandle = MfiState;

/// Returns static metadata about the `mfi` indicator: its name, input
/// names, option names, and (mandatory/optional) output names, mirroring
/// `Mfi::INFO`.
///
/// The returned strings are leaked, process-lifetime C strings -- read them,
/// don't free them.
#[no_mangle]
pub extern "C" fn mfi_info() -> CIndicatorInfo {
    pack_info(&Mfi::INFO)
}

/// Returns the minimum number of bars `mfi` needs to produce any output at
/// all, given `options`.
///
/// # Safety
/// `options` must point to `OPTIONS` (1) valid `f64`s.
#[no_mangle]
pub unsafe extern "C" fn mfi_min_data(options: *const f64) -> usize {
    let options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    Mfi::min_data(&options)
}

/// Runs `mfi` over `data_len` bars.
///
/// `inputs` must point to `INPUTS` (4) pointers, in order:
/// `high, low, close, volume`, each `data_len` `f64`s long.
/// `options` must point to `OPTIONS` (1) value: `period`.
///
/// Returns the mandatory `mfi` output plus any requested optional outputs
/// (`typprice`) and a fresh continuation state.
///
/// # Safety
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s (pass null + 0 to request no optional outputs).
#[no_mangle]
pub unsafe extern "C" fn mfi_indicator(
    inputs: *const *const f64,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CIndicatorResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match Mfi::indicator(&inputs, &options, optional) {
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

/// Continues an `mfi` calculation from `state`, mutating it in place so
/// it's ready for the next call. `state` must have come from
/// `mfi_indicator()` and not yet have been passed to `mfi_state_free()`.
///
/// `inputs` must point to `INPUTS` (4) pointers (`high, low, close,
/// volume`), each `data_len` `f64`s long.
///
/// # Safety
/// - `state` must be a live pointer previously returned by
///   `mfi_indicator()`.
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn mfi_batch(
    state: *mut c_void,
    inputs: *const *const f64,
    data_len: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CBatchResult {
    if state.is_null() {
        return CBatchResult::err(IndicatorError::InvalidIndicatorState);
    }
    let state = &mut *(state as *mut MfiStateHandle);

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

/// Frees a state handle returned by `mfi_indicator()` (or one of the
/// `states[i]` entries from `mfi_simd_by_assets()`/`mfi_simd_by_options()`).
/// Call this once you're done streaming (after your last `mfi_batch()` call).
///
/// # Safety
/// `state` must be a live pointer previously returned by `mfi_indicator()`
/// or found in a `CSimdResult::states` array, and must not be used again
/// after this call.
#[no_mangle]
pub unsafe extern "C" fn mfi_state_free(state: *mut c_void) {
    if !state.is_null() {
        drop(Box::from_raw(state as *mut MfiStateHandle));
    }
}

/// Computes MFI for `N` assets simultaneously (SIMD), sharing a single
/// options array. `num_assets` must be 2, 4, 8, or 16 -- anything else
/// returns `InvalidInputs`.
///
/// `inputs` must point to `num_assets` pointers, each itself pointing to
/// `INPUTS` (4) pointers (`high, low, close, volume`), each `data_len` `f64`s
/// long. `options` must point to `OPTIONS` (1) shared value: `period`.
///
/// Returns a `CSimdResult` with `num_assets` results, each with its own
/// output rows and its own ordinary `mfi` continuation state (reusable
/// with `mfi_batch()`/`mfi_state_free()`). Free each state via
/// `mfi_state_free()`, then free the rest via `tulip_ffi_simd_result_free()`.
///
/// # Safety
/// - `inputs` must point to `num_assets` valid pointers, each pointing to
///   `INPUTS` valid non-null `*const f64`s, each pointing to `data_len` valid
///   `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn mfi_simd_by_assets(
    inputs: *const *const *const f64,
    num_assets: usize,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    match num_assets {
        2 => mfi_simd_by_assets_n::<2>(inputs, data_len, options, optional_outputs, numoptional),
        4 => mfi_simd_by_assets_n::<4>(inputs, data_len, options, optional_outputs, numoptional),
        8 => mfi_simd_by_assets_n::<8>(inputs, data_len, options, optional_outputs, numoptional),
        16 => mfi_simd_by_assets_n::<16>(inputs, data_len, options, optional_outputs, numoptional),
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn mfi_simd_by_assets_n<const N: usize>(
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

    match Mfi::indicator_by_assets::<N>(&refs, &options, optional) {
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

/// Computes MFI for one asset with `N` different option sets
/// simultaneously (SIMD). `num_option_sets` must be 2, 4, 8, or 16 --
/// anything else returns `InvalidInputs`.
///
/// `inputs` must point to `INPUTS` (4) pointers (`high, low, close,
/// volume`), each `data_len` `f64`s long, shared across all option sets.
/// `options` must point to `num_option_sets` pointers, each itself
/// pointing to `OPTIONS` (1) value: `period`.
///
/// Returns a `CSimdResult` with `num_option_sets` results, each with its
/// own output rows and its own ordinary `mfi` continuation state
/// (reusable with `mfi_batch()`/`mfi_state_free()`). Free each state
/// via `mfi_state_free()`, then free the rest via
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
pub unsafe extern "C" fn mfi_simd_by_options(
    inputs: *const *const f64,
    data_len: usize,
    options: *const *const f64,
    num_option_sets: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    match num_option_sets {
        2 => mfi_simd_by_options_n::<2>(inputs, data_len, options, optional_outputs, numoptional),
        4 => mfi_simd_by_options_n::<4>(inputs, data_len, options, optional_outputs, numoptional),
        8 => mfi_simd_by_options_n::<8>(inputs, data_len, options, optional_outputs, numoptional),
        16 => mfi_simd_by_options_n::<16>(inputs, data_len, options, optional_outputs, numoptional),
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn mfi_simd_by_options_n<const N: usize>(
    inputs: *const *const f64,
    data_len: usize,
    options: *const *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let options = read_simd_options::<N, OPTIONS>(options);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match Mfi::indicator_by_options::<N>(&inputs, &options, optional) {
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
    use crate::{tulip_ffi_batch_result_free, tulip_ffi_result_free, tulip_ffi_simd_result_free};
    use std::ptr;

    #[test]
    fn test_mfi_indicator() {
        unsafe {
            use crate::common::test::build_synthetic_data;

            let data_len = 20;
            let high_arr: Vec<f64> = build_synthetic_data(data_len).map(|x| x * 100.0).collect();
            let low_arr: Vec<f64> = build_synthetic_data(data_len)
                .map(|x| (x - 50.0) * 2.0)
                .collect();
            let close_arr: Vec<f64> = build_synthetic_data(data_len).map(|x| x * 100.0).collect();
            let volume_arr: Vec<f64> = build_synthetic_data(data_len).map(|x| x * 1000.0).collect();

            let inputs_arr: [*const f64; INPUTS] = [
                high_arr.as_ptr(),
                low_arr.as_ptr(),
                close_arr.as_ptr(),
                volume_arr.as_ptr(),
            ];
            let inputs: *const *const f64 = inputs_arr.as_ptr();
            let options_arr: [f64; OPTIONS] = [14.0];
            let options: *const f64 = options_arr.as_ptr();

            // Request optional output typprice
            let optional_arr: [bool; 1] = [true];
            let optional: *const bool = optional_arr.as_ptr();

            let result = mfi_indicator(inputs, data_len, options, optional, 1);

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_outputs, 2); // mfi + typprice
            assert!(!result.state.is_null());

            tulip_ffi_result_free(result);
        }
    }

    #[test]
    fn test_mfi_batch() {
        unsafe {
            use crate::common::test::build_synthetic_data;

            let data_len = 20;
            let high_arr: Vec<f64> = build_synthetic_data(data_len).map(|x| x * 100.0).collect();
            let low_arr: Vec<f64> = build_synthetic_data(data_len)
                .map(|x| (x - 50.0) * 2.0)
                .collect();
            let close_arr: Vec<f64> = build_synthetic_data(data_len).map(|x| x * 100.0).collect();
            let volume_arr: Vec<f64> = build_synthetic_data(data_len).map(|x| x * 1000.0).collect();

            let inputs_arr: [*const f64; INPUTS] = [
                high_arr.as_ptr(),
                low_arr.as_ptr(),
                close_arr.as_ptr(),
                volume_arr.as_ptr(),
            ];
            let inputs: *const *const f64 = inputs_arr.as_ptr();
            let options_arr: [f64; OPTIONS] = [14.0];
            let options: *const f64 = options_arr.as_ptr();

            let result = mfi_indicator(inputs, data_len, options, ptr::null(), 0);
            assert_eq!(result.error, CIndicatorError::Ok);

            // Second batch call with exactly 10 elements (batch length)
            let new_high_arr: Vec<f64> = build_synthetic_data(10).map(|x| x * 100.0).collect();
            let new_low_arr: Vec<f64> =
                build_synthetic_data(10).map(|x| (x - 50.0) * 2.0).collect();
            let new_close_arr: Vec<f64> = build_synthetic_data(10).map(|x| x * 100.0).collect();
            let new_volume_arr: Vec<f64> = build_synthetic_data(10).map(|x| x * 1000.0).collect();

            let new_inputs_arr: [*const f64; INPUTS] = [
                new_high_arr.as_ptr(),
                new_low_arr.as_ptr(),
                new_close_arr.as_ptr(),
                new_volume_arr.as_ptr(),
            ];
            let new_inputs: *const *const f64 = new_inputs_arr.as_ptr();

            let batch_result = mfi_batch(result.state, new_inputs, 10, ptr::null(), 0);
            assert_eq!(batch_result.error, CIndicatorError::Ok);

            tulip_ffi_batch_result_free(batch_result);
            mfi_state_free(result.state);
        }
    }

    #[test]
    fn test_mfi_simd_by_assets() {
        unsafe {
            use crate::common::test::build_synthetic_data;

            let data_len = 20;
            let num_assets = 2;

            // Create inputs for 2 assets
            let asset0_high: Vec<f64> = build_synthetic_data(data_len).map(|x| x * 100.0).collect();
            let asset0_low: Vec<f64> = build_synthetic_data(data_len)
                .map(|x| (x - 50.0) * 2.0)
                .collect();
            let asset0_close: Vec<f64> =
                build_synthetic_data(data_len).map(|x| x * 100.0).collect();
            let asset0_volume: Vec<f64> =
                build_synthetic_data(data_len).map(|x| x * 1000.0).collect();

            let asset1_high: Vec<f64> = build_synthetic_data(data_len).map(|x| x * 100.0).collect();
            let asset1_low: Vec<f64> = build_synthetic_data(data_len)
                .map(|x| (x - 50.0) * 2.0)
                .collect();
            let asset1_close: Vec<f64> =
                build_synthetic_data(data_len).map(|x| x * 100.0).collect();
            let asset1_volume: Vec<f64> =
                build_synthetic_data(data_len).map(|x| x * 1000.0).collect();

            let inputs_arr0: [*const f64; INPUTS] = [
                asset0_high.as_ptr(),
                asset0_low.as_ptr(),
                asset0_close.as_ptr(),
                asset0_volume.as_ptr(),
            ];
            let inputs_arr1: [*const f64; INPUTS] = [
                asset1_high.as_ptr(),
                asset1_low.as_ptr(),
                asset1_close.as_ptr(),
                asset1_volume.as_ptr(),
            ];

            let assets_inputs: [*const *const f64; 2] =
                [inputs_arr0.as_ptr(), inputs_arr1.as_ptr()];
            let inputs: *const *const *const f64 = assets_inputs.as_ptr();

            let options_arr: [f64; OPTIONS] = [14.0];
            let options: *const f64 = options_arr.as_ptr();

            let result = mfi_simd_by_assets(inputs, num_assets, data_len, options, ptr::null(), 0);

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, 2);
            assert_eq!(result.num_outputs, 1); // only mfi (mandatory)
            assert!(!result.states.is_null());

            tulip_ffi_simd_result_free(result);
        }
    }

    #[test]
    fn test_mfi_simd_by_options() {
        unsafe {
            use crate::common::test::build_synthetic_data;

            // Must satisfy min_data for *both* option sets used below.
            let data_len = 40;

            let high_arr: Vec<f64> = build_synthetic_data(data_len).map(|x| x * 100.0).collect();
            let low_arr: Vec<f64> = build_synthetic_data(data_len)
                .map(|x| (x - 50.0) * 2.0)
                .collect();
            let close_arr: Vec<f64> = build_synthetic_data(data_len).map(|x| x * 100.0).collect();
            let volume_arr: Vec<f64> = build_synthetic_data(data_len).map(|x| x * 1000.0).collect();

            let inputs_arr: [*const f64; INPUTS] = [
                high_arr.as_ptr(),
                low_arr.as_ptr(),
                close_arr.as_ptr(),
                volume_arr.as_ptr(),
            ];
            let inputs: *const *const f64 = inputs_arr.as_ptr();

            // Create 2 different option sets
            let options0: [f64; OPTIONS] = [14.0];
            let options1: [f64; OPTIONS] = [21.0];
            let options_arr: [*const f64; 2] = [options0.as_ptr(), options1.as_ptr()];
            let options: *const *const f64 = options_arr.as_ptr();

            let result = mfi_simd_by_options(inputs, data_len, options, 2, ptr::null(), 0);

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, 2);
            assert_eq!(result.num_outputs, 1); // only mfi (mandatory)
            assert!(!result.states.is_null());

            tulip_ffi_simd_result_free(result);
        }
    }

    #[test]
    fn test_mfi_info() {
        let info = mfi_info();
        assert!(info.inputs.len > 0);
        assert_eq!(info.options.len, OPTIONS);
        assert!(info.outputs.len > 0);
        assert!(info.optional_outputs.len > 0);
    }

    #[test]
    fn test_mfi_min_data() {
        unsafe {
            let options: [f64; OPTIONS] = [14.0];
            let min = mfi_min_data(options.as_ptr());
            assert!(min > 0);
        }
    }
}
