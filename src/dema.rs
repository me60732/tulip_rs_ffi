//! extern "C" wrapper for `dema`, mirroring the core `tulip_rs` crate's
//! `Dema::indicator` / `IndicatorState::batch_indicator` interface with a
//! Tulip-Indicators-style C calling convention: `inputs` is an array of
//! `INPUTS` pointers (one per input series, each `data_len` long, in the same
//! order as `tulip_rs::indicators::dema::Dema::INFO.inputs`), `options`
//! is a flat array of `OPTIONS` values -- exactly like `ti_dema`'s
//! `double const *const *inputs, double const *options`. Output allocation
//! stays inside the indicator function (never caller-supplied).
//!
//! Parameter order convention (kept consistent across every function in
//! this crate): each pointer parameter is immediately followed by the
//! count(s) that describe it, e.g. `inputs, data_len, options, ...,
//! optional_outputs, numoptional`.
//!
//! SIMD entry points:
//! - `dema_simd_by_assets`: compute DEMA for N assets simultaneously,
//!   sharing a single options array. N must be 2, 4, 8, or 16.
//! - `dema_simd_by_options`: compute DEMA for one asset with N different
//!   option sets simultaneously. N must be 2, 4, 8, or 16.
//!
//! `#[no_mangle]` functions can't be generic, so each SIMD entry point is a
//! thin non-generic dispatcher that matches the runtime `N` against
//! {2,4,8,16} and calls a private generic `_n::<N>` helper that does the
//! real work.

use std::os::raw::c_void;

use tulip_rs::indicator_types::{Indicator, IndicatorByOptions, TIndicatorState};
use tulip_rs::indicators::dema::{Dema, IndicatorState as DemaState, INPUTS, OPTIONS};
use tulip_rs::types::IndicatorError;

use crate::common::{
    optional_outputs_slice, pack_info, pack_outputs, pack_simd_outputs, pack_states, read_inputs,
    read_simd_assets_inputs, read_simd_options, CBatchResult, CIndicatorError, CIndicatorInfo,
    CIndicatorResult, CSimdResult,
};

/// Opaque state handle returned by `dema_indicator()` and consumed by
/// `dema_batch()` / `dema_state_free().
pub type DemaStateHandle = DemaState;

/// Returns static metadata about the `dema` indicator: its name, input
/// names, option names, and (mandatory/optional) output names, mirroring
/// `Dema::INFO`.
///
/// The returned strings are leaked, process-lifetime C strings -- read them,
/// don't free them.
#[no_mangle]
pub extern "C" fn dema_info() -> CIndicatorInfo {
    pack_info(&Dema::INFO)
}

/// Returns the minimum number of bars `dema` needs to produce any output at
/// all, given `options`.
///
/// # Safety
/// `options` must point to `OPTIONS` (1) valid `f64`s.
#[no_mangle]
pub unsafe extern "C" fn dema_min_data(options: *const f64) -> usize {
    let options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    Dema::min_data(&options)
}

