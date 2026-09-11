//! extern "C" wrapper for `hilberttransform`, mirroring the core `tulip_rs` crate's
//! `HilbertTransform::indicator` / `IndicatorState::batch_indicator` interface with a
//! Tulip-Indicators-style C calling convention: `inputs` is an array of
//! `INPUTS` pointers (one per input series, each `data_len` long, in the same
//! order as `tulip_rs::indicators::hilberttransform::HilbertTransform::INFO.inputs`),
//! `options` is a flat array of `OPTIONS` values -- exactly like `ti_hilberttransform`'s
//! `double const *const *inputs, double const *options`. Output allocation
//! stays inside the indicator function (never caller-supplied).
//!
//! Parameter order convention (kept consistent across every function in
//! this crate): each pointer parameter is immediately followed by the
//! count(s) that describe it, e.g. `inputs, data_len, options, ...,
//! optional_outputs, numoptional`.
//!
//! SIMD entry points:
//! - `hilberttransform_simd_by_assets`: compute HilbertTransform for N assets simultaneously,
//!   sharing a single options array. N must be 2, 4, 8, or 16.
//! - `hilberttransform_simd_by_options`: compute HilbertTransform for one asset with N different
//!   option sets simultaneously. N must be 2, 4, 8, or 16.
//!
//! `#[no_mangle]` functions can't be generic, so each SIMD entry point is a
//! thin non-generic dispatcher that matches the runtime `N` against
//! {2,4,8,16} and calls a private generic `_n::<N>` helper that does the
//! real work.

use std::os::raw::c_void;

use tulip_rs::indicator_types::{Indicator, IndicatorByOptions, TIndicatorState};
use tulip_rs::indicators::hilberttransform::{HilbertTransform, IndicatorState, INPUTS, OPTIONS};
use tulip_rs::types::IndicatorError;

use crate::common::{
    optional_outputs_slice, pack_info, pack_outputs, pack_simd_outputs, pack_states, read_inputs,
    read_simd_assets_inputs, read_simd_options, CBatchResult, CIndicatorError, CIndicatorInfo,
    CIndicatorResult, CSimdResult,
};

/// Returns static metadata about the `hilberttransform` indicator: its name, input
/// names, option names, and (mandatory/optional) output names, mirroring
/// `HilbertTransform::INFO`.
///
/// The returned strings are leaked, process-lifetime C strings -- read them,
/// don't free them.
#[no_mangle]
pub extern "C" fn hilberttransform_info() -> CIndicatorInfo {
    pack_info(&HilbertTransform::INFO)
}

/// Returns the minimum number of bars `hilberttransform` needs to produce any output at
/// all, given `options`.
///
/// # Safety
/// `options` must point to `OPTIONS` (2) valid `f64`s.
#[no_mangle]
pub unsafe extern "C" fn hilberttransform_min_data(options: *const f64) -> usize {
    let options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    HilbertTransform::min_data(&options)
}

