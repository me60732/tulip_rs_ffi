//! extern "C" wrapper for `bop`, mirroring the core `tulip_rs` crate's
//! `Bop::indicator` / `IndicatorState::batch_indicator` interface. See
//! `adosc.rs` for the general Tulip-Indicators-style `inputs`/`options`
//! calling convention this follows, including the parameter-order rule
//! (every pointer parameter is immediately followed by the count(s) that
//! describe it).
//!
//! SIMD entry points:
//! - `bop_simd_by_assets`: compute BOP for N assets simultaneously,
//!   sharing a single options array. N must be 2, 4, 8, or 16.
//!
//! Note: `bop` does not support SIMD by-options (no `indicator_by_options` impl).
//!
//! `#[no_mangle]` functions can't be generic, so each SIMD entry point is a
//! thin non-generic dispatcher that matches the runtime `N` against
//! {2,4,8,16} and calls a private generic `_n::<N>` helper that does the
//! real work.

use std::os::raw::c_void;

use tulip_rs::indicator_types::{Indicator, TIndicatorState};
use tulip_rs::indicators::bop::{Bop, IndicatorState as BopState, INPUTS, OPTIONS};
use tulip_rs::types::IndicatorError;

use crate::common::{
    optional_outputs_slice, pack_info, pack_outputs, pack_simd_outputs, pack_states, read_inputs,
    read_simd_assets_inputs, CBatchResult, CIndicatorError, CIndicatorInfo, CIndicatorResult,
    CSimdResult,
};

/// Opaque state handle returned by `bop_indicator()` and consumed by
/// `bop_batch()` / `bop_state_free()`.
pub type BopStateHandle = BopState;

/// Returns static metadata about the `bop` indicator: its name, input
/// names, option names, and (mandatory/optional) output names, mirroring
/// `Bop::INFO`.
///
/// The returned strings are leaked, process-lifetime C strings -- read them,
/// don't free them.
#[no_mangle]
pub extern "C" fn bop_info() -> CIndicatorInfo {
    pack_info(&Bop::INFO)
}

/// Returns the minimum number of bars `bop` needs to produce any output at
/// all, given `options`.
#[no_mangle]
pub extern "C" fn bop_min_data(_options: *const f64) -> usize {
    Bop::min_data(&[])
}

