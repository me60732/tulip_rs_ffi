//! extern "C" wrapper for `volatility`, mirroring the core `tulip_rs` crate's
//! `Volatility::indicator` / `IndicatorState::batch_indicator` interface with a
//! Tulip-Indicators-style C calling convention: `inputs` is an array of
//! `INPUTS` pointers (one per input series, each `data_len` long, in the same
//! order as `tulip_rs::indicators::volatility::Volatility::INFO.inputs`), `options`
//! is a flat array of `OPTIONS` values -- exactly like `ti_volatility`'s
//! `double const *const *inputs, double const *options`. Output allocation
//! stays inside the indicator function (never caller-supplied).
//!
//! Parameter order convention (kept consistent across every function in
//! this crate): each pointer parameter is immediately followed by the
//! count(s) that describe it, e.g. `inputs, data_len, options, ...,
//! optional_outputs, numoptional`.
//!
//! SIMD entry points:
//! - `volatility_simd_by_assets`: compute Volatility for N assets simultaneously,
//!   sharing a single options array. N must be 2, 4, 8, or 16.
//! - `volatility_simd_by_options`: compute Volatility for N option sets simultaneously,
//!   using the same inputs data. N must be 2, 4, 8, or 16.

use std::os::raw::c_void;

use tulip_rs::indicator_types::{Indicator, IndicatorByOptions, TIndicatorState};
use tulip_rs::indicators::volatility::{
    IndicatorState as VolatilityState, Volatility, INPUTS, OPTIONS,
};
use tulip_rs::types::IndicatorError;

use crate::common::{
    optional_outputs_slice, pack_info, pack_outputs, pack_simd_outputs, pack_states, read_inputs,
    read_simd_assets_inputs, read_simd_options, CBatchResult, CIndicatorError, CIndicatorInfo,
    CIndicatorResult, CSimdResult,
};

/// Opaque state handle returned by `volatility_indicator()` and consumed by
/// `volatility_batch()` / `volatility_state_free()`.
pub type VolatilityStateHandle = VolatilityState;

/// Returns static metadata about the `volatility` indicator: its name, input
/// names, option names, and (mandatory/optional) output names, mirroring
/// `Volatility::INFO`.
///
/// The returned strings are leaked, process-lifetime C strings -- read them,
/// don't free them.
#[no_mangle]
pub extern "C" fn volatility_info() -> CIndicatorInfo {
    pack_info(&Volatility::INFO)
}

/// Returns the minimum number of bars `volatility` needs to produce any output at
/// all, given `options`.
#[no_mangle]
pub unsafe extern "C" fn volatility_min_data(options: *const f64) -> usize {
    let _options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    Volatility::min_data(&_options)
}

