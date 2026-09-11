//! extern "C" wrapper for `vortex`, mirroring the core `tulip_rs` crate's
//! `Vortex::indicator` / `IndicatorState::batch_indicator` interface with a
//! Tulip-Indicators-style C calling convention: `inputs` is an array of
//! `INPUTS` pointers (one per input series, each `data_len` long, in the same
//! order as `tulip_rs::indicators::vortex::Vortex::INFO.inputs`), `options`
//! is a flat array of `OPTIONS` values -- exactly like `ti_vortex`'s
//! `double const *const *inputs, double const *options`. Output allocation
//! stays inside the indicator function (never caller-supplied).
//!
//! Parameter order convention (kept consistent across every function in
//! this crate): each pointer parameter is immediately followed by the
//! count(s) that describe it, e.g. `inputs, data_len, options, ...,
//! optional_outputs, numoptional`.
//!
//! SIMD entry points:
//! - `vortex_simd_by_assets`: compute Vortex for N assets simultaneously,
//!   sharing a single options array. N must be 2, 4, 8, or 16.
//! - `vortex_simd_by_options`: compute Vortex for N option sets simultaneously,
//!   using the same inputs data. N must be 2, 4, 8, or 16.

use std::os::raw::c_void;

use tulip_rs::indicator_types::{Indicator, IndicatorByOptions, TIndicatorState};
use tulip_rs::indicators::vortex::{IndicatorState as VortexState, Vortex, INPUTS, OPTIONS};
use tulip_rs::types::IndicatorError;

use crate::common::{
    optional_outputs_slice, pack_info, pack_outputs, pack_simd_outputs, pack_states, read_inputs,
    read_simd_assets_inputs, read_simd_options, CBatchResult, CIndicatorError, CIndicatorInfo,
    CIndicatorResult, CSimdResult,
};

/// Opaque state handle returned by `vortex_indicator()` and consumed by
/// `vortex_batch()` / `vortex_state_free()`.
pub type VortexStateHandle = VortexState;

/// Returns static metadata about the `vortex` indicator: its name, input
/// names, option names, and (mandatory/optional) output names, mirroring
/// `Vortex::INFO`.
///
/// The returned strings are leaked, process-lifetime C strings -- read them,
/// don't free them.
#[no_mangle]
pub extern "C" fn vortex_info() -> CIndicatorInfo {
    pack_info(&Vortex::INFO)
}

/// Returns the minimum number of bars `vortex` needs to produce any output at
/// all, given `options`.
#[no_mangle]
pub unsafe extern "C" fn vortex_min_data(options: *const f64) -> usize {
    let _options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    Vortex::min_data(&_options)
}