/// Runs `dema` over `data_len` bars.
///
/// `inputs` must point to `INPUTS` (1) pointer: `real`, `data_len` `f64`s long.
/// `options` must point to `OPTIONS` (1) value: `period`.
///
/// Returns the mandatory `dema`/`ema` outputs plus any requested optional
/// outputs and a fresh continuation state.
///
/// # Safety
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s (pass null + 0 to request no optional outputs).
#[no_mangle]
pub unsafe extern "C" fn dema_indicator(
    inputs: *const *const f64,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CIndicatorResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match Dema::indicator(&inputs, &options, optional) {
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

/// Continues a `dema` calculation from `state`, mutating it in place so
/// it's ready for the next call. `state` must have come from
/// `dema_indicator()` and not yet have been passed to `dema_state_free()`.
///
/// `inputs` must point to `INPUTS` (1) pointer: `real`, `data_len` `f64`s long.
///
/// # Safety
/// - `state` must be a live pointer previously returned by
///   `dema_indicator()`.
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn dema_batch(
    state: *mut c_void,
    inputs: *const *const f64,
    data_len: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CBatchResult {
    if state.is_null() {
        return CBatchResult::err(IndicatorError::InvalidIndicatorState);
    }
    let state = &mut *(state as *mut DemaStateHandle);

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

/// Frees a state handle returned by `dema_indicator()` (or one of the
/// `states[i]` entries from `dema_simd_by_assets()`/`dema_simd_by_options()`).
/// Call this once you're done streaming (after your last `dema_batch()` call).
///
/// # Safety
/// `state` must be a live pointer previously returned by `dema_indicator()`
/// or found in a `CSimdResult::states` array, and must not be used again
/// after this call.
#[no_mangle]
pub unsafe extern "C" fn dema_state_free(state: *mut c_void) {
    if !state.is_null() {
        drop(Box::from_raw(state as *mut DemaStateHandle));
    }
}

/// Computes DEMA for `N` assets simultaneously (SIMD), sharing a single
/// options array. `num_assets` must be 2, 4, 8, or 16 -- anything else
/// returns `InvalidInputs`.
///
/// `inputs` must point to `num_assets` pointers, each itself pointing to
/// `INPUTS` (1) pointer (`real`), `data_len` `f64`s long. `options` must point
/// to `OPTIONS` (1) shared values.
///
/// Returns a `CSimdResult` with `num_assets` results, each with its own
/// output rows and its own ordinary `dema` continuation state (reusable
/// with `dema_batch()`/`dema_state_free()`). Free each state via
/// `dema_state_free()`, then free the rest via `tulip_ffi_simd_result_free()`.
///
/// # Safety
/// - `inputs` must point to `num_assets` valid pointers, each pointing to
///   `INPUTS` valid non-null `*const f64`s, each pointing to `data_len` valid
///   `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn dema_simd_by_assets(
    inputs: *const *const *const f64,
    num_assets: usize,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    match num_assets {
        2 => dema_simd_by_assets_n::<2>(inputs, data_len, options, optional_outputs, numoptional),
        4 => dema_simd_by_assets_n::<4>(inputs, data_len, options, optional_outputs, numoptional),
        8 => dema_simd_by_assets_n::<8>(inputs, data_len, options, optional_outputs, numoptional),
        16 => dema_simd_by_assets_n::<16>(inputs, data_len, options, optional_outputs, numoptional),
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn dema_simd_by_assets_n<const N: usize>(
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

    match Dema::indicator_by_assets::<N>(&refs, &options, optional) {
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

/// Computes DEMA for one asset with `N` different option sets
/// simultaneously (SIMD). `num_option_sets` must be 2, 4, 8, or 16 --
/// anything else returns `InvalidInputs`.
///
/// `inputs` must point to `INPUTS` (1) pointer (`real`), `data_len` `f64`s
/// long, shared across all option sets. `options` must point to
/// `num_option_sets` pointers, each itself pointing to `OPTIONS` (1)
/// values.
///
/// Returns a `CSimdResult` with `num_option_sets` results, each with its
/// own output rows and its own ordinary `dema` continuation state
/// (reusable with `dema_batch()`/`dema_state_free()`). Free each state via
/// `dema_state_free()`, then free the rest via
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
pub unsafe extern "C" fn dema_simd_by_options(
    inputs: *const *const f64,
    data_len: usize,
    options: *const *const f64,
    num_option_sets: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    match num_option_sets {
        2 => dema_simd_by_options_n::<2>(inputs, data_len, options, optional_outputs, numoptional),
        4 => dema_simd_by_options_n::<4>(inputs, data_len, options, optional_outputs, numoptional),
        8 => dema_simd_by_options_n::<8>(inputs, data_len, options, optional_outputs, numoptional),
        16 => {
            dema_simd_by_options_n::<16>(inputs, data_len, options, optional_outputs, numoptional)
        }
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn dema_simd_by_options_n<const N: usize>(
    inputs: *const *const f64,
    data_len: usize,
    options: *const *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let options = read_simd_options::<N, OPTIONS>(options);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match Dema::indicator_by_options::<N>(&inputs, &options, optional) {
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
    fn test_dema_info() {
        let info = dema_info();
        assert!(info.inputs.len > 0);
        assert_eq!(info.options.len, 1);
        assert!(info.outputs.len > 0);
        assert_eq!(info.optional_outputs.len, 1);
    }

    #[test]
    fn test_dema_min_data() {
        unsafe {
            let options: [f64; OPTIONS] = [10.0];
            let min = dema_min_data(options.as_ptr());
            assert!(min > 0);
        }
    }

    #[test]
    fn test_dema_indicator() {
        use crate::common::test::build_synthetic_data;

        let data_len = 20;
        let real: Vec<f64> = build_synthetic_data(data_len);
        let inputs = [real.as_ptr()];
        let options = [10.0];

        unsafe {
            let result = dema_indicator(
                inputs.as_ptr(),
                data_len,
                options.as_ptr(),
                std::ptr::null(),
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            // Without optional outputs: only mandatory rows returned (dema)
            assert_eq!(result.num_outputs, 1);

            tulip_ffi_result_free(result);
        }
    }

    #[test]
    fn test_dema_batch() {
        use crate::common::test::build_synthetic_data;

        let data_len = 20;
        let real: Vec<f64> = build_synthetic_data(data_len);
        let inputs = [real.as_ptr()];
        let options = [10.0];

        unsafe {
            let result = dema_indicator(
                inputs.as_ptr(),
                data_len,
                options.as_ptr(),
                std::ptr::null(),
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);

            // Second batch call with exactly 10 elements (batch length)
            let real2: Vec<f64> = build_synthetic_data(10);
            let inputs2 = [real2.as_ptr()];

            let batch_result = dema_batch(result.state, inputs2.as_ptr(), 10, std::ptr::null(), 0);

            assert_eq!(batch_result.error, CIndicatorError::Ok);
            // Without optional outputs: only mandatory rows returned (dema)
            assert_eq!(batch_result.num_outputs, 1);

            tulip_ffi_batch_result_free(batch_result);
            dema_state_free(result.state);
        }
    }

    #[test]
    fn test_dema_simd_by_assets() {
        use crate::common::test::build_synthetic_data;

        let data_len = 20;
        let real1: Vec<f64> = build_synthetic_data(data_len);
        let real2: Vec<f64> = build_synthetic_data(data_len);

        // For SIMD by assets with INPUTS=1:
        // Each asset has 1 input pointer, so we have 2 arrays of 1 element each
        let inputs_array1 = [real1.as_ptr()];
        let inputs_array2 = [real2.as_ptr()];
        // Then the inputs pointer points to an array of pointers to these arrays
        let inputs = [inputs_array1.as_ptr(), inputs_array2.as_ptr()];

        let options = [10.0];

        unsafe {
            let result = dema_simd_by_assets(
                inputs.as_ptr(),
                2,
                data_len,
                options.as_ptr(),
                std::ptr::null(),
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, 2);
            // Without optional outputs: only mandatory rows returned (dema)
            assert_eq!(result.num_outputs, 1);

            tulip_ffi_simd_result_free(result);
        }
    }

    #[test]
    fn test_dema_simd_by_options() {
        use crate::common::test::build_synthetic_data;

        let data_len = 40;
        let real: Vec<f64> = build_synthetic_data(data_len);
        // inputs is a single array of INPUTS pointers
        let inputs = [real.as_ptr()];

        let options1 = [10.0];
        let options2 = [20.0];
        // For SIMD by options with OPTIONS=1:
        // Each option set has 1 value, so we have 2 arrays of 1 element each
        let options_array = [options1.as_ptr(), options2.as_ptr()];

        unsafe {
            let result = dema_simd_by_options(
                inputs.as_ptr(),
                data_len,
                options_array.as_ptr(),
                2,
                std::ptr::null(),
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, 2);
            // Without optional outputs: only mandatory rows returned (dema)
            assert_eq!(result.num_outputs, 1);

            tulip_ffi_simd_result_free(result);
        }
    }
}
