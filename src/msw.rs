//! extern "C" wrapper for `msw`, mirroring the core `tulip_rs` crate's
//! `Msw::indicator` / `IndicatorState::batch_indicator` interface with a
//! Tulip-Indicators-style C calling convention: `inputs` is an array of
//! `INPUTS` pointers (one per input series, each `data_len` long, in the same
//! order as `tulip_rs::indicators::msw::Msw::INFO.inputs`), `options`
//! is a flat array of `OPTIONS` values -- exactly like `ti_msw`'s
//! `double const *const *inputs, double const *options`. Output allocation
//! stays inside the indicator function (never caller-supplied).
//!
//! Parameter order convention (kept consistent across every function in
//! this crate): each pointer parameter is immediately followed by the
//! count(s) that describe it, e.g. `inputs, data_len, options, ...,
//! optional_outputs, numoptional`.
//!
//! SIMD entry points:
//! - `msw_simd_by_assets`: compute MSW for N assets simultaneously,
//!   sharing a single options array. N must be 2, 4, 8, or 16.
//! - `msw_simd_by_options`: compute MSW for one asset with N different
//!   option sets simultaneously. N must be 2, 4, 8, or 16.
//!
//! `#[no_mangle]` functions can't be generic, so each SIMD entry point is a
//! thin non-generic dispatcher that matches the runtime `N` against
//! {2,4,8,16} and calls a private generic `_n::<N>` helper that does the
//! real work.

use std::os::raw::c_void;

use tulip_rs::indicator_types::{Indicator, IndicatorByOptions, TIndicatorState};
use tulip_rs::indicators::msw::{IndicatorState as MswState, Msw, INPUTS, OPTIONS};
use tulip_rs::types::IndicatorError;

use crate::common::{
    optional_outputs_slice, pack_info, pack_outputs, pack_simd_outputs, pack_states, read_inputs,
    read_simd_assets_inputs, read_simd_options, CBatchResult, CIndicatorError, CIndicatorInfo,
    CIndicatorResult, CSimdResult,
};

/// Opaque state handle returned by `msw_indicator()` and consumed by
/// `msw_batch()` / `msw_state_free()`.
pub type MswStateHandle = MswState;

/// Returns static metadata about the `msw` indicator: its name, input
/// names, option names, and (mandatory/optional) output names, mirroring
/// `Msw::INFO`.
///
/// The returned strings are leaked, process-lifetime C strings -- read them,
/// don't free them.
#[no_mangle]
pub extern "C" fn msw_info() -> CIndicatorInfo {
    pack_info(&Msw::INFO)
}

/// Returns the minimum number of bars `msw` needs to produce any output at
/// all, given `options`.
///
/// # Safety
/// `options` must point to `OPTIONS` (3) valid `f64`s.
#[no_mangle]
pub unsafe extern "C" fn msw_min_data(options: *const f64) -> usize {
    let options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    Msw::min_data(&options)
}

