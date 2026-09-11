//! extern "C" wrapper for `kvo`, mirroring the core `tulip_rs` crate's
//! `Kvo::indicator` / `IndicatorState::batch_indicator` interface with a
//! Tulip-Indicators-style C calling convention: `inputs` is an array of
//! `INPUTS` pointers (one per input series, each `data_len` long, in the same
//! order as `tulip_rs::indicators::kvo::Kvo::INFO.inputs`), `options`
//! is a flat array of `OPTIONS` values -- exactly like `ti_kvo`'s
//! `double const *const *inputs, double const *options`. Output allocation
//! stays inside the indicator function (never caller-supplied).
//!
//! Parameter order convention (kept consistent across every function in
//! this crate): each pointer parameter is immediately followed by the
//! count(s) that describe it, e.g. `inputs, data_len, options, ...,
//! optional_outputs, numoptional`.
//!
//! SIMD entry points:
//! - `kvo_simd_by_assets`: compute KVO for N assets simultaneously,
//!   sharing a single options array. N must be 2, 4, 8, or 16.
//! - `kvo_simd_by_options`: compute KVO for one asset with N different
//!   option sets simultaneously. N must be 2, 4, 8, or 16.
//!
//! `#[no_mangle]` functions can't be generic, so each SIMD entry point is a
//! thin non-generic dispatcher that matches the runtime `N` against
//! {2,4,8,16} and calls a private generic `_n::<N>` helper that does the
//! real work.

use std::os::raw::c_void;

use tulip_rs::indicator_types::{Indicator, IndicatorByOptions, TIndicatorState};
use tulip_rs::indicators::kvo::{IndicatorState as KvoState, Kvo, INPUTS, OPTIONS};
use tulip_rs::types::IndicatorError;

use crate::common::{
    optional_outputs_slice, pack_info, pack_outputs, pack_simd_outputs, pack_states, read_inputs,
    read_simd_assets_inputs, read_simd_options, CBatchResult, CIndicatorError, CIndicatorInfo,
    CIndicatorResult, CSimdResult,
};

/// Opaque state handle returned by `kvo_indicator()` and consumed by
/// `kvo_batch()` / `kvo_state_free()`.
pub type KvoStateHandle = KvoState;

/// Returns static metadata about the `kvo` indicator: its name, input
/// names, option names, and (mandatory/optional) output names, mirroring
/// `Kvo::INFO`.
///
/// The returned strings are leaked, process-lifetime C strings -- read them,
/// don't free them.
#[no_mangle]
pub extern "C" fn kvo_info() -> CIndicatorInfo {
    pack_info(&Kvo::INFO)
}

/// Returns the minimum number of bars `kvo` needs to produce any output at
/// all, given `options`.
///
/// # Safety
/// `options` must point to `OPTIONS` (2) valid `f64`s.
#[no_mangle]
pub unsafe extern "C" fn kvo_min_data(options: *const f64) -> usize {
    let options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    Kvo::min_data(&options)
}