/// Runs `hilberttransform` over `data_len` bars.
///
/// `inputs` must point to `INPUTS` (1) pointer: `real`, `data_len` `f64`s long.
/// `options` must point to `OPTIONS` (2) values: `ss_period, hp_period`.
///
/// Returns the mandatory outputs (`in_phase`, `quadrature`) plus any requested
/// optional outputs (`roofing`, `highpass`, in that fixed order).
///
/// # Safety
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s (pass null + 0 to request no optional outputs).
#[no_mangle]
pub unsafe extern "C" fn hilberttransform_indicator(
    inputs: *const *const f64,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CIndicatorResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match HilbertTransform::indicator(&inputs, &options, optional) {
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

/// Continues a `hilberttransform` calculation from `state`, mutating it in place so
/// it's ready for the next call. `state` must have come from
/// `hilberttransform_indicator()` and not yet have been passed to `hilberttransform_state_free()`.
///
/// `inputs` must point to `INPUTS` (1) pointer: `real`, `data_len` `f64`s long.
///
/// # Safety
/// - `state` must be a live pointer previously returned by
///   `hilberttransform_indicator()`.
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn hilberttransform_batch(
    state: *mut c_void,
    inputs: *const *const f64,
    data_len: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CBatchResult {
    if state.is_null() {
        return CBatchResult::err(IndicatorError::InvalidIndicatorState);
    }
    let state = &mut *(state as *mut IndicatorState);

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

/// Frees a state handle returned by `hilberttransform_indicator()` (or one of the
/// `states[i]` entries from `hilberttransform_simd_by_assets()`/`hilberttransform_simd_by_options()`).
/// Call this once you're done streaming (after your last `hilberttransform_batch()` call).
///
/// # Safety
/// `state` must be a live pointer previously returned by `hilberttransform_indicator()`
/// or found in a `CSimdResult::states` array, and must not be used again
/// after this call.
#[no_mangle]
pub unsafe extern "C" fn hilberttransform_state_free(state: *mut c_void) {
    if !state.is_null() {
        drop(Box::from_raw(state as *mut IndicatorState));
    }
}

/// Computes HilbertTransform for `N` assets simultaneously (SIMD), sharing a single
/// options array. `num_assets` must be 2, 4, 8, or 16 -- anything else
/// returns `InvalidInputs`.
///
/// `inputs` must point to `num_assets` pointers, each itself pointing to
/// `INPUTS` (1) pointer (`real`), `data_len` `f64`s long. `options` must point
/// to `OPTIONS` (2) shared values.
///
/// Returns a `CSimdResult` with `num_assets` results, each with its own
/// output rows and its own ordinary `hilberttransform` continuation state (reusable
/// with `hilberttransform_batch()`/`hilberttransform_state_free()`). Free each state via
/// `hilberttransform_state_free()`, then free the rest via `tulip_ffi_simd_result_free()`.
///
/// # Safety
/// - `inputs` must point to `num_assets` valid pointers, each pointing to
///   `INPUTS` valid non-null `*const f64`s, each pointing to `data_len` valid
///   `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn hilberttransform_simd_by_assets(
    inputs: *const *const *const f64,
    num_assets: usize,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    match num_assets {
        2 => hilberttransform_simd_by_assets_n::<2>(
            inputs,
            data_len,
            options,
            optional_outputs,
            numoptional,
        ),
        4 => hilberttransform_simd_by_assets_n::<4>(
            inputs,
            data_len,
            options,
            optional_outputs,
            numoptional,
        ),
        8 => hilberttransform_simd_by_assets_n::<8>(
            inputs,
            data_len,
            options,
            optional_outputs,
            numoptional,
        ),
        16 => hilberttransform_simd_by_assets_n::<16>(
            inputs,
            data_len,
            options,
            optional_outputs,
            numoptional,
        ),
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn hilberttransform_simd_by_assets_n<const N: usize>(
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

    match HilbertTransform::indicator_by_assets::<N>(&refs, &options, optional) {
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

/// Computes HilbertTransform for one asset with `N` different option sets
/// simultaneously (SIMD). `num_option_sets` must be 2, 4, 8, or 16 --
/// anything else returns `InvalidInputs`.
///
/// `inputs` must point to `INPUTS` (1) pointer (`real`), `data_len` `f64`s
/// long, shared across all option sets. `options` must point to
/// `num_option_sets` pointers, each itself pointing to `OPTIONS` (2)
/// values.
///
/// Returns a `CSimdResult` with `num_option_sets` results, each with its
/// own output rows and its own ordinary `hilberttransform` continuation state
/// (reusable with `hilberttransform_batch()`/`hilberttransform_state_free()`). Free each state
/// via `hilberttransform_state_free()`, then free the rest via
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
pub unsafe extern "C" fn hilberttransform_simd_by_options(
    inputs: *const *const f64,
    data_len: usize,
    options: *const *const f64,
    num_option_sets: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    match num_option_sets {
        2 => hilberttransform_simd_by_options_n::<2>(
            inputs,
            data_len,
            options,
            optional_outputs,
            numoptional,
        ),
        4 => hilberttransform_simd_by_options_n::<4>(
            inputs,
            data_len,
            options,
            optional_outputs,
            numoptional,
        ),
        8 => hilberttransform_simd_by_options_n::<8>(
            inputs,
            data_len,
            options,
            optional_outputs,
            numoptional,
        ),
        16 => hilberttransform_simd_by_options_n::<16>(
            inputs,
            data_len,
            options,
            optional_outputs,
            numoptional,
        ),
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn hilberttransform_simd_by_options_n<const N: usize>(
    inputs: *const *const f64,
    data_len: usize,
    options: *const *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let options = read_simd_options::<N, OPTIONS>(options);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match HilbertTransform::indicator_by_options::<N>(&inputs, &options, optional) {
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
        tulip_ffi_batch_result_free, tulip_ffi_result_free, tulip_ffi_simd_result_free,
    };

    use crate::common::test::build_synthetic_data;

    #[test]
    fn test_hilberttransform_info() {
        let info = hilberttransform_info();
        assert!(info.inputs.len > 0);
        assert_eq!(info.options.len, 2);
        assert!(info.outputs.len > 0);
    }

    #[test]
    fn test_hilberttransform_min_data() {
        unsafe {
            let options: [f64; OPTIONS] = [10.0, 20.0];
            let min = hilberttransform_min_data(options.as_ptr());
            assert!(min > 0);
        }
    }

    #[test]
    fn test_hilberttransform_indicator() {
        unsafe {
            let ss_period = 10.0;
            let hp_period = 20.0;
            let data_len = 80; // Need enough for min_data: roofing + 7
            let inputs_arr = build_synthetic_data(data_len, 0);
            let options_arr = [ss_period, hp_period];

            let inputs_ptr = &inputs_arr as *const _ as *const f64;
            let inputs = [inputs_ptr];
            let options = &options_arr as *const _;

            let optional_outputs: [bool; 2] = [true, true]; // roofing, highpass
            let numoptional = 2;

            let result = hilberttransform_indicator(
                inputs.as_ptr(),
                data_len,
                options,
                optional_outputs.as_ptr(),
                numoptional,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_outputs, 4); // in_phase + quadrature + roofing + highpass

            tulip_ffi_result_free(result);
        }
    }

    #[test]
    fn test_hilberttransform_batch() {
        unsafe {
            let ss_period = 10.0;
            let hp_period = 20.0;
            let data_len = 80;
            let inputs_arr = build_synthetic_data(data_len, 0);
            let options_arr = [ss_period, hp_period];

            let inputs_ptr = &inputs_arr as *const _ as *const f64;
            let inputs = [inputs_ptr];
            let options = &options_arr as *const _;

            let optional_outputs: [bool; 2] = [true, true];
            let numoptional = 2;

            let result = hilberttransform_indicator(
                inputs.as_ptr(),
                data_len,
                options,
                optional_outputs.as_ptr(),
                numoptional,
            );

            assert_eq!(result.error, CIndicatorError::Ok);

            let state = result.state;

            // Second batch call with additional data
            let additional_data = build_synthetic_data(30, 0);
            let inputs_ptr2 = &additional_data as *const _ as *const f64;
            let inputs2 = [inputs_ptr2];

            let batch_result = hilberttransform_batch(
                state,
                inputs2.as_ptr(),
                30,
                optional_outputs.as_ptr(),
                numoptional,
            );

            assert_eq!(batch_result.error, CIndicatorError::Ok);

            tulip_ffi_batch_result_free(batch_result);
            hilberttransform_state_free(state);
        }
    }

    #[test]
    fn test_hilberttransform_simd_by_assets() {
        unsafe {
            let ss_period = 10.0;
            let hp_period = 20.0;
            let data_len = 80;
            let num_assets = 2;

            let inputs_arr1 = build_synthetic_data(data_len, 0);
            let inputs_arr2 = build_synthetic_data(data_len, 0);
            let options_arr = [ss_period, hp_period];

            let inputs_ptr1 = &inputs_arr1 as *const _ as *const f64;
            let inputs_ptr2 = &inputs_arr2 as *const _ as *const f64;
            let asset_inputs1 = [inputs_ptr1];
            let asset_inputs2 = [inputs_ptr2];
            let inputs = [asset_inputs1.as_ptr(), asset_inputs2.as_ptr()];
            let options = &options_arr as *const _;

            let optional_outputs: [bool; 2] = [true, true];
            let numoptional = 2;

            let result = hilberttransform_simd_by_assets(
                inputs.as_ptr(),
                num_assets,
                data_len,
                options,
                optional_outputs.as_ptr(),
                numoptional,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, num_assets);
            assert_eq!(result.num_outputs, 4);

            for i in 0..num_assets {
                let state_ptr = *result.states.add(i);
                hilberttransform_state_free(state_ptr);
            }

            tulip_ffi_simd_result_free(result);
        }
    }

    #[test]
    fn test_hilberttransform_simd_by_options() {
        unsafe {
            let ss_period1 = 10.0;
            let hp_period1 = 20.0;
            let ss_period2 = 15.0;
            let hp_period2 = 25.0;
            let data_len = 80;
            let num_option_sets = 2;

            let inputs_arr = build_synthetic_data(data_len, 0);
            let options_arr1 = [ss_period1, hp_period1];
            let options_arr2 = [ss_period2, hp_period2];

            let inputs_ptr = &inputs_arr as *const _ as *const f64;
            let inputs = [inputs_ptr];
            let options_ptr1 = &options_arr1 as *const _ as *const f64;
            let options_ptr2 = &options_arr2 as *const _ as *const f64;
            let options = [options_ptr1, options_ptr2];

            let optional_outputs: [bool; 2] = [true, true];
            let numoptional = 2;

            let result = hilberttransform_simd_by_options(
                inputs.as_ptr(),
                data_len,
                options.as_ptr(),
                num_option_sets,
                optional_outputs.as_ptr(),
                numoptional,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, num_option_sets);
            assert_eq!(result.num_outputs, 4);

            for i in 0..num_option_sets {
                let state_ptr = *result.states.add(i);
                hilberttransform_state_free(state_ptr);
            }

            tulip_ffi_simd_result_free(result);
        }
    }
}
