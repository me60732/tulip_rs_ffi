//! extern "C" wrapper for `md`, mirroring the core `tulip_rs` crate's
//! `Md::indicator` / `IndicatorState::batch_indicator` interface with a
//! Tulip-Indicators-style C calling convention: `inputs` is an array of
//! `INPUTS` pointers (one per input series, each `data_len` long, in the same
//! order as `tulip_rs::indicators::md::Md::INFO.inputs`), `options`
//! is a flat array of `OPTIONS` values -- exactly like `ti_md`'s
//! `double const *const *inputs, double const *options`. Output allocation
//! stays inside the indicator function (never caller-supplied).
//!
//! Parameter order convention (kept consistent across every function in
//! this crate): each pointer parameter is immediately followed by the
//! count(s) that describe it, e.g. `inputs, data_len, options, ...,
//! optional_outputs, numoptional`.
//!
//! SIMD entry points:
//! - `md_simd_by_assets`: compute MD for N assets simultaneously,
//!   sharing a single options array. N must be 2, 4, 8, or 16.
//! - `md_simd_by_options`: compute MD for one asset with N different
//!   option sets simultaneously. N must be 2, 4, 8, or 16.
//!
//! `#[no_mangle]` functions can't be generic, so each SIMD entry point is a
//! thin non-generic dispatcher that matches the runtime `N` against
//! {2,4,8,16} and calls a private generic `_n::<N>` helper that does the
//! real work.

use std::os::raw::c_void;

use tulip_rs::indicator_types::{Indicator, IndicatorByOptions, TIndicatorState};
use tulip_rs::indicators::md::{IndicatorState as MdState, Md, INPUTS, OPTIONS};
use tulip_rs::types::IndicatorError;

use crate::common::{
    optional_outputs_slice, pack_info, pack_outputs, pack_simd_outputs, pack_states, read_inputs,
    read_simd_assets_inputs, read_simd_options, CBatchResult, CIndicatorError, CIndicatorInfo,
    CIndicatorResult, CSimdResult,
};

/// Opaque state handle returned by `md_indicator()` and consumed by
/// `md_batch()` / `md_state_free()`.
pub type MdStateHandle = MdState;

/// Returns static metadata about the `md` indicator: its name, input
/// names, option names, and (mandatory/optional) output names, mirroring
/// `Md::INFO`.
///
/// The returned strings are leaked, process-lifetime C strings -- read them,
/// don't free them.
#[no_mangle]
pub extern "C" fn md_info() -> CIndicatorInfo {
    pack_info(&Md::INFO)
}

/// Returns the minimum number of bars `md` needs to produce any output at
/// all, given `options`.
///
/// # Safety
/// `options` must point to `OPTIONS` (1) valid `f64`s.
#[no_mangle]
pub unsafe extern "C" fn md_min_data(options: *const f64) -> usize {
    let options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    Md::min_data(&options)
}