/// Runs `kvo` over `data_len` bars.
///
/// `inputs` must point to `INPUTS` (4) pointers, in order:
/// `high, low, close, volume`, each `data_len` `f64`s long.
/// `options` must point to `OPTIONS` (2) values: `short_period, long_period`.
///
/// Returns the mandatory `kvo` output plus any requested optional outputs
/// (`short_ema, long_ema`) and a fresh continuation state.
///
/// # Safety
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s (pass null + 0 to request no optional outputs).
#[no_mangle]
pub unsafe extern "C" fn kvo_indicator(
    inputs: *const *const f64,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CIndicatorResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match Kvo::indicator(&inputs, &options, optional) {
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

/// Continues a `kvo` calculation from `state`, mutating it in place so
/// it's ready for the next call. `state` must have come from
/// `kvo_indicator()` and not yet have been passed to `kvo_state_free()`.
///
/// `inputs` must point to `INPUTS` (4) pointers (`high, low, close,
/// volume`), each `data_len` `f64`s long.
///
/// # Safety
/// - `state` must be a live pointer previously returned by
///   `kvo_indicator()`.
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn kvo_batch(
    state: *mut c_void,
    inputs: *const *const f64,
    data_len: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CBatchResult {
    if state.is_null() {
        return CBatchResult::err(IndicatorError::InvalidIndicatorState);
    }
    let state = &mut *(state as *mut KvoStateHandle);

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

/// Frees a state handle returned by `kvo_indicator()` (or one of the
/// `states[i]` entries from `kvo_simd_by_assets()`/`kvo_simd_by_options()`).
/// Call this once you're done streaming (after your last `kvo_batch()` call).
///
/// # Safety
/// `state` must be a live pointer previously returned by `kvo_indicator()`
/// or found in a `CSimdResult::states` array, and must not be used again
/// after this call.
#[no_mangle]
pub unsafe extern "C" fn kvo_state_free(state: *mut c_void) {
    if !state.is_null() {
        drop(Box::from_raw(state as *mut KvoStateHandle));
    }
}

/// Computes KVO for `N` assets simultaneously (SIMD), sharing a single
/// options array. `num_assets` must be 2, 4, 8, or 16 -- anything else
/// returns `InvalidInputs`.
///
/// `inputs` must point to `num_assets` pointers, each itself pointing to
/// `INPUTS` (4) pointers (`high, low, close, volume`), each `data_len` `f64`s
/// long. `options` must point to `OPTIONS` (2) shared values.
///
/// Returns a `CSimdResult` with `num_assets` results, each with its own
/// output rows and its own ordinary `kvo` continuation state (reusable
/// with `kvo_batch()`/`kvo_state_free()`). Free each state via
/// `kvo_state_free()`, then free the rest via `tulip_ffi_simd_result_free()`.
///
/// # Safety
/// - `inputs` must point to `num_assets` valid pointers, each pointing to
///   `INPUTS` valid non-null `*const f64`s, each pointing to `data_len` valid
///   `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn kvo_simd_by_assets(
    inputs: *const *const *const f64,
    num_assets: usize,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    match num_assets {
        2 => kvo_simd_by_assets_n::<2>(inputs, data_len, options, optional_outputs, numoptional),
        4 => kvo_simd_by_assets_n::<4>(inputs, data_len, options, optional_outputs, numoptional),
        8 => kvo_simd_by_assets_n::<8>(inputs, data_len, options, optional_outputs, numoptional),
        16 => kvo_simd_by_assets_n::<16>(inputs, data_len, options, optional_outputs, numoptional),
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn kvo_simd_by_assets_n<const N: usize>(
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

    match Kvo::indicator_by_assets::<N>(&refs, &options, optional) {
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

/// Computes KVO for one asset with `N` different option sets
/// simultaneously (SIMD). `num_option_sets` must be 2, 4, 8, or 16 --
/// anything else returns `InvalidInputs`.
///
/// `inputs` must point to `INPUTS` (4) pointers (`high, low, close,
/// volume`), each `data_len` `f64`s long, shared across all option sets.
/// `options` must point to `num_option_sets` pointers, each itself
/// pointing to `OPTIONS` (2) values.
///
/// Returns a `CSimdResult` with `num_option_sets` results, each with its
/// own output rows and its own ordinary `kvo` continuation state
/// (reusable with `kvo_batch()`/`kvo_state_free()`). Free each state via
/// `kvo_state_free()`, then free the rest via
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
pub unsafe extern "C" fn kvo_simd_by_options(
    inputs: *const *const f64,
    data_len: usize,
    options: *const *const f64,
    num_option_sets: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    match num_option_sets {
        2 => kvo_simd_by_options_n::<2>(inputs, data_len, options, optional_outputs, numoptional),
        4 => kvo_simd_by_options_n::<4>(inputs, data_len, options, optional_outputs, numoptional),
        8 => kvo_simd_by_options_n::<8>(inputs, data_len, options, optional_outputs, numoptional),
        16 => kvo_simd_by_options_n::<16>(inputs, data_len, options, optional_outputs, numoptional),
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn kvo_simd_by_options_n<const N: usize>(
    inputs: *const *const f64,
    data_len: usize,
    options: *const *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let options = read_simd_options::<N, OPTIONS>(options);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match Kvo::indicator_by_options::<N>(&inputs, &options, optional) {
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

    unsafe fn min_data(options: &[f64; OPTIONS]) -> usize {
        Kvo::min_data(options)
    }

    #[test]
    fn test_kvo_info() {
        let info = kvo_info();
        assert!(info.inputs.len > 0);
        assert_eq!(info.options.len, 2);
        assert!(info.outputs.len > 0);
    }

    #[test]
    fn test_kvo_min_data() {
        unsafe {
            let options: [f64; OPTIONS] = [10.0, 30.0];
            let min = kvo_min_data(options.as_ptr());
            assert!(min > 0);
        }
    }

    #[test]
    fn test_kvo_indicator() {
        unsafe {
            let options = [10.0, 30.0];
            let len = min_data(&options);
            let high = build_synthetic_data(len, 0);
            let low = build_synthetic_data(len, 0);
            let close = build_synthetic_data(len, 0);
            let volume = build_synthetic_data(len, 0);

            let inputs: [*const f64; INPUTS] =
                [high.as_ptr(), low.as_ptr(), close.as_ptr(), volume.as_ptr()];

            let result = kvo_indicator(inputs.as_ptr(), len, options.as_ptr(), std::ptr::null(), 0);

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_outputs, 1); // only kvo (mandatory)

            tulip_ffi_result_free(result);
        }
    }

    #[test]
    fn test_kvo_batch() {
        unsafe {
            let options = [10.0, 30.0];
            let len1 = min_data(&options);
            let high1 = build_synthetic_data(len1, 0);
            let low1 = build_synthetic_data(len1, 0);
            let close1 = build_synthetic_data(len1, 0);
            let volume1 = build_synthetic_data(len1, 0);

            let inputs1: [*const f64; INPUTS] = [
                high1.as_ptr(),
                low1.as_ptr(),
                close1.as_ptr(),
                volume1.as_ptr(),
            ];

            let result = kvo_indicator(
                inputs1.as_ptr(),
                len1,
                options.as_ptr(),
                std::ptr::null(),
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            let state = result.state;

            let len2 = 30;
            let high2 = build_synthetic_data(len2, 0);
            let low2 = build_synthetic_data(len2, 0);
            let close2 = build_synthetic_data(len2, 0);
            let volume2 = build_synthetic_data(len2, 0);

            let inputs2: [*const f64; INPUTS] = [
                high2.as_ptr(),
                low2.as_ptr(),
                close2.as_ptr(),
                volume2.as_ptr(),
            ];

            let batch_result = kvo_batch(state, inputs2.as_ptr(), len2, std::ptr::null(), 0);

            assert_eq!(batch_result.error, CIndicatorError::Ok);
            tulip_ffi_batch_result_free(batch_result);
            kvo_state_free(state);
        }
    }

    #[test]
    fn test_kvo_simd_by_assets() {
        const NUM_ASSETS: usize = 2;
        unsafe {
            let options = [10.0, 30.0];
            let len = min_data(&options);

            let high1 = build_synthetic_data(len, 0);
            let low1 = build_synthetic_data(len, 0);
            let close1 = build_synthetic_data(len, 0);
            let volume1 = build_synthetic_data(len, 0);
            let high2 = build_synthetic_data(len, 0);
            let low2 = build_synthetic_data(len, 0);
            let close2 = build_synthetic_data(len, 0);
            let volume2 = build_synthetic_data(len, 0);

            let inputs_array: [*const f64; INPUTS] = [
                high1.as_ptr(),
                low1.as_ptr(),
                close1.as_ptr(),
                volume1.as_ptr(),
            ];
            let inputs_array2: [*const f64; INPUTS] = [
                high2.as_ptr(),
                low2.as_ptr(),
                close2.as_ptr(),
                volume2.as_ptr(),
            ];

            let inputs: [*const *const f64; NUM_ASSETS] =
                [inputs_array.as_ptr(), inputs_array2.as_ptr()];

            let result = kvo_simd_by_assets(
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
                kvo_state_free(*state_ptr);
            }
            tulip_ffi_simd_result_free(result);
        }
    }

    #[test]
    fn test_kvo_simd_by_options() {
        const NUM_OPTION_SETS: usize = 2;
        unsafe {
            // Must satisfy min_data for *all* option sets used below.
            let len = min_data(&[10.0, 30.0]).max(min_data(&[15.0, 35.0]));

            let high = build_synthetic_data(len, 0);
            let low = build_synthetic_data(len, 0);
            let close = build_synthetic_data(len, 0);
            let volume = build_synthetic_data(len, 0);

            let inputs: [*const f64; INPUTS] =
                [high.as_ptr(), low.as_ptr(), close.as_ptr(), volume.as_ptr()];

            let options1: [f64; OPTIONS] = [10.0, 30.0];
            let options2: [f64; OPTIONS] = [15.0, 35.0];

            let options_array: [*const f64; NUM_OPTION_SETS] =
                [options1.as_ptr(), options2.as_ptr()];

            let result = kvo_simd_by_options(
                inputs.as_ptr(),
                len,
                options_array.as_ptr(),
                NUM_OPTION_SETS,
                std::ptr::null(),
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, NUM_OPTION_SETS);

            for i in 0..NUM_OPTION_SETS {
                let state_ptr = result.states.wrapping_add(i);
                kvo_state_free(*state_ptr);
            }
            tulip_ffi_simd_result_free(result);
        }
    }
}
