//! extern "C" wrapper for `homodynediscriminator`, mirroring the core `tulip_rs` crate's
//! `HomodyneDiscriminator::indicator` / `IndicatorState::batch_indicator` interface with a
//! Tulip-Indicators-style C calling convention: `inputs` is an array of
//! `INPUTS` pointers (one per input series, each `data_len` long, in the same
//! order as `tulip_rs::indicators::homodynediscriminator::HomodyneDiscriminator::INFO.inputs`),
//! `options` is a flat array of `OPTIONS` values -- exactly like `ti_homodynediscriminator`'s
//! `double const *const *inputs, double const *options`. Output allocation
//! stays inside the indicator function (never caller-supplied).
//!
//! Parameter order convention (kept consistent across every function in
//! this crate): each pointer parameter is immediately followed by the
//! count(s) that describe it, e.g. `inputs, data_len, options, ...,
//! optional_outputs, numoptional`.
//!
//! SIMD entry points:
//! - `homodynediscriminator_simd_by_assets`: compute HomodyneDiscriminator for N assets simultaneously,
//!   sharing a single options array. N must be 2, 4, 8, or 16.
//!
//! Note: `homodynediscriminator` does not support SIMD by-options (no `indicator_by_options` impl).
//!
//! `#[no_mangle]` functions can't be generic, so each SIMD entry point is a
//! thin non-generic dispatcher that matches the runtime `N` against
//! {2,4,8,16} and calls a private generic `_n::<N>` helper that does the
//! real work.

use std::os::raw::c_void;

use tulip_rs::indicator_types::{Indicator, TIndicatorState};
use tulip_rs::indicators::homodynediscriminator::{HomodyneDiscriminator, IndicatorState, INPUTS};
use tulip_rs::types::IndicatorError;

use crate::common::{
    optional_outputs_slice, pack_info, pack_outputs, pack_simd_outputs, pack_states, read_inputs,
    read_simd_assets_inputs, CBatchResult, CIndicatorError, CIndicatorInfo, CIndicatorResult,
    CSimdResult,
};

/// Returns static metadata about the `homodynediscriminator` indicator: its name, input
/// names, option names, and (mandatory/optional) output names, mirroring
/// `HomodyneDiscriminator::INFO`.
///
/// The returned strings are leaked, process-lifetime C strings -- read them,
/// don't free them.
#[no_mangle]
pub extern "C" fn homodynediscriminator_info() -> CIndicatorInfo {
    pack_info(&HomodyneDiscriminator::INFO)
}

/// Returns the minimum number of bars `homodynediscriminator` needs to produce any output at
/// all. This indicator has no options, so it always returns the same value.
#[no_mangle]
pub extern "C" fn homodynediscriminator_min_data(_options: *const f64) -> usize {
    HomodyneDiscriminator::min_data(&[])
}