/// Runs `vortex` over `data_len` bars.
///
/// `inputs` must point to `INPUTS` (3) pointers, in order:
/// `high, low, close`, each `data_len` `f64`s long.
/// `options` must point to `OPTIONS` (1) values: `period`.
///
/// Returns the mandatory `vortex` outputs (`vi_up, vi_down`) plus any requested optional outputs
/// (`tr`) and a fresh continuation state.
///
/// # Safety
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
#[no_mangle]
pub unsafe extern "C" fn vortex_indicator(
    inputs: *const *const f64,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CIndicatorResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let _options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match Vortex::indicator(&inputs, &_options, optional) {
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

/// Continues a `vortex` calculation from `state`, mutating it in place so
/// it's ready for the next call. `state` must have come from
/// `vortex_indicator()` and not yet have been passed to `vortex_state_free()`.
///
/// `inputs` must point to `INPUTS` (3) pointers (`high, low, close`), each `data_len` `f64`s long.
///
/// # Safety
/// - `state` must be a live pointer previously returned by
///   `vortex_indicator()`.
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
#[no_mangle]
pub unsafe extern "C" fn vortex_batch(
    state: *mut c_void,
    inputs: *const *const f64,
    data_len: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CBatchResult {
    if state.is_null() {
        return CBatchResult::err(IndicatorError::InvalidIndicatorState);
    }
    let state = &mut *(state as *mut VortexStateHandle);

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

/// Frees a state handle returned by `vortex_indicator()` (or one of the
/// `states[i]` entries from `vortex_simd_by_assets()` or `vortex_simd_by_options()`).
/// Call this once you're done streaming (after your last `vortex_batch()` call).
///
/// # Safety
/// `state` must be a live pointer previously returned by `vortex_indicator()`
/// or found in a `CSimdResult::states` array, and must not be used again
/// after this call.
#[no_mangle]
pub unsafe extern "C" fn vortex_state_free(state: *mut c_void) {
    if !state.is_null() {
        drop(Box::from_raw(state as *mut VortexStateHandle));
    }
}

/// Computes Vortex for `N` assets simultaneously (SIMD), sharing a single
/// options array. `num_assets` must be 2, 4, 8, or 16 -- anything else
/// returns `InvalidInputs`.
///
/// `inputs` must point to `num_assets` pointers, each itself pointing to
/// `INPUTS` (3) pointers (`high, low, close`), each `data_len` `f64`s long.
/// `options` must point to `OPTIONS` (1) shared values: `period`.
///
/// Returns a `CSimdResult` with `num_assets` results, each with its own
/// output rows and its own ordinary `vortex` continuation state (reusable
/// with `vortex_batch()`/`vortex_state_free()`). Free each state via
/// `vortex_state_free()`, then free the rest via `tulip_ffi_simd_result_free()`.
///
/// # Safety
/// - `inputs` must point to `num_assets` valid pointers, each pointing to
///   `INPUTS` valid non-null `*const f64`s, each pointing to `data_len` valid
///   `f64`s.
#[no_mangle]
pub unsafe extern "C" fn vortex_simd_by_assets(
    inputs: *const *const *const f64,
    num_assets: usize,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    match num_assets {
        2 => vortex_simd_by_assets_n::<2>(inputs, data_len, options, optional_outputs, numoptional),
        4 => vortex_simd_by_assets_n::<4>(inputs, data_len, options, optional_outputs, numoptional),
        8 => vortex_simd_by_assets_n::<8>(inputs, data_len, options, optional_outputs, numoptional),
        16 => {
            vortex_simd_by_assets_n::<16>(inputs, data_len, options, optional_outputs, numoptional)
        }
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn vortex_simd_by_assets_n<const N: usize>(
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
    let _options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match Vortex::indicator_by_assets::<N>(&refs, &_options, optional) {
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

/// Computes Vortex for `N` option sets simultaneously (SIMD), using the same
/// inputs data. `num_option_sets` must be 2, 4, 8, or 16 -- anything else
/// returns `InvalidInputs`.
///
/// `inputs` must point to `INPUTS` (3) pointers (`high, low, close`), each `data_len` `f64`s long.
/// `options` must point to `num_option_sets` arrays of `OPTIONS` (1) values: `period`.
///
/// Returns a `CSimdResult` with `num_option_sets` results, each with its own
/// output rows and its own ordinary `vortex` continuation state (reusable
/// with `vortex_batch()`/`vortex_state_free()`). Free each state via
/// `vortex_state_free()`, then free the rest via `tulip_ffi_simd_result_free()`.
///
/// # Safety
/// - `inputs` must point to `INPUTS` valid non-null `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `options` must point to `num_option_sets` valid arrays of `OPTIONS` `f64`s.
#[no_mangle]
pub unsafe extern "C" fn vortex_simd_by_options(
    inputs: *const *const f64,
    data_len: usize,
    options: *const *const f64,
    num_option_sets: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    match num_option_sets {
        2 => {
            vortex_simd_by_options_n::<2>(inputs, data_len, options, optional_outputs, numoptional)
        }
        4 => {
            vortex_simd_by_options_n::<4>(inputs, data_len, options, optional_outputs, numoptional)
        }
        8 => {
            vortex_simd_by_options_n::<8>(inputs, data_len, options, optional_outputs, numoptional)
        }
        16 => {
            vortex_simd_by_options_n::<16>(inputs, data_len, options, optional_outputs, numoptional)
        }
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn vortex_simd_by_options_n<const N: usize>(
    inputs: *const *const f64,
    data_len: usize,
    options: *const *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let options_arr: [&[f64; OPTIONS]; N] = read_simd_options::<N, OPTIONS>(options);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match Vortex::indicator_by_options::<N>(&inputs, &options_arr, optional) {
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
    fn test_vortex_info() {
        let info = vortex_info();
        assert!(info.inputs.len > 0);
        assert_eq!(info.options.len, 1);
        assert!(info.outputs.len > 0);
        assert_eq!(info.optional_outputs.len, 1);
    }

    #[test]
    fn test_vortex_min_data() {
        unsafe {
            let options: [f64; OPTIONS] = [14.0];
            let min = vortex_min_data(options.as_ptr());
            assert!(min > 0);
        }
    }

    #[test]
    fn test_vortex_indicator() {
        unsafe {
            let data_len = 60;
            let high: Vec<f64> = (1..=data_len).map(|i| i as f64 + 102.0).collect();
            let low: Vec<f64> = (1..=data_len).map(|i| i as f64 + 98.0).collect();
            let close: Vec<f64> = (1..=data_len).map(|i| i as f64 + 100.0).collect();

            // Bind inputs to local first (dangling temp segfault rule)
            let inputs_ptr: [*const f64; INPUTS] = [high.as_ptr(), low.as_ptr(), close.as_ptr()];
            let inputs = inputs_ptr.as_ptr();
            let options: [f64; OPTIONS] = [14.0];
            let options_ptr = options.as_ptr();

            // Request all optional outputs
            let optional_outputs: [bool; 1] = [true];

            let result =
                vortex_indicator(inputs, data_len, options_ptr, optional_outputs.as_ptr(), 1);

            assert_eq!(result.error, CIndicatorError::Ok);
            // Mandatory: vi_up, vi_down. Optional: tr.
            assert_eq!(result.num_outputs, 3);
            tulip_ffi_result_free(result);
        }
    }

    #[test]
    fn test_vortex_batch() {
        unsafe {
            let data_len = 60;
            let high: Vec<f64> = (1..=data_len).map(|i| i as f64 + 102.0).collect();
            let low: Vec<f64> = (1..=data_len).map(|i| i as f64 + 98.0).collect();
            let close: Vec<f64> = (1..=data_len).map(|i| i as f64 + 100.0).collect();

            // Bind inputs to local first (dangling temp segfault rule)
            let inputs_ptr: [*const f64; INPUTS] = [high.as_ptr(), low.as_ptr(), close.as_ptr()];
            let inputs = inputs_ptr.as_ptr();
            let options: [f64; OPTIONS] = [14.0];
            let options_ptr = options.as_ptr();

            // Request all optional outputs
            let optional_outputs: [bool; 1] = [true];

            let result =
                vortex_indicator(inputs, data_len, options_ptr, optional_outputs.as_ptr(), 1);

            assert_eq!(result.error, CIndicatorError::Ok);
            let state = result.state;

            // Second batch with more data
            let extra_data_len = 10;
            let high_extra: Vec<f64> = (61..=70).map(|i| i as f64 + 102.0).collect();
            let low_extra: Vec<f64> = (61..=70).map(|i| i as f64 + 98.0).collect();
            let close_extra: Vec<f64> = (61..=70).map(|i| i as f64 + 100.0).collect();

            let inputs_ptr_extra: [*const f64; INPUTS] = [
                high_extra.as_ptr(),
                low_extra.as_ptr(),
                close_extra.as_ptr(),
            ];
            let inputs_extra = inputs_ptr_extra.as_ptr();

            let batch_result = vortex_batch(
                state,
                inputs_extra,
                extra_data_len,
                optional_outputs.as_ptr(),
                1,
            );

            assert_eq!(batch_result.error, CIndicatorError::Ok);
            assert_eq!(batch_result.num_outputs, 3);

            tulip_ffi_batch_result_free(batch_result);
            vortex_state_free(state);
        }
    }

    #[test]
    fn test_vortex_simd_by_assets() {
        unsafe {
            let data_len = 60;
            let high: Vec<f64> = (1..=data_len).map(|i| i as f64 + 102.0).collect();
            let low: Vec<f64> = (1..=data_len).map(|i| i as f64 + 98.0).collect();
            let close: Vec<f64> = (1..=data_len).map(|i| i as f64 + 100.0).collect();

            let high2: Vec<f64> = (21..=80).map(|i| i as f64 + 105.0).collect();
            let low2: Vec<f64> = (21..=80).map(|i| i as f64 + 95.0).collect();
            let close2: Vec<f64> = (21..=80).map(|i| i as f64 + 100.0).collect();

            // Create a proper nested array of asset pointers
            // For SIMD by assets with INPUTS=3:
            // - Each asset has 3 input pointers: [high, low, close]
            // - The inputs pointer is to an array of N asset arrays
            let assets_array: [[*const f64; INPUTS]; 2] = [
                [high.as_ptr(), low.as_ptr(), close.as_ptr()],
                [high2.as_ptr(), low2.as_ptr(), close2.as_ptr()],
            ];
            // Cast to the expected pointer type: an array of 2 pointers, each
            // pointing to one asset's array of INPUTS data pointers.
            let assets_ptrs_arr: [*const *const f64; 2] =
                [assets_array[0].as_ptr(), assets_array[1].as_ptr()];
            let assets_ptrs = assets_ptrs_arr.as_ptr();

            let options: [f64; OPTIONS] = [14.0];
            let options_ptr = options.as_ptr();

            // Request all optional outputs
            let optional_outputs: [bool; 1] = [true];

            let result = vortex_simd_by_assets(
                assets_ptrs,
                2,
                data_len,
                options_ptr,
                optional_outputs.as_ptr(),
                1,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, 2);
            // Mandatory: vi_up, vi_down. Optional: tr.
            assert_eq!(result.num_outputs, 3);

            // Free individual states using pointer arithmetic
            vortex_state_free(result.states.wrapping_add(0).read());
            vortex_state_free(result.states.wrapping_add(1).read());

            tulip_ffi_simd_result_free(result);
        }
    }

    #[test]
    fn test_vortex_simd_by_options() {
        unsafe {
            let data_len = 60;
            let high: Vec<f64> = (1..=data_len).map(|i| i as f64 + 102.0).collect();
            let low: Vec<f64> = (1..=data_len).map(|i| i as f64 + 98.0).collect();
            let close: Vec<f64> = (1..=data_len).map(|i| i as f64 + 100.0).collect();

            // Bind inputs to local first
            let inputs_ptr: [*const f64; INPUTS] = [high.as_ptr(), low.as_ptr(), close.as_ptr()];
            let inputs = inputs_ptr.as_ptr();

            // Create two option sets as separate arrays
            let options1: [f64; OPTIONS] = [14.0];
            let options2: [f64; OPTIONS] = [20.0];

            // Create array of pointers to the option arrays
            let options_ptrs_arr: [*const f64; 2] = [options1.as_ptr(), options2.as_ptr()];
            let options_ptr = options_ptrs_arr.as_ptr();

            // Request all optional outputs
            let optional_outputs: [bool; 1] = [true];

            let result = vortex_simd_by_options(
                inputs,
                data_len,
                options_ptr,
                2,
                optional_outputs.as_ptr(),
                1,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, 2);
            assert_eq!(result.num_outputs, 3); // vi_up, vi_down, tr (all requested via optional)

            // Free individual states using pointer arithmetic
            vortex_state_free(result.states.wrapping_add(0).read());
            vortex_state_free(result.states.wrapping_add(1).read());

            tulip_ffi_simd_result_free(result);
        }
    }
}
