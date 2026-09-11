//! extern "C" wrapper for `ccfisher`, mirroring the core `tulip_rs` crate's
//! `CcFisher::indicator` / `IndicatorState::batch_indicator` interface. See
//! `adosc.rs` for the general Tulip-Indicators-style `inputs`/`options`
//! calling convention this follows, including the parameter-order rule
//! (every pointer parameter is immediately followed by the count(s) that
//! describe it).
//!
//! SIMD entry points:
//! - `ccfisher_simd_by_assets`: compute CCFisher for N assets simultaneously,
//!   sharing a single options array. N must be 2, 4, 8, or 16.
//! - `ccfisher_simd_by_options`: compute CCFisher for one asset with N different
//!   option sets simultaneously. N must be 2, 4, 8, or 16.
//!
//! `#[no_mangle]` functions can't be generic, so each SIMD entry point is a
//! thin non-generic dispatcher that matches the runtime `N` against
//! {2,4,8,16} and calls a private generic `_n::<N>` helper that does the
//! real work.

use std::os::raw::c_void;

use tulip_rs::indicator_types::{Indicator, IndicatorByOptions, TIndicatorState};
use tulip_rs::indicators::ccfisher::{CcFisher, IndicatorState as CcFisherState, INPUTS, OPTIONS};
use tulip_rs::types::IndicatorError;

use crate::common::{
    optional_outputs_slice, pack_info, pack_outputs, pack_simd_outputs, pack_states, read_inputs,
    read_simd_assets_inputs, read_simd_options, CBatchResult, CIndicatorError, CIndicatorInfo,
    CIndicatorResult, CSimdResult,
};

/// Opaque state handle returned by `ccfisher_indicator()` and consumed by
/// `ccfisher_batch()` / `ccfisher_state_free().
pub type CcFisherStateHandle = CcFisherState;

/// Returns static metadata about the `ccfisher` indicator: its name, input
/// names, option names, and (mandatory/optional) output names, mirroring
/// `CcFisher::INFO`.
///
/// The returned strings are leaked, process-lifetime C strings -- read them,
/// don't free them.
#[no_mangle]
pub extern "C" fn ccfisher_info() -> CIndicatorInfo {
    pack_info(&CcFisher::INFO)
}

/// Returns the minimum number of bars `ccfisher` needs to produce any output at
/// all, given `options`.
///
/// # Safety
/// `options` must point to `OPTIONS` (1) valid `f64`s.
#[no_mangle]
pub unsafe extern "C" fn ccfisher_min_data(options: *const f64) -> usize {
    let options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    CcFisher::min_data(&options)
}