/// Runs `volatility` over `data_len` bars.
///
/// `inputs` must point to `INPUTS` (1) pointers, in order:
/// `real`, each `data_len` `f64`s long.
/// `options` must point to `OPTIONS` (1) values: `period`.
///
/// Returns the mandatory `volatility` output plus any requested optional outputs
/// (none for this indicator) and a fresh continuation state.
///
/// # Safety
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
#[no_mangle]
pub unsafe extern "C" fn volatility_indicator(
    inputs: *const *const f64,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CIndicatorResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let _options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match Volatility::indicator(&inputs, &_options, optional) {
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

/// Continues a `volatility` calculation from `state`, mutating it in place so
/// it's ready for the next call. `state` must have come from
/// `volatility_indicator()` and not yet have been passed to `volatility_state_free()`.
///
/// `inputs` must point to `INPUTS` (1) pointers (`real`), each `data_len` `f64`s long.
///
/// # Safety
/// - `state` must be a live pointer previously returned by
///   `volatility_indicator()`.
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
#[no_mangle]
pub unsafe extern "C" fn volatility_batch(
    state: *mut c_void,
    inputs: *const *const f64,
    data_len: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CBatchResult {
    if state.is_null() {
        return CBatchResult::err(IndicatorError::InvalidIndicatorState);
    }
    let state = &mut *(state as *mut VolatilityStateHandle);

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

/// Frees a state handle returned by `volatility_indicator()` (or one of the
/// `states[i]` entries from `volatility_simd_by_assets()` or `volatility_simd_by_options()`).
/// Call this once you're done streaming (after your last `volatility_batch()` call).
///
/// # Safety
/// `state` must be a live pointer previously returned by `volatility_indicator()`
/// or found in a `CSimdResult::states` array, and must not be used again
/// after this call.
#[no_mangle]
pub unsafe extern "C" fn volatility_state_free(state: *mut c_void) {
    if !state.is_null() {
        drop(Box::from_raw(state as *mut VolatilityStateHandle));
    }
}

/// Computes Volatility for `N` assets simultaneously (SIMD), sharing a single
/// options array. `num_assets` must be 2, 4, 8, or 16 -- anything else
/// returns `InvalidInputs`.
///
/// `inputs` must point to `num_assets` pointers, each itself pointing to
/// `INPUTS` (1) pointers (`real`), each `data_len` `f64`s long.
/// `options` must point to `OPTIONS` (1) shared values: `period`.
///
/// Returns a `CSimdResult` with `num_assets` results, each with its own
/// output rows and its own ordinary `volatility` continuation state (reusable
/// with `volatility_batch()`/`volatility_state_free()`). Free each state via
/// `volatility_state_free()`, then free the rest via `tulip_ffi_simd_result_free()`.
///
/// # Safety
/// - `inputs` must point to `num_assets` valid pointers, each pointing to
///   `INPUTS` valid non-null `*const f64`s, each pointing to `data_len` valid
///   `f64`s.
#[no_mangle]
pub unsafe extern "C" fn volatility_simd_by_assets(
    inputs: *const *const *const f64,
    num_assets: usize,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    match num_assets {
        2 => volatility_simd_by_assets_n::<2>(
            inputs,
            data_len,
            options,
            optional_outputs,
            numoptional,
        ),
        4 => volatility_simd_by_assets_n::<4>(
            inputs,
            data_len,
            options,
            optional_outputs,
            numoptional,
        ),
        8 => volatility_simd_by_assets_n::<8>(
            inputs,
            data_len,
            options,
            optional_outputs,
            numoptional,
        ),
        16 => volatility_simd_by_assets_n::<16>(
            inputs,
            data_len,
            options,
            optional_outputs,
            numoptional,
        ),
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn volatility_simd_by_assets_n<const N: usize>(
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

    match Volatility::indicator_by_assets::<N>(&refs, &_options, optional) {
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

/// Computes Volatility for `N` option sets simultaneously (SIMD), using the same
/// inputs data. `num_option_sets` must be 2, 4, 8, or 16 -- anything else
/// returns `InvalidInputs`.
///
/// `inputs` must point to `INPUTS` (1) pointers (`real`), each `data_len` `f64`s long.
/// `options` must point to `num_option_sets` arrays of `OPTIONS` (1) values: `period`.
///
/// Returns a `CSimdResult` with `num_option_sets` results, each with its own
/// output rows and its own ordinary `volatility` continuation state (reusable
/// with `volatility_batch()`/`volatility_state_free()`). Free each state via
/// `volatility_state_free()`, then free the rest via `tulip_ffi_simd_result_free()`.
///
/// # Safety
/// - `inputs` must point to `INPUTS` valid non-null `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `options` must point to `num_option_sets` valid arrays of `OPTIONS` `f64`s.
#[no_mangle]
pub unsafe extern "C" fn volatility_simd_by_options(
    inputs: *const *const f64,
    data_len: usize,
    options: *const *const f64,
    num_option_sets: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    match num_option_sets {
        2 => volatility_simd_by_options_n::<2>(
            inputs,
            data_len,
            options,
            optional_outputs,
            numoptional,
        ),
        4 => volatility_simd_by_options_n::<4>(
            inputs,
            data_len,
            options,
            optional_outputs,
            numoptional,
        ),
        8 => volatility_simd_by_options_n::<8>(
            inputs,
            data_len,
            options,
            optional_outputs,
            numoptional,
        ),
        16 => volatility_simd_by_options_n::<16>(
            inputs,
            data_len,
            options,
            optional_outputs,
            numoptional,
        ),
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn volatility_simd_by_options_n<const N: usize>(
    inputs: *const *const f64,
    data_len: usize,
    options: *const *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let options_arr: [&[f64; OPTIONS]; N] = read_simd_options::<N, OPTIONS>(options);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match Volatility::indicator_by_options::<N>(&inputs, &options_arr, optional) {
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
    fn test_volatility_info() {
        let info = volatility_info();
        assert!(info.inputs.len > 0);
        assert_eq!(info.options.len, 1);
        assert!(info.outputs.len > 0);
        assert_eq!(info.optional_outputs.len, 0);
    }

    #[test]
    fn test_volatility_min_data() {
        unsafe {
            let options: [f64; OPTIONS] = [14.0];
            let min = volatility_min_data(options.as_ptr());
            assert!(min > 0);
        }
    }

    #[test]
    fn test_volatility_indicator() {
        unsafe {
            let data_len = 60;
            let real: Vec<f64> = build_synthetic_data(data_len, 0);

            // Bind inputs to local first (dangling temp segfault rule)
            let inputs_ptr: [*const f64; INPUTS] = [real.as_ptr()];
            let inputs = inputs_ptr.as_ptr();
            let options: [f64; OPTIONS] = [14.0];
            let options_ptr = options.as_ptr();

            let result = volatility_indicator(inputs, data_len, options_ptr, std::ptr::null(), 0);

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_outputs, 1);
            tulip_ffi_result_free(result);
        }
    }

    #[test]
    fn test_volatility_batch() {
        unsafe {
            let data_len = 60;
            let real: Vec<f64> = build_synthetic_data(data_len, 0);

            // Bind inputs to local first (dangling temp segfault rule)
            let inputs_ptr: [*const f64; INPUTS] = [real.as_ptr()];
            let inputs = inputs_ptr.as_ptr();
            let options: [f64; OPTIONS] = [14.0];
            let options_ptr = options.as_ptr();

            let result = volatility_indicator(inputs, data_len, options_ptr, std::ptr::null(), 0);

            assert_eq!(result.error, CIndicatorError::Ok);
            let state = result.state;

            // Second batch with more data
            let extra_data_len = 10;
            let real_extra: Vec<f64> = build_synthetic_data(10, 0);

            let inputs_ptr_extra: [*const f64; INPUTS] = [real_extra.as_ptr()];
            let inputs_extra = inputs_ptr_extra.as_ptr();

            let batch_result =
                volatility_batch(state, inputs_extra, extra_data_len, std::ptr::null(), 0);

            assert_eq!(batch_result.error, CIndicatorError::Ok);
            assert_eq!(batch_result.num_outputs, 1);

            tulip_ffi_batch_result_free(batch_result);
            volatility_state_free(state);
        }
    }

    #[test]
    fn test_volatility_simd_by_assets() {
        unsafe {
            let data_len = 60;
            let real: Vec<f64> = build_synthetic_data(data_len, 0);
            let real2: Vec<f64> = build_synthetic_data(60, 2);

            // Create a proper nested array of asset pointers
            // For SIMD by assets with INPUTS=1:
            // - Each asset has 1 input pointer: [real]
            // - The inputs pointer is to an array of N asset arrays
            let assets_array: [[*const f64; INPUTS]; 2] = [[real.as_ptr()], [real2.as_ptr()]];
            // Cast to the expected pointer type: an array of 2 pointers, each
            // pointing to one asset's array of INPUTS data pointers.
            let assets_ptrs_arr: [*const *const f64; 2] =
                [assets_array[0].as_ptr(), assets_array[1].as_ptr()];
            let assets_ptrs = assets_ptrs_arr.as_ptr();

            let options: [f64; OPTIONS] = [14.0];
            let options_ptr = options.as_ptr();

            let result = volatility_simd_by_assets(
                assets_ptrs,
                2,
                data_len,
                options_ptr,
                std::ptr::null(),
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, 2);
            assert_eq!(result.num_outputs, 1);

            // Free individual states using pointer arithmetic
            volatility_state_free(result.states.wrapping_add(0).read());
            volatility_state_free(result.states.wrapping_add(1).read());

            tulip_ffi_simd_result_free(result);
        }
    }

    #[test]
    fn test_volatility_simd_by_options() {
        unsafe {
            let data_len = 60;
            let real: Vec<f64> = build_synthetic_data(data_len, 0);

            // Bind inputs to local first
            let inputs_ptr: [*const f64; INPUTS] = [real.as_ptr()];
            let inputs = inputs_ptr.as_ptr();

            // Create two option sets as separate arrays
            let options1: [f64; OPTIONS] = [14.0];
            let options2: [f64; OPTIONS] = [20.0];

            // Create array of pointers to the option arrays
            let options_ptrs_arr: [*const f64; 2] = [options1.as_ptr(), options2.as_ptr()];
            let options_ptr = options_ptrs_arr.as_ptr();

            let result =
                volatility_simd_by_options(inputs, data_len, options_ptr, 2, std::ptr::null(), 0);

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, 2);
            assert_eq!(result.num_outputs, 1); // only volatility (mandatory)

            // Free individual states using pointer arithmetic
            volatility_state_free(result.states.wrapping_add(0).read());
            volatility_state_free(result.states.wrapping_add(1).read());

            tulip_ffi_simd_result_free(result);
        }
    }
}