/// Runs `bop` over `data_len` bars.
///
/// `inputs` must point to `INPUTS` (4) pointers, in order:
/// `open, high, low, close`, each `data_len` `f64`s long.
/// `options` must point to `OPTIONS` (0) values (no options).
///
/// Returns the mandatory `bop` output plus any requested optional outputs
/// and a fresh continuation state.
///
/// # Safety
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s (pass null + 0 to request no optional outputs).
#[no_mangle]
pub unsafe extern "C" fn bop_indicator(
    inputs: *const *const f64,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CIndicatorResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    // OPTIONS=0 - dereference with underscore prefix to avoid unused variable warning
    let _options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match Bop::indicator(&inputs, &_options, optional) {
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

/// Continues a `bop` calculation from `state`, mutating it in place so
/// it's ready for the next call. `state` must have come from
/// `bop_indicator()` and not yet have been passed to `bop_state_free()`.
///
/// `inputs` must point to `INPUTS` (4) pointers (`open, high, low, close`),
/// each `data_len` `f64`s long.
///
/// # Safety
/// - `state` must be a live pointer previously returned by
///   `bop_indicator()`.
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn bop_batch(
    state: *mut c_void,
    inputs: *const *const f64,
    data_len: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CBatchResult {
    if state.is_null() {
        return CBatchResult::err(IndicatorError::InvalidIndicatorState);
    }
    let state = &mut *(state as *mut BopStateHandle);

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

/// Frees a state handle returned by `bop_indicator()` (or one of the
/// `states[i]` entries from `bop_simd_by_assets()`).
/// Call this once you're done streaming (after your last `bop_batch()` call).
///
/// # Safety
/// `state` must be a live pointer previously returned by `bop_indicator()`
/// or found in a `CSimdResult::states` array, and must not be used again
/// after this call.
#[no_mangle]
pub unsafe extern "C" fn bop_state_free(state: *mut c_void) {
    if !state.is_null() {
        drop(Box::from_raw(state as *mut BopStateHandle));
    }
}

/// Computes BOP for `N` assets simultaneously (SIMD), sharing a single
/// options array. `num_assets` must be 2, 4, 8, or 16 -- anything else
/// returns `InvalidInputs`.
///
/// `inputs` must point to `num_assets` pointers, each itself pointing to
/// `INPUTS` (4) pointers (`open, high, low, close`), each `data_len` `f64`s
/// long. `options` must point to `OPTIONS` (0) shared values.
///
/// Returns a `CSimdResult` with `num_assets` results, each with its own
/// output rows and its own ordinary `bop` continuation state (reusable
/// with `bop_batch()`/`bop_state_free()`). Free each state via
/// `bop_state_free()`, then free the rest via `tulip_ffi_simd_result_free()`.
///
/// # Safety
/// - `inputs` must point to `num_assets` valid pointers, each pointing to
///   `INPUTS` valid non-null `*const f64`s, each pointing to `data_len` valid
///   `f64`s.
#[no_mangle]
pub unsafe extern "C" fn bop_simd_by_assets(
    inputs: *const *const *const f64,
    num_assets: usize,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    match num_assets {
        2 => bop_simd_by_assets_n::<2>(inputs, data_len, options, optional_outputs, numoptional),
        4 => bop_simd_by_assets_n::<4>(inputs, data_len, options, optional_outputs, numoptional),
        8 => bop_simd_by_assets_n::<8>(inputs, data_len, options, optional_outputs, numoptional),
        16 => bop_simd_by_assets_n::<16>(inputs, data_len, options, optional_outputs, numoptional),
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn bop_simd_by_assets_n<const N: usize>(
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
    // OPTIONS=0
    let _options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match Bop::indicator_by_assets::<N>(&refs, &_options, optional) {
        Ok((results, states)) => {
            let (outputs, output_lens, num_outputs, num_results) = pack_simd_outputs(results, optional);
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
    fn test_bop_info() {
        let info = bop_info();
        assert!(info.inputs.len > 0);
        assert_eq!(info.options.len, 0);
        assert!(info.outputs.len > 0);
        assert_eq!(info.optional_outputs.len, 0);
    }

    #[test]
    fn test_bop_min_data() {
        let min = bop_min_data(std::ptr::null());
        assert!(min > 0);
    }

    #[test]
    fn test_bop_indicator() {
        unsafe {
            // Create test data
            let data_len = 20;
            let open: Vec<f64> = build_synthetic_data(data_len, 1);
            let high: Vec<f64> = build_synthetic_data(data_len, 2);
            let low: Vec<f64> = build_synthetic_data(data_len, 0);
            let close: Vec<f64> = build_synthetic_data(data_len, 1);

            let inputs = [open.as_ptr(), high.as_ptr(), low.as_ptr(), close.as_ptr()];
            // OPTIONS=0, create empty options array
            let _options: [f64; OPTIONS] = [];

            let result = bop_indicator(
                inputs.as_ptr(),
                data_len,
                _options.as_ptr(),
                std::ptr::null(),
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_outputs, 1);

            // Check output length
            let lens = std::slice::from_raw_parts(result.output_lens, result.num_outputs);
            assert_eq!(lens[0], data_len);

            // Free outputs and state
            tulip_ffi_result_free(result);
        }
    }

    #[test]
    fn test_bop_batch() {
        unsafe {
            let data_len = 20;
            let open: Vec<f64> = build_synthetic_data(data_len, 1);
            let high: Vec<f64> = build_synthetic_data(data_len, 2);
            let low: Vec<f64> = build_synthetic_data(data_len, 0);
            let close: Vec<f64> = build_synthetic_data(data_len, 1);

            let inputs = [open.as_ptr(), high.as_ptr(), low.as_ptr(), close.as_ptr()];
            // OPTIONS=0
            let _options: [f64; OPTIONS] = [];

            // First call to indicator
            let result = bop_indicator(
                inputs.as_ptr(),
                data_len,
                _options.as_ptr(),
                std::ptr::null(),
                0,
            );
            assert_eq!(result.error, CIndicatorError::Ok);

            // Second call to batch with new data
            let more_open: Vec<f64> = build_synthetic_data(10, 1);
            let more_high: Vec<f64> = build_synthetic_data(10, 2);
            let more_low: Vec<f64> = build_synthetic_data(10, 0);
            let more_close: Vec<f64> = build_synthetic_data(10, 1);

            let new_inputs = [
                more_open.as_ptr(),
                more_high.as_ptr(),
                more_low.as_ptr(),
                more_close.as_ptr(),
            ];

            let batch_result =
                bop_batch(result.state, new_inputs.as_ptr(), 10, std::ptr::null(), 0);
            assert_eq!(batch_result.error, CIndicatorError::Ok);

            // Free outputs
            tulip_ffi_batch_result_free(batch_result);
            bop_state_free(result.state);
        }
    }

    #[test]
    fn test_bop_simd_by_assets() {
        unsafe {
            let data_len = 20;
            let num_assets = 2;

            // Create inputs for 2 assets
            let open1: Vec<f64> = build_synthetic_data(data_len, 1);
            let high1: Vec<f64> = build_synthetic_data(data_len, 2);
            let low1: Vec<f64> = build_synthetic_data(data_len, 0);
            let close1: Vec<f64> = build_synthetic_data(data_len, 1);

            let open2: Vec<f64> = build_synthetic_data(data_len, 2);
            let high2: Vec<f64> = build_synthetic_data(data_len, 3);
            let low2: Vec<f64> = build_synthetic_data(data_len, 1);
            let close2: Vec<f64> = build_synthetic_data(data_len, 2);

            let inputs1 = [
                open1.as_ptr(),
                high1.as_ptr(),
                low1.as_ptr(),
                close1.as_ptr(),
            ];
            let inputs2 = [
                open2.as_ptr(),
                high2.as_ptr(),
                low2.as_ptr(),
                close2.as_ptr(),
            ];

            let asset_inputs = [inputs1.as_ptr(), inputs2.as_ptr()];
            // OPTIONS=0
            let _options: [f64; OPTIONS] = [];

            let result = bop_simd_by_assets(
                asset_inputs.as_ptr(),
                num_assets,
                data_len,
                _options.as_ptr(),
                std::ptr::null(),
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, num_assets);
            assert_eq!(result.num_outputs, 1);

            // Free outputs and states
            tulip_ffi_simd_result_free(result);
        }
    }
}