/// Runs `msw` over `data_len` bars.
///
/// `inputs` must point to `INPUTS` (1) pointer: `real`, `data_len` `f64`s long.
/// `options` must point to `OPTIONS` (3) values: `short_period, long_period,
/// cycle_period`.
///
/// Returns the mandatory `msw` output plus any requested optional outputs
/// (`msw`, `trend`, in that fixed order) and a fresh continuation state.
///
/// # Safety
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s (pass null + 0 to request no optional outputs).
#[no_mangle]
pub unsafe extern "C" fn msw_indicator(
    inputs: *const *const f64,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CIndicatorResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match Msw::indicator(&inputs, &options, optional) {
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

/// Continues an `msw` calculation from `state`, mutating it in place so
/// it's ready for the next call. `state` must have come from
/// `msw_indicator()` and not yet have been passed to `msw_state_free()`.
///
/// `inputs` must point to `INPUTS` (1) pointer: `real`, `data_len` `f64`s long.
///
/// # Safety
/// - `state` must be a live pointer previously returned by
///   `msw_indicator()`.
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn msw_batch(
    state: *mut c_void,
    inputs: *const *const f64,
    data_len: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CBatchResult {
    if state.is_null() {
        return CBatchResult::err(IndicatorError::InvalidIndicatorState);
    }
    let state = &mut *(state as *mut MswStateHandle);

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

/// Frees a state handle returned by `msw_indicator()` (or one of the
/// `states[i]` entries from `msw_simd_by_assets()`/`msw_simd_by_options()`).
/// Call this once you're done streaming (after your last `msw_batch()` call).
///
/// # Safety
/// `state` must be a live pointer previously returned by `msw_indicator()`
/// or found in a `CSimdResult::states` array, and must not be used again
/// after this call.
#[no_mangle]
pub unsafe extern "C" fn msw_state_free(state: *mut c_void) {
    if !state.is_null() {
        drop(Box::from_raw(state as *mut MswStateHandle));
    }
}

/// Computes MSW for `N` assets simultaneously (SIMD), sharing a single
/// options array. `num_assets` must be 2, 4, 8, or 16 -- anything else
/// returns `InvalidInputs`.
///
/// `inputs` must point to `num_assets` pointers, each itself pointing to
/// `INPUTS` (1) pointer (`real`), `data_len` `f64`s long. `options` must point
/// to `OPTIONS` (3) shared values.
///
/// Returns a `CSimdResult` with `num_assets` results, each with its own
/// output rows and its own ordinary `msw` continuation state (reusable
/// with `msw_batch()`/`msw_state_free()`). Free each state via
/// `msw_state_free()`, then free the rest via `tulip_ffi_simd_result_free()`.
///
/// # Safety
/// - `inputs` must point to `num_assets` valid pointers, each pointing to
///   `INPUTS` valid non-null `*const f64`s, each pointing to `data_len` valid
///   `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn msw_simd_by_assets(
    inputs: *const *const *const f64,
    num_assets: usize,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    match num_assets {
        2 => msw_simd_by_assets_n::<2>(inputs, data_len, options, optional_outputs, numoptional),
        4 => msw_simd_by_assets_n::<4>(inputs, data_len, options, optional_outputs, numoptional),
        8 => msw_simd_by_assets_n::<8>(inputs, data_len, options, optional_outputs, numoptional),
        16 => msw_simd_by_assets_n::<16>(inputs, data_len, options, optional_outputs, numoptional),
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn msw_simd_by_assets_n<const N: usize>(
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

    match Msw::indicator_by_assets::<N>(&refs, &options, optional) {
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

/// Computes MSW for one asset with `N` different option sets
/// simultaneously (SIMD). `num_option_sets` must be 2, 4, 8, or 16 --
/// anything else returns `InvalidInputs`.
///
/// `inputs` must point to `INPUTS` (1) pointer (`real`), `data_len` `f64`s
/// long, shared across all option sets. `options` must point to
/// `num_option_sets` pointers, each itself pointing to `OPTIONS` (3)
/// values.
///
/// Returns a `CSimdResult` with `num_option_sets` results, each with its
/// own output rows and its own ordinary `msw` continuation state
/// (reusable with `msw_batch()`/`msw_state_free()`). Free each state via
/// `msw_state_free()`, then free the rest via
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
pub unsafe extern "C" fn msw_simd_by_options(
    inputs: *const *const f64,
    data_len: usize,
    options: *const *const f64,
    num_option_sets: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    match num_option_sets {
        2 => msw_simd_by_options_n::<2>(inputs, data_len, options, optional_outputs, numoptional),
        4 => msw_simd_by_options_n::<4>(inputs, data_len, options, optional_outputs, numoptional),
        8 => msw_simd_by_options_n::<8>(inputs, data_len, options, optional_outputs, numoptional),
        16 => msw_simd_by_options_n::<16>(inputs, data_len, options, optional_outputs, numoptional),
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn msw_simd_by_options_n<const N: usize>(
    inputs: *const *const f64,
    data_len: usize,
    options: *const *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let options = read_simd_options::<N, OPTIONS>(options);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match Msw::indicator_by_options::<N>(&inputs, &options, optional) {
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

    unsafe fn free_simd_result(result: CSimdResult) {
        for i in 0..result.num_results {
            msw_state_free(*result.states.add(i));
        }
        tulip_ffi_simd_result_free(result);
    }

    #[test]
    fn test_msw_indicator() {
        unsafe {
            let data_len = 60;
            let inputs_data: Vec<f64> = build_synthetic_data(data_len);
            let inputs_ptr = Box::into_raw(Box::new(inputs_data)) as *const f64;
            let inputs = [&inputs_ptr];

            let options: [f64; OPTIONS] = [10.0];

            let optional_outputs: [bool; 0] = [];

            let result = msw_indicator(
                &inputs as *const _ as *const *const f64,
                data_len,
                &options as *const _,
                &optional_outputs as *const _,
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_outputs, 2);

            tulip_ffi_result_free(result);
        }
    }

    #[test]
    fn test_msw_batch() {
        unsafe {
            let data_len = 60;
            let inputs_data: Vec<f64> = build_synthetic_data(data_len);
            let inputs_ptr = Box::into_raw(Box::new(inputs_data)) as *const f64;
            let inputs = [&inputs_ptr];

            let options: [f64; OPTIONS] = [10.0];

            let optional_outputs: [bool; 0] = [];

            let result = msw_indicator(
                &inputs as *const _ as *const *const f64,
                data_len,
                &options as *const _,
                &optional_outputs as *const _,
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);

            let state = result.state;

            // Additional batch call
            let more_data: Vec<f64> = build_synthetic_data(30);
            let more_ptr = Box::into_raw(Box::new(more_data)) as *const f64;
            let more_inputs = [&more_ptr];

            let batch_result = msw_batch(
                state,
                &more_inputs as *const _ as *const *const f64,
                30,
                std::ptr::null(),
                0,
            );

            assert_eq!(batch_result.error, CIndicatorError::Ok);

            tulip_ffi_batch_result_free(batch_result);
            msw_state_free(state);
        }
    }

    #[test]
    fn test_msw_simd_by_assets() {
        unsafe {
            const NUM_ASSETS: usize = 2;
            let data_len = 60;

            let asset1_data: Vec<f64> = build_synthetic_data(data_len);
            let asset2_data: Vec<f64> = build_synthetic_data(data_len);

            let asset1_ptr = Box::into_raw(Box::new(asset1_data)) as *const f64;
            let asset2_ptr = Box::into_raw(Box::new(asset2_data)) as *const f64;

            let inputs_asset1 = [&asset1_ptr];
            let inputs_asset2 = [&asset2_ptr];

            let all_inputs = [inputs_asset1, inputs_asset2];

            let options: [f64; OPTIONS] = [10.0];

            let optional_outputs: [bool; 0] = [];

            let result = msw_simd_by_assets(
                &all_inputs as *const _ as *const *const *const f64,
                NUM_ASSETS,
                data_len,
                &options as *const _,
                &optional_outputs as *const _,
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, NUM_ASSETS);

            free_simd_result(result);
        }
    }

    #[test]
    fn test_msw_simd_by_options() {
        unsafe {
            let data_len = 60;

            let inputs_data: Vec<f64> = build_synthetic_data(data_len);
            let inputs_ptr = Box::into_raw(Box::new(inputs_data)) as *const f64;
            let inputs = [&inputs_ptr];

            let options1: [f64; OPTIONS] = [10.0];
            let options2: [f64; OPTIONS] = [20.0];

            let all_options: [*const f64; 2] = [options1.as_ptr(), options2.as_ptr()];

            let optional_outputs: [bool; 0] = [];

            let result = msw_simd_by_options(
                &inputs as *const _ as *const *const f64,
                data_len,
                all_options.as_ptr(),
                2,
                &optional_outputs as *const _,
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, 2);

            free_simd_result(result);
        }
    }

    #[test]
    fn test_msw_info() {
        let info = msw_info();
        assert!(info.inputs.len > 0);
        assert_eq!(info.options.len, OPTIONS);
        assert!(info.outputs.len > 0);
        assert_eq!(info.optional_outputs.len, 0);
    }

    #[test]
    fn test_msw_min_data() {
        unsafe {
            let options: [f64; OPTIONS] = [10.0];
            let min = msw_min_data(options.as_ptr());
            assert!(min > 0);
        }
    }
}
