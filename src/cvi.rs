//! extern "C" wrapper for `cvi`, mirroring the core `tulip_rs` crate's
//! `Cvi::indicator` / `IndicatorState::batch_indicator` interface with a
//! Tulip-Indicators-style C calling convention: `inputs` is an array of
//! `INPUTS` pointers (one per input series, each `data_len` long, in the same
//! order as `tulip_rs::indicators::cvi::Cvi::INFO.inputs`), `options`
//! is a flat array of `OPTIONS` values -- exactly like `ti_cvi`'s
//! `double const *const *inputs, double const *options`. Output allocation
//! stays inside the indicator function (never caller-supplied).
//!
//! Parameter order convention (kept consistent across every function in
//! this crate): each pointer parameter is immediately followed by the
//! count(s) that describe it, e.g. `inputs, data_len, options, ...,
//! optional_outputs, num_optional`.
//!
//! SIMD entry points:
//! - `cvi_simd_by_assets`: compute CVI for N assets simultaneously,
//!   sharing a single options array. N must be 2, 4, 8, or 16.
//! - `cvi_simd_by_options`: compute CVI for one asset with N different
//!   option sets simultaneously. N must be 2, 4, 8, or 16.
//!
//! `#[no_mangle]` functions can't be generic, so each SIMD entry point is a
//! thin non-generic dispatcher that matches the runtime `N` against
//! {2,4,8,16} and calls a private generic `_n::<N>` helper that does the
//! real work.

use std::os::raw::c_void;

use tulip_rs::indicator_types::{Indicator, IndicatorByOptions, TIndicatorState};
use tulip_rs::indicators::cvi::{Cvi, IndicatorState as CviState, INPUTS, OPTIONS};
use tulip_rs::types::IndicatorError;

use crate::common::{
    optional_outputs_slice, pack_info, pack_outputs, pack_simd_outputs, pack_states, read_inputs,
    read_simd_assets_inputs, read_simd_options, CBatchResult, CIndicatorError, CIndicatorInfo,
    CIndicatorResult, CSimdResult,
};

/// Opaque state handle returned by `cvi_indicator()` and consumed by
/// `cvi_batch()` / `cvi_state_free()`.
pub type CviStateHandle = CviState;