/// Runs `ccfisher` over `data_len` bars.
///
/// `inputs` must point to `INPUTS` (1) pointer: `real`, `data_len` `f64`s long.
/// `options` must point to `OPTIONS` (1) value: `alpha`.
///
/// Returns the mandatory `fisher`/`signal` outputs plus any requested optional
/// outputs (`trendmode`, `cycle`, `peak`, in that fixed order) and a fresh
/// continuation state.
///
/// # Safety
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s (pass null + 0 to request no optional outputs).
#[no_mangle]
pub unsafe extern "C" fn ccfisher_indicator(
    inputs: *const *const f64,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CIndicatorResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match CcFisher::indicator(&inputs, &options, optional) {
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

/// Continues a `ccfisher` calculation from `state`, mutating it in place so
/// it's ready for the next call. `state` must have come from
/// `ccfisher_indicator()` and not yet have been passed to `ccfisher_state_free()`.
///
/// `inputs` must point to `INPUTS` (1) pointer: `real`, `data_len` `f64`s long.
///
/// # Safety
/// - `state` must be a live pointer previously returned by
///   `ccfisher_indicator()`.
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn ccfisher_batch(
    state: *mut c_void,
    inputs: *const *const f64,
    data_len: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CBatchResult {
    if state.is_null() {
        return CBatchResult::err(IndicatorError::InvalidIndicatorState);
    }
    let state = &mut *(state as *mut CcFisherStateHandle);

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

/// Frees a state handle returned by `ccfisher_indicator()` (or one of the
/// `states[i]` entries from `ccfisher_simd_by_assets()`/`ccfisher_simd_by_options()`).
/// Call this once you're done streaming (after your last `ccfisher_batch()` call).
///
/// # Safety
/// `state` must be a live pointer previously returned by `ccfisher_indicator()`
/// or found in a `CSimdResult::states` array, and must not be used again
/// after this call.
#[no_mangle]
pub unsafe extern "C" fn ccfisher_state_free(state: *mut c_void) {
    if !state.is_null() {
        drop(Box::from_raw(state as *mut CcFisherStateHandle));
    }
}

/// Computes CCFisher for `N` assets simultaneously (SIMD), sharing a single
/// options array. `num_assets` must be 2, 4, 8, or 16 -- anything else
/// returns `InvalidInputs`.
///
/// `inputs` must point to `num_assets` pointers, each itself pointing to
/// `INPUTS` (1) pointer (`real`), `data_len` `f64`s long. `options` must point
/// to `OPTIONS` (1) shared values.
///
/// Returns a `CSimdResult` with `num_assets` results, each with its own
/// output rows and its own ordinary `ccfisher` continuation state (reusable
/// with `ccfisher_batch()`/`ccfisher_state_free()`). Free each state via
/// `ccfisher_state_free()`, then free the rest via `tulip_ffi_simd_result_free()`.
///
/// # Safety
/// - `inputs` must point to `num_assets` valid pointers, each pointing to
///   `INPUTS` valid non-null `*const f64`s, each pointing to `data_len` valid
///   `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn ccfisher_simd_by_assets(
    inputs: *const *const *const f64,
    num_assets: usize,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    match num_assets {
        2 => {
            ccfisher_simd_by_assets_n::<2>(inputs, data_len, options, optional_outputs, numoptional)
        }
        4 => {
            ccfisher_simd_by_assets_n::<4>(inputs, data_len, options, optional_outputs, numoptional)
        }
        8 => {
            ccfisher_simd_by_assets_n::<8>(inputs, data_len, options, optional_outputs, numoptional)
        }
        16 => ccfisher_simd_by_assets_n::<16>(
            inputs,
            data_len,
            options,
            optional_outputs,
            numoptional,
        ),
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn ccfisher_simd_by_assets_n<const N: usize>(
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

    match CcFisher::indicator_by_assets::<N>(&refs, &options, optional) {
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

/// Computes CCFisher for one asset with `N` different option sets
/// simultaneously (SIMD). `num_option_sets` must be 2, 4, 8, or 16 --
/// anything else returns `InvalidInputs`.
///
/// `inputs` must point to `INPUTS` (1) pointer (`real`), `data_len` `f64`s
/// long, shared across all option sets. `options` must point to
/// `num_option_sets` pointers, each itself pointing to `OPTIONS` (1)
/// values.
///
/// Returns a `CSimdResult` with `num_option_sets` results, each with its
/// own output rows and its own ordinary `ccfisher` continuation state
/// (reusable with `ccfisher_batch()`/`ccfisher_state_free()`). Free each state via
/// `ccfisher_state_free()`, then free the rest via
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
pub unsafe extern "C" fn ccfisher_simd_by_options(
    inputs: *const *const f64,
    data_len: usize,
    options: *const *const f64,
    num_option_sets: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    match num_option_sets {
        2 => ccfisher_simd_by_options_n::<2>(
            inputs,
            data_len,
            options,
            optional_outputs,
            numoptional,
        ),
        4 => ccfisher_simd_by_options_n::<4>(
            inputs,
            data_len,
            options,
            optional_outputs,
            numoptional,
        ),
        8 => ccfisher_simd_by_options_n::<8>(
            inputs,
            data_len,
            options,
            optional_outputs,
            numoptional,
        ),
        16 => ccfisher_simd_by_options_n::<16>(
            inputs,
            data_len,
            options,
            optional_outputs,
            numoptional,
        ),
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn ccfisher_simd_by_options_n<const N: usize>(
    inputs: *const *const f64,
    data_len: usize,
    options: *const *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let options = read_simd_options::<N, OPTIONS>(options);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match CcFisher::indicator_by_options::<N>(&inputs, &options, optional) {
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
    fn test_ccfisher_info() {
        let info = ccfisher_info();
        assert!(info.inputs.len > 0);
        assert_eq!(info.options.len, 1);
        assert!(info.outputs.len > 0);
        assert_eq!(info.optional_outputs.len, 3);
    }

    #[test]
    fn test_ccfisher_min_data() {
        unsafe {
            let options: [f64; OPTIONS] = [0.07];
            let min = ccfisher_min_data(options.as_ptr());
            assert!(min > 0);
        }
    }

    #[test]
    fn test_ccfisher_indicator() {
        unsafe {
            // Create test data - need at least 56 bars for warmup
            let data_len = 60;
            let real: Vec<f64> = build_synthetic_data(data_len, 0);
            let inputs = [real.as_ptr()];
            let options: [f64; OPTIONS] = [0.07];

            let result = ccfisher_indicator(
                inputs.as_ptr(),
                data_len,
                options.as_ptr(),
                std::ptr::null(),
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            // Without optional outputs: only non-empty rows returned (fisher=5, signal=5)
            assert_eq!(result.num_outputs, 2);

            // Check output lengths (output_length = data_len - min_data + 1)
            let lens = std::slice::from_raw_parts(result.output_lens, result.num_outputs);

            assert_eq!(lens[0], 5); // fisher (mandatory)
            assert_eq!(lens[1], 5); // signal (mandatory)

            // Free outputs and state
            tulip_ffi_result_free(result);
        }
    }

    #[test]
    fn test_ccfisher_batch() {
        unsafe {
            let data_len = 60;
            let real: Vec<f64> = build_synthetic_data(data_len, 0);
            let inputs = [real.as_ptr()];
            let options: [f64; OPTIONS] = [0.07];

            // First call to indicator
            let result = ccfisher_indicator(
                inputs.as_ptr(),
                data_len,
                options.as_ptr(),
                std::ptr::null(),
                0,
            );
            assert_eq!(result.error, CIndicatorError::Ok);

            // Second call to batch with new data
            let more_data: Vec<f64> = build_synthetic_data(10, 0);
            let new_inputs = [more_data.as_ptr()];

            let batch_result =
                ccfisher_batch(result.state, new_inputs.as_ptr(), 10, std::ptr::null(), 0);
            assert_eq!(batch_result.error, CIndicatorError::Ok);

            // Free outputs
            tulip_ffi_batch_result_free(batch_result);
            ccfisher_state_free(result.state);
        }
    }

    #[test]
    fn test_ccfisher_simd_by_assets() {
        unsafe {
            let data_len = 60;
            let num_assets = 2;

            // Create inputs for 2 assets
            let real1: Vec<f64> = build_synthetic_data(data_len, 0);
            let real2: Vec<f64> = build_synthetic_data(data_len, 2);

            let inputs1 = [real1.as_ptr()];
            let inputs2 = [real2.as_ptr()];

            let asset_inputs = [inputs1.as_ptr(), inputs2.as_ptr()];
            let options: [f64; OPTIONS] = [0.07];

            let result = ccfisher_simd_by_assets(
                asset_inputs.as_ptr(),
                num_assets,
                data_len,
                options.as_ptr(),
                std::ptr::null(),
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, num_assets);
            // Without optional outputs: only non-empty rows returned (fisher, signal)
            assert_eq!(result.num_outputs, 2);

            // Free outputs and states
            tulip_ffi_simd_result_free(result);
        }
    }

    #[test]
    fn test_ccfisher_simd_by_options() {
        unsafe {
            let data_len = 60;
            let num_option_sets = 2;

            let real: Vec<f64> = build_synthetic_data(data_len, 0);
            let inputs = [real.as_ptr()];

            // Two different option sets
            let options1: [f64; OPTIONS] = [0.07];
            let options2: [f64; OPTIONS] = [0.1];

            let option_sets = [options1.as_ptr(), options2.as_ptr()];

            let result = ccfisher_simd_by_options(
                inputs.as_ptr(),
                data_len,
                option_sets.as_ptr(),
                num_option_sets,
                std::ptr::null(),
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, num_option_sets);
            // Without optional outputs: only non-empty rows returned (fisher, signal)
            assert_eq!(result.num_outputs, 2);

            // Free outputs and states
            tulip_ffi_simd_result_free(result);
        }
    }
}