/// Runs `md` over `data_len` bars.
///
/// `inputs` must point to `INPUTS` (1) pointer: `real`, `data_len` `f64`s long.
/// `options` must point to `OPTIONS` (1) value: `period`.
///
/// Returns the mandatory `md` output plus any requested optional outputs
/// (`sma`) and a fresh continuation state.
///
/// # Safety
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s (pass null + 0 to request no optional outputs).
#[no_mangle]
pub unsafe extern "C" fn md_indicator(
    inputs: *const *const f64,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CIndicatorResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match Md::indicator(&inputs, &options, optional) {
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

/// Continues a `md` calculation from `state`, mutating it in place so
/// it's ready for the next call. `state` must have come from
/// `md_indicator()` and not yet have been passed to `md_state_free()`.
///
/// `inputs` must point to `INPUTS` (1) pointer: `real`, `data_len` `f64`s long.
///
/// # Safety
/// - `state` must be a live pointer previously returned by
///   `md_indicator()`.
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn md_batch(
    state: *mut c_void,
    inputs: *const *const f64,
    data_len: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CBatchResult {
    if state.is_null() {
        return CBatchResult::err(IndicatorError::InvalidIndicatorState);
    }
    let state = &mut *(state as *mut MdStateHandle);

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

/// Frees a state handle returned by `md_indicator()` (or one of the
/// `states[i]` entries from `md_simd_by_assets()`/`md_simd_by_options()`).
/// Call this once you're done streaming (after your last `md_batch()` call).
///
/// # Safety
/// `state` must be a live pointer previously returned by `md_indicator()`
/// or found in a `CSimdResult::states` array, and must not be used again
/// after this call.
#[no_mangle]
pub unsafe extern "C" fn md_state_free(state: *mut c_void) {
    if !state.is_null() {
        drop(Box::from_raw(state as *mut MdStateHandle));
    }
}

/// Computes MD for `N` assets simultaneously (SIMD), sharing a single
/// options array. `num_assets` must be 2, 4, 8, or 16 -- anything else
/// returns `InvalidInputs`.
///
/// `inputs` must point to `num_assets` pointers, each itself pointing to
/// `INPUTS` (1) pointer (`real`), `data_len` `f64`s long. `options` must point
/// to `OPTIONS` (1) shared values.
///
/// Returns a `CSimdResult` with `num_assets` results, each with its own
/// output rows and its own ordinary `md` continuation state (reusable
/// with `md_batch()`/`md_state_free()`). Free each state via
/// `md_state_free()`, then free the rest via `tulip_ffi_simd_result_free()`.
///
/// # Safety
/// - `inputs` must point to `num_assets` valid pointers, each pointing to
///   `INPUTS` valid non-null `*const f64`s, each pointing to `data_len` valid
///   `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn md_simd_by_assets(
    inputs: *const *const *const f64,
    num_assets: usize,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    match num_assets {
        2 => md_simd_by_assets_n::<2>(inputs, data_len, options, optional_outputs, numoptional),
        4 => md_simd_by_assets_n::<4>(inputs, data_len, options, optional_outputs, numoptional),
        8 => md_simd_by_assets_n::<8>(inputs, data_len, options, optional_outputs, numoptional),
        16 => md_simd_by_assets_n::<16>(inputs, data_len, options, optional_outputs, numoptional),
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn md_simd_by_assets_n<const N: usize>(
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

    match Md::indicator_by_assets::<N>(&refs, &options, optional) {
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

/// Computes MD for one asset with `N` different option sets
/// simultaneously (SIMD). `num_option_sets` must be 2, 4, 8, or 16 --
/// anything else returns `InvalidInputs`.
///
/// `inputs` must point to `INPUTS` (1) pointer (`real`), `data_len` `f64`s
/// long, shared across all option sets. `options` must point to
/// `num_option_sets` pointers, each itself pointing to `OPTIONS` (1)
/// values.
///
/// Returns a `CSimdResult` with `num_option_sets` results, each with its
/// own output rows and its own ordinary `md` continuation state
/// (reusable with `md_batch()`/`md_state_free()`). Free each state via
/// `md_state_free()`, then free the rest via
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
pub unsafe extern "C" fn md_simd_by_options(
    inputs: *const *const f64,
    data_len: usize,
    options: *const *const f64,
    num_option_sets: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    match num_option_sets {
        2 => md_simd_by_options_n::<2>(inputs, data_len, options, optional_outputs, numoptional),
        4 => md_simd_by_options_n::<4>(inputs, data_len, options, optional_outputs, numoptional),
        8 => md_simd_by_options_n::<8>(inputs, data_len, options, optional_outputs, numoptional),
        16 => md_simd_by_options_n::<16>(inputs, data_len, options, optional_outputs, numoptional),
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn md_simd_by_options_n<const N: usize>(
    inputs: *const *const f64,
    data_len: usize,
    options: *const *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let options = read_simd_options::<N, OPTIONS>(options);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match Md::indicator_by_options::<N>(&inputs, &options, optional) {
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

    #[test]
    fn test_md_info() {
        let info = md_info();
        assert!(info.inputs.len > 0);
        assert_eq!(info.options.len, 1);
        assert!(info.outputs.len > 0);
        assert_eq!(info.optional_outputs.len, 1);
    }

    #[test]
    fn test_md_min_data() {
        unsafe {
            let options: [f64; OPTIONS] = [5.0];
            let min = md_min_data(options.as_ptr());
            assert!(min > 0);
        }
    }

    #[test]
    fn test_md_indicator() {
        unsafe {
            let data_len = 20;
            let real = build_synthetic_data(data_len);

            let inputs: [*const f64; INPUTS] = [real.as_ptr()];
            let options = [5.0];

            let result = md_indicator(
                inputs.as_ptr(),
                data_len,
                options.as_ptr(),
                std::ptr::null(),
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_outputs, 1); // only md (mandatory)

            tulip_ffi_result_free(result);
        }
    }

    #[test]
    fn test_md_batch() {
        unsafe {
            let data_len = 20;
            let real1 = build_synthetic_data(data_len);

            let inputs1: [*const f64; INPUTS] = [real1.as_ptr()];
            let options = [5.0];

            let result = md_indicator(
                inputs1.as_ptr(),
                data_len,
                options.as_ptr(),
                std::ptr::null(),
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            let state = result.state;

            let data_len2 = 10;
            let real2 = build_synthetic_data(data_len2);

            let inputs2: [*const f64; INPUTS] = [real2.as_ptr()];

            let batch_result = md_batch(state, inputs2.as_ptr(), data_len2, std::ptr::null(), 0);

            assert_eq!(batch_result.error, CIndicatorError::Ok);
            tulip_ffi_batch_result_free(batch_result);
            md_state_free(state);
        }
    }

    #[test]
    fn test_md_simd_by_assets() {
        const NUM_ASSETS: usize = 2;
        unsafe {
            let data_len = 20;
            let real1 = build_synthetic_data(data_len);
            let real2 = build_synthetic_data(data_len);

            let inputs_array1: [*const f64; INPUTS] = [real1.as_ptr()];
            let inputs_array2: [*const f64; INPUTS] = [real2.as_ptr()];

            let inputs: [*const *const f64; NUM_ASSETS] =
                [inputs_array1.as_ptr(), inputs_array2.as_ptr()];

            let options = [5.0];

            let result = md_simd_by_assets(
                inputs.as_ptr(),
                NUM_ASSETS,
                data_len,
                options.as_ptr(),
                std::ptr::null(),
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, NUM_ASSETS);

            for i in 0..NUM_ASSETS {
                let state_ptr = result.states.wrapping_add(i);
                md_state_free(*state_ptr);
            }
            tulip_ffi_simd_result_free(result);
        }
    }

    #[test]
    fn test_md_simd_by_options() {
        const NUM_OPTION_SETS: usize = 2;
        unsafe {
            let data_len = 20;
            let real = build_synthetic_data(data_len);

            let inputs: [*const f64; INPUTS] = [real.as_ptr()];

            let options1: [f64; OPTIONS] = [5.0];
            let options2: [f64; OPTIONS] = [10.0];

            let options_array: [*const f64; NUM_OPTION_SETS] =
                [options1.as_ptr(), options2.as_ptr()];

            let result = md_simd_by_options(
                inputs.as_ptr(),
                data_len,
                options_array.as_ptr(),
                NUM_OPTION_SETS,
                std::ptr::null(),
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, NUM_OPTION_SETS);

            for i in 0..NUM_OPTION_SETS {
                let state_ptr = result.states.wrapping_add(i);
                md_state_free(*state_ptr);
            }
            tulip_ffi_simd_result_free(result);
        }
    }
}