/// Runs `cvi` over `data_len` bars.
///
/// `inputs` must point to `INPUTS` (2) pointers, in order:
/// `high, low`, each `data_len` `f64`s long.
/// `options` must point to `OPTIONS` (1) value: `period`.
///
/// Returns the mandatory `cvi` output plus any requested optional outputs
/// and a fresh continuation state.
///
/// # Safety
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `num_optional` valid
///   `bool`s (pass null + 0 to request no optional outputs).
#[no_mangle]
pub unsafe extern "C" fn cvi_indicator(
    inputs: *const *const f64,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    num_optional: usize,
) -> CIndicatorResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    let optional = optional_outputs_slice(optional_outputs, num_optional);

    match Cvi::indicator(&inputs, &options, optional) {
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

/// Continues a `cvi` calculation from `state`, mutating it in place so
/// it's ready for the next call. `state` must have come from
/// `cvi_indicator()` and not yet have been passed to `cvi_state_free()`.
///
/// `inputs` must point to `INPUTS` (2) pointers (`high, low`),
/// each `data_len` `f64`s long.
///
/// # Safety
/// - `state` must be a live pointer previously returned by
///   `cvi_indicator()`.
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `num_optional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn cvi_batch(
    state: *mut c_void,
    inputs: *const *const f64,
    data_len: usize,
    optional_outputs: *const bool,
    num_optional: usize,
) -> CBatchResult {
    if state.is_null() {
        return CBatchResult::err(IndicatorError::InvalidIndicatorState);
    }
    let state = &mut *(state as *mut CviStateHandle);

    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let optional = optional_outputs_slice(optional_outputs, num_optional);

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

/// Frees a state handle returned by `cvi_indicator()` (or one of the
/// `states[i]` entries from `cvi_simd_by_assets()`/`cvi_simd_by_options()`).
/// Call this once you're done streaming (after your last `cvi_batch()` call).
///
/// # Safety
/// `state` must be a live pointer previously returned by `cvi_indicator()`
/// or found in a `CSimdResult::states` array, and must not be used again
/// after this call.
#[no_mangle]
pub unsafe extern "C" fn cvi_state_free(state: *mut c_void) {
    if !state.is_null() {
        drop(Box::from_raw(state as *mut CviStateHandle));
    }
}

/// Returns static metadata about the `cvi` indicator: its name, input
/// names, option names, and (mandatory/optional) output names, mirroring
/// `Cvi::INFO`.
///
/// The returned strings are leaked, process-lifetime C strings -- read them,
/// don't free them.
#[no_mangle]
pub extern "C" fn cvi_info() -> CIndicatorInfo {
    pack_info(&Cvi::INFO)
}

/// Returns the minimum number of bars `cvi` needs to produce any output at
/// all, given `options`.
///
/// # Safety
/// `options` must point to `OPTIONS` (1) valid `f64`s.
#[no_mangle]
pub unsafe extern "C" fn cvi_min_data(options: *const f64) -> usize {
    let options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    Cvi::min_data(&options)
}

/// Computes CVI for `N` assets simultaneously (SIMD), sharing a single
/// options array. `num_assets` must be 2, 4, 8, or 16 -- anything else
/// returns `InvalidInputs`.
///
/// `inputs` must point to `num_assets` pointers, each itself pointing to
/// `INPUTS` (2) pointers (`high, low`), each `data_len` `f64`s long.
/// `options` must point to `OPTIONS` (1) shared values.
///
/// Returns a `CSimdResult` with `num_assets` results, each with its own
/// output rows and its own ordinary `cvi` continuation state (reusable
/// with `cvi_batch()`/`cvi_state_free()`). Free each state via
/// `cvi_state_free()`, then free the rest via `tulip_ffi_simd_result_free()`.
///
/// # Safety
/// - `inputs` must point to `num_assets` valid pointers, each pointing to
///   `INPUTS` valid non-null `*const f64`s, each pointing to `data_len` valid
///   `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `num_optional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn cvi_simd_by_assets(
    inputs: *const *const *const f64,
    num_assets: usize,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    num_optional: usize,
) -> CSimdResult {
    match num_assets {
        2 => cvi_simd_by_assets_n::<2>(inputs, data_len, options, optional_outputs, num_optional),
        4 => cvi_simd_by_assets_n::<4>(inputs, data_len, options, optional_outputs, num_optional),
        8 => cvi_simd_by_assets_n::<8>(inputs, data_len, options, optional_outputs, num_optional),
        16 => cvi_simd_by_assets_n::<16>(inputs, data_len, options, optional_outputs, num_optional),
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn cvi_simd_by_assets_n<const N: usize>(
    inputs: *const *const *const f64,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    num_optional: usize,
) -> CSimdResult {
    // `owned` holds the per-asset input slices; `refs` borrows from it, so
    // both must live in this stack frame for the duration of the call.
    let owned = read_simd_assets_inputs::<N, INPUTS>(inputs, data_len);
    let refs: [&[&[f64]; INPUTS]; N] = std::array::from_fn(|i| &owned[i]);
    let options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    let optional = optional_outputs_slice(optional_outputs, num_optional);

    match Cvi::indicator_by_assets::<N>(&refs, &options, optional) {
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

/// Computes CVI for one asset with `N` different option sets
/// simultaneously (SIMD). `num_option_sets` must be 2, 4, 8, or 16 --
/// anything else returns `InvalidInputs`.
///
/// `inputs` must point to `INPUTS` (2) pointers (`high, low`),
/// each `data_len` `f64`s long, shared across all option sets.
/// `options` must point to `num_option_sets` pointers, each itself
/// pointing to `OPTIONS` (1) values.
///
/// Returns a `CSimdResult` with `num_option_sets` results, each with its
/// own output rows and its own ordinary `cvi` continuation state
/// (reusable with `cvi_batch()`/`cvi_state_free()`). Free each state via
/// `cvi_state_free()`, then free the rest via
/// `tulip_ffi_simd_result_free()`.
///
/// # Safety
/// - `inputs` must point to `INPUTS` valid non-null `*const f64`s, each
///   pointing to `data_len` valid `f64`s.
/// - `options` must point to `num_option_sets` valid pointers, each
///   pointing to `OPTIONS` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `num_optional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn cvi_simd_by_options(
    inputs: *const *const f64,
    data_len: usize,
    options: *const *const f64,
    num_option_sets: usize,
    optional_outputs: *const bool,
    num_optional: usize,
) -> CSimdResult {
    match num_option_sets {
        2 => cvi_simd_by_options_n::<2>(inputs, data_len, options, optional_outputs, num_optional),
        4 => cvi_simd_by_options_n::<4>(inputs, data_len, options, optional_outputs, num_optional),
        8 => cvi_simd_by_options_n::<8>(inputs, data_len, options, optional_outputs, num_optional),
        16 => {
            cvi_simd_by_options_n::<16>(inputs, data_len, options, optional_outputs, num_optional)
        }
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn cvi_simd_by_options_n<const N: usize>(
    inputs: *const *const f64,
    data_len: usize,
    options: *const *const f64,
    optional_outputs: *const bool,
    num_optional: usize,
) -> CSimdResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let options = read_simd_options::<N, OPTIONS>(options);
    let optional = optional_outputs_slice(optional_outputs, num_optional);

    match Cvi::indicator_by_options::<N>(&inputs, &options, optional) {
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
    fn test_cvi_indicator() {
        use crate::common::test::build_synthetic_data;

        let data_len = 20;
        let high: Vec<f64> = build_synthetic_data(data_len, 2);
        let low: Vec<f64> = build_synthetic_data(data_len, 0);

        let inputs = [high.as_ptr(), low.as_ptr()];
        let options = [10.0];

        unsafe {
            let result = cvi_indicator(
                inputs.as_ptr(),
                data_len,
                options.as_ptr(),
                std::ptr::null(),
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_outputs, 1);

            tulip_ffi_result_free(result);
        }
    }

    #[test]
    fn test_cvi_batch() {
        use crate::common::test::build_synthetic_data;

        let data_len = 20;
        let high: Vec<f64> = build_synthetic_data(data_len, 2);
        let low: Vec<f64> = build_synthetic_data(data_len, 0);

        let inputs = [high.as_ptr(), low.as_ptr()];
        let options = [10.0];

        unsafe {
            let result = cvi_indicator(
                inputs.as_ptr(),
                data_len,
                options.as_ptr(),
                std::ptr::null(),
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);

            // Second batch call with exactly 10 elements (batch length)
            let high2: Vec<f64> = build_synthetic_data(10, 2);
            let low2: Vec<f64> = build_synthetic_data(10, 0);

            let inputs2 = [high2.as_ptr(), low2.as_ptr()];

            let batch_result = cvi_batch(result.state, inputs2.as_ptr(), 10, std::ptr::null(), 0);

            assert_eq!(batch_result.error, CIndicatorError::Ok);
            assert_eq!(batch_result.num_outputs, 1);

            tulip_ffi_batch_result_free(batch_result);
            cvi_state_free(result.state);
        }
    }

    #[test]
    fn test_cvi_simd_by_assets() {
        use crate::common::test::build_synthetic_data;

        let data_len = 20;
        let high1: Vec<f64> = build_synthetic_data(data_len, 2);
        let low1: Vec<f64> = build_synthetic_data(data_len, 0);

        let high2: Vec<f64> = build_synthetic_data(data_len, 2);
        let low2: Vec<f64> = build_synthetic_data(data_len, 0);

        // For SIMD by assets with INPUTS=2:
        // Each asset has 2 input pointers, so we have 2 arrays of 2 elements each
        let inputs_array1 = [high1.as_ptr(), low1.as_ptr()];
        let inputs_array2 = [high2.as_ptr(), low2.as_ptr()];
        let inputs = [inputs_array1.as_ptr(), inputs_array2.as_ptr()];

        let options = [10.0];

        unsafe {
            let result = cvi_simd_by_assets(
                inputs.as_ptr(),
                2,
                data_len,
                options.as_ptr(),
                std::ptr::null(),
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, 2);
            assert_eq!(result.num_outputs, 1);

            tulip_ffi_simd_result_free(result);
        }
    }

    #[test]
    fn test_cvi_simd_by_options() {
        use crate::common::test::build_synthetic_data;

        let data_len = 50;
        let high: Vec<f64> = build_synthetic_data(data_len, 2);
        let low: Vec<f64> = build_synthetic_data(data_len, 0);

        // inputs is a single array of INPUTS pointers
        let inputs = [high.as_ptr(), low.as_ptr()];

        let options1 = [10.0];
        let options2 = [20.0];
        let options_array = [options1.as_ptr(), options2.as_ptr()];

        unsafe {
            let result = cvi_simd_by_options(
                inputs.as_ptr(),
                data_len,
                options_array.as_ptr(),
                2,
                std::ptr::null(),
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, 2);
            assert_eq!(result.num_outputs, 1);

            tulip_ffi_simd_result_free(result);
        }
    }
}