/// Runs `homodynediscriminator` over `data_len` bars.
///
/// `inputs` must point to `INPUTS` (1) pointer: `real`, `data_len` `f64`s long.
/// `options` is ignored (the HD has no user parameters).
///
/// Returns the mandatory `dc_period` output only (no optional outputs).
///
/// # Safety
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `options` is ignored (homodynediscriminator has no options).
/// - `optional_outputs` is ignored (homodynediscriminator has no optional outputs).
#[no_mangle]
pub unsafe extern "C" fn homodynediscriminator_indicator(
    inputs: *const *const f64,
    data_len: usize,
    _options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CIndicatorResult {
    // options is ignored; homodynediscriminator has no parameters

    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match HomodyneDiscriminator::indicator(&inputs, &[], None) {
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

/// Continues a `homodynediscriminator` calculation from `state`, mutating it in place so
/// it's ready for the next call. `state` must have come from
/// `homodynediscriminator_indicator()` and not yet have been passed to `homodynediscriminator_state_free()`.
///
/// `inputs` must point to `INPUTS` (1) pointer: `real`, `data_len` `f64`s long.
///
/// # Safety
/// - `state` must be a live pointer previously returned by
///   `homodynediscriminator_indicator()`.
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `optional_outputs` is ignored (homodynediscriminator has no optional outputs).
#[no_mangle]
pub unsafe extern "C" fn homodynediscriminator_batch(
    state: *mut c_void,
    inputs: *const *const f64,
    data_len: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CBatchResult {
    if state.is_null() {
        return CBatchResult::err(IndicatorError::InvalidIndicatorState);
    }
    let optional = optional_outputs_slice(optional_outputs, numoptional);
    let state = &mut *(state as *mut IndicatorState);

    let inputs = read_inputs::<INPUTS>(inputs, data_len);

    match state.batch_indicator(&inputs, None) {
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

/// Frees a state handle returned by `homodynediscriminator_indicator()` (or one of the
/// `states[i]` entries from `homodynediscriminator_simd_by_assets()`/`homodynediscriminator_simd_by_options()`).
/// Call this once you're done streaming (after your last `homodynediscriminator_batch()` call).
///
/// # Safety
/// `state` must be a live pointer previously returned by `homodynediscriminator_indicator()`
/// or found in a `CSimdResult::states` array, and must not be used again
/// after this call.
#[no_mangle]
pub unsafe extern "C" fn homodynediscriminator_state_free(state: *mut c_void) {
    if !state.is_null() {
        drop(Box::from_raw(state as *mut IndicatorState));
    }
}

/// Computes HomodyneDiscriminator for `N` assets simultaneously (SIMD), sharing a single
/// options array. `num_assets` must be 2, 4, 8, or 16 -- anything else
/// returns `InvalidInputs`.
///
/// `inputs` must point to `num_assets` pointers, each itself pointing to
/// `INPUTS` (1) pointer (`real`), `data_len` `f64`s long. `options` is ignored.
///
/// Returns a `CSimdResult` with `num_assets` results, each with its own
/// output rows and its own ordinary `homodynediscriminator` continuation state (reusable
/// with `homodynediscriminator_batch()`/`homodynediscriminator_state_free()`). Free each state via
/// `homodynediscriminator_state_free()`, then free the rest via `tulip_ffi_simd_result_free()`.
///
/// # Safety
/// - `inputs` must point to `num_assets` valid pointers, each pointing to
///   `INPUTS` valid non-null `*const f64`s, each pointing to `data_len` valid
///   `f64`s.
/// - `options` is ignored (homodynediscriminator has no options).
/// - `optional_outputs` is ignored (homodynediscriminator has no optional outputs).
#[no_mangle]
pub unsafe extern "C" fn homodynediscriminator_simd_by_assets(
    inputs: *const *const *const f64,
    num_assets: usize,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    match num_assets {
        2 => homodynediscriminator_simd_by_assets_n::<2>(
            inputs,
            data_len,
            options,
            optional_outputs,
            numoptional,
        ),
        4 => homodynediscriminator_simd_by_assets_n::<4>(
            inputs,
            data_len,
            options,
            optional_outputs,
            numoptional,
        ),
        8 => homodynediscriminator_simd_by_assets_n::<8>(
            inputs,
            data_len,
            options,
            optional_outputs,
            numoptional,
        ),
        16 => homodynediscriminator_simd_by_assets_n::<16>(
            inputs,
            data_len,
            options,
            optional_outputs,
            numoptional,
        ),
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn homodynediscriminator_simd_by_assets_n<const N: usize>(
    inputs: *const *const *const f64,
    data_len: usize,
    _options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    // `owned` holds the per-asset input slices; `refs` borrows from it, so
    // both must live in this stack frame for the duration of the call.
    let optional = optional_outputs_slice(optional_outputs, numoptional);
    let owned = read_simd_assets_inputs::<N, INPUTS>(inputs, data_len);
    let refs: [&[&[f64]; INPUTS]; N] = std::array::from_fn(|i| &owned[i]);
    // options is ignored; homodynediscriminator has no parameters

    match HomodyneDiscriminator::indicator_by_assets::<N>(&refs, &[], None) {
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
    use crate::common::{
        test::build_synthetic_data, tulip_ffi_batch_result_free, tulip_ffi_result_free,
        tulip_ffi_simd_result_free,
    };

    #[test]
    fn test_homodynediscriminator_info() {
        let info = homodynediscriminator_info();
        assert!(info.inputs.len > 0);
        assert_eq!(info.options.len, 0);
        assert!(info.outputs.len > 0);
    }

    #[test]
    fn test_homodynediscriminator_min_data() {
        let min = homodynediscriminator_min_data(std::ptr::null());
        assert!(min > 0);
    }

    #[test]
    fn test_homodynediscriminator_indicator() {
        unsafe {
            let data_len = 60; // Need at least 23 bars for min_data
            let inputs_arr = build_synthetic_data(data_len, 0);

            let inputs_ptr = &inputs_arr as *const _ as *const f64;
            let inputs = [inputs_ptr];

            // homodynediscriminator has no options and no optional outputs
            let result = homodynediscriminator_indicator(
                inputs.as_ptr(),
                data_len,
                std::ptr::null(),
                std::ptr::null(),
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_outputs, 1); // only dc_period

            tulip_ffi_result_free(result);
        }
    }

    #[test]
    fn test_homodynediscriminator_batch() {
        unsafe {
            let data_len = 60;
            let inputs_arr = build_synthetic_data(data_len, 0);

            let inputs_ptr = &inputs_arr as *const _ as *const f64;
            let inputs = [inputs_ptr];

            // homodynediscriminator has no options and no optional outputs
            let result = homodynediscriminator_indicator(
                inputs.as_ptr(),
                data_len,
                std::ptr::null(),
                std::ptr::null(),
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);

            let state = result.state;

            // Second batch call with additional data
            let additional_data = build_synthetic_data(30, 0);
            let inputs_ptr2 = &additional_data as *const _ as *const f64;
            let inputs2 = [inputs_ptr2];

            let batch_result =
                homodynediscriminator_batch(state, inputs2.as_ptr(), 30, std::ptr::null(), 0);

            assert_eq!(batch_result.error, CIndicatorError::Ok);

            tulip_ffi_batch_result_free(batch_result);
            homodynediscriminator_state_free(state);
        }
    }

    #[test]
    fn test_homodynediscriminator_simd_by_assets() {
        unsafe {
            let data_len = 60;
            let num_assets = 2;

            let inputs_arr1 = build_synthetic_data(data_len, 0);
            let inputs_arr2 = build_synthetic_data(data_len, 0);

            let inputs_ptr1 = &inputs_arr1 as *const _ as *const f64;
            let inputs_ptr2 = &inputs_arr2 as *const _ as *const f64;
            let asset_inputs1 = [inputs_ptr1];
            let asset_inputs2 = [inputs_ptr2];
            let inputs = [asset_inputs1.as_ptr(), asset_inputs2.as_ptr()];

            // homodynediscriminator has no options and no optional outputs
            let result = homodynediscriminator_simd_by_assets(
                inputs.as_ptr(),
                num_assets,
                data_len,
                std::ptr::null(),
                std::ptr::null(),
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, num_assets);
            assert_eq!(result.num_outputs, 1);

            for i in 0..num_assets {
                let state_ptr = *result.states.add(i);
                homodynediscriminator_state_free(state_ptr);
            }

            tulip_ffi_simd_result_free(result);
        }
    }
}
