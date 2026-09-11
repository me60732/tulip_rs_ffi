//! extern "C" wrapper for `natr`, mirroring the core `tulip_rs` crate's
//! `Natr::indicator` / `IndicatorState::batch_indicator` interface with a
//! Tulip-Indicators-style C calling convention: `inputs` is an array of
//! `INPUTS` pointers (one per input series, each `data_len` long, in the same
//! order as `tulip_rs::indicators::natr::Natr::INFO.inputs`), `options`
//! is a flat array of `OPTIONS` values -- exactly like `ti_natr`'s
//! `double const *const *inputs, double const *options`. Output allocation
//! stays inside the indicator function (never caller-supplied).
//!
//! Parameter order convention (kept consistent across every function in
//! this crate): each pointer parameter is immediately followed by the
//! count(s) that describe it, e.g. `inputs, data_len, options, ...,
//! optional_outputs, numoptional`.
//!
//! SIMD entry points:
//! - `natr_simd_by_assets`: compute NATR for N assets simultaneously,
//!   sharing a single options array. N must be 2, 4, 8, or 16.
//! - `natr_simd_by_options`: compute NATR for one asset with N different
//!   option sets simultaneously. N must be 2, 4, 8, or 16.

use std::os::raw::c_void;

use tulip_rs::indicator_types::{Indicator, IndicatorByOptions, TIndicatorState};
use tulip_rs::indicators::natr::{IndicatorState as NatrState, Natr, INPUTS, OPTIONS};
use tulip_rs::types::IndicatorError;

use crate::common::{
    optional_outputs_slice, pack_info, pack_outputs, pack_simd_outputs, pack_states, read_inputs,
    read_simd_assets_inputs, read_simd_options, CBatchResult, CIndicatorError, CIndicatorInfo,
    CIndicatorResult, CSimdResult,
};

/// Opaque state handle returned by `natr_indicator()` and consumed by
/// `natr_batch()` / `natr_state_free()`.
pub type NatrStateHandle = NatrState;

/// Returns static metadata about the `natr` indicator: its name, input
/// names, option names, and (mandatory/optional) output names, mirroring
/// `Natr::INFO`.
///
/// The returned strings are leaked, process-lifetime C strings -- read them,
/// don't free them.
#[no_mangle]
pub extern "C" fn natr_info() -> CIndicatorInfo {
    pack_info(&Natr::INFO)
}

/// Returns the minimum number of bars `natr` needs to produce any output at
/// all, given `options`.
///
/// # Safety
/// `options` must point to `OPTIONS` (1) valid `f64`s.
#[no_mangle]
pub unsafe extern "C" fn natr_min_data(options: *const f64) -> usize {
    let options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    Natr::min_data(&options)
}

/// Runs `natr` over `data_len` bars.
///
/// `inputs` must point to `INPUTS` (3) pointers, in order:
/// `high, low, close`, each `data_len` `f64`s long.
/// `options` must point to `OPTIONS` (1) value: `period`.
///
/// Returns the mandatory `natr` output plus any requested optional outputs
/// (`atr`, `tr`, in that fixed order) and a fresh continuation state.
///
/// # Safety
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s (pass null + 0 to request no optional outputs).
#[no_mangle]
pub unsafe extern "C" fn natr_indicator(
    inputs: *const *const f64,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CIndicatorResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match Natr::indicator(&inputs, &options, optional) {
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

/// Continues a `natr` calculation from `state`, mutating it in place so
/// it's ready for the next call. `state` must have come from
/// `natr_indicator()` and not yet have been passed to `natr_state_free()`.
///
/// `inputs` must point to `INPUTS` (3) pointers (`high, low, close`),
/// each `data_len` `f64`s long.
///
/// # Safety
/// - `state` must be a live pointer previously returned by
///   `natr_indicator()`.
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn natr_batch(
    state: *mut c_void,
    inputs: *const *const f64,
    data_len: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CBatchResult {
    if state.is_null() {
        return CBatchResult::err(IndicatorError::InvalidIndicatorState);
    }
    let state = &mut *(state as *mut NatrStateHandle);

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

/// Frees a state handle returned by `natr_indicator()` (or one of the
/// `states[i]` entries from `natr_simd_by_assets()`/`natr_simd_by_options()`).
/// Call this once you're done streaming (after your last `natr_batch()` call).
///
/// # Safety
/// `state` must be a live pointer previously returned by `natr_indicator()`
/// or found in a `CSimdResult::states` array, and must not be used again
/// after this call.
#[no_mangle]
pub unsafe extern "C" fn natr_state_free(state: *mut c_void) {
    if !state.is_null() {
        drop(Box::from_raw(state as *mut NatrStateHandle));
    }
}

/// Computes NATR for `N` assets simultaneously (SIMD), sharing a single
/// options array. `num_assets` must be 2, 4, 8, or 16 -- anything else
/// returns `InvalidInputs`.
///
/// `inputs` must point to `num_assets` pointers, each itself pointing to
/// `INPUTS` (3) pointers (`high, low, close`), each `data_len` `f64`s long.
/// `options` must point to `OPTIONS` (1) shared value.
///
/// Returns a `CSimdResult` with `num_assets` results, each with its own
/// output rows and its own ordinary `natr` continuation state (reusable
/// with `natr_batch()`/`natr_state_free()`). Free each state via
/// `natr_state_free()`, then free the rest via `tulip_ffi_simd_result_free()`.
///
/// # Safety
/// - `inputs` must point to `num_assets` valid pointers, each pointing to
///   `INPUTS` valid non-null `*const f64`s, each pointing to `data_len` valid
///   `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn natr_simd_by_assets(
    inputs: *const *const *const f64,
    num_assets: usize,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    match num_assets {
        2 => natr_simd_by_assets_n::<2>(inputs, data_len, options, optional_outputs, numoptional),
        4 => natr_simd_by_assets_n::<4>(inputs, data_len, options, optional_outputs, numoptional),
        8 => natr_simd_by_assets_n::<8>(inputs, data_len, options, optional_outputs, numoptional),
        16 => natr_simd_by_assets_n::<16>(inputs, data_len, options, optional_outputs, numoptional),
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn natr_simd_by_assets_n<const N: usize>(
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

    match Natr::indicator_by_assets::<N>(&refs, &options, optional) {
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

/// Computes NATR for one asset with `N` different option sets
/// simultaneously (SIMD). `num_option_sets` must be 2, 4, 8, or 16 --
/// anything else returns `InvalidInputs`.
///
/// `inputs` must point to `INPUTS` (3) pointers (`high, low, close`),
/// each `data_len` `f64`s long, shared across all option sets.
/// `options` must point to `num_option_sets` pointers, each itself
/// pointing to `OPTIONS` (1) value.
///
/// Returns a `CSimdResult` with `num_option_sets` results, each with its
/// own output rows and its own ordinary `natr` continuation state
/// (reusable with `natr_batch()`/`natr_state_free()`). Free each state
/// via `natr_state_free()`, then free the rest via
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
pub unsafe extern "C" fn natr_simd_by_options(
    inputs: *const *const f64,
    data_len: usize,
    options: *const *const f64,
    num_option_sets: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    match num_option_sets {
        2 => natr_simd_by_options_n::<2>(inputs, data_len, options, optional_outputs, numoptional),
        4 => natr_simd_by_options_n::<4>(inputs, data_len, options, optional_outputs, numoptional),
        8 => natr_simd_by_options_n::<8>(inputs, data_len, options, optional_outputs, numoptional),
        16 => {
            natr_simd_by_options_n::<16>(inputs, data_len, options, optional_outputs, numoptional)
        }
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn natr_simd_by_options_n<const N: usize>(
    inputs: *const *const f64,
    data_len: usize,
    options: *const *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let options = read_simd_options::<N, OPTIONS>(options);
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match Natr::indicator_by_options::<N>(&inputs, &options, optional) {
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
    fn test_natr_info() {
        let info = natr_info();
        assert_eq!(info.inputs.len, INPUTS);
        assert_eq!(info.options.len, OPTIONS);
        assert_eq!(info.outputs.len, 1);
        assert_eq!(info.optional_outputs.len, 2);
    }

    #[test]
    fn test_natr_min_data() {
        let options: [f64; OPTIONS] = [14.0];
        unsafe {
            assert!(natr_min_data(options.as_ptr()) > 0);
        }
    }

    #[test]
    fn test_natr_indicator() {
        let data_len = 60;
        let options: [f64; OPTIONS] = [14.0];
        let optional = [true; 2];

        unsafe {
            let high = build_synthetic_data(data_len);
            let low = build_synthetic_data(data_len);
            let close = build_synthetic_data(data_len);
            let inputs: [*const f64; INPUTS] = [high.as_ptr(), low.as_ptr(), close.as_ptr()];
            let result = natr_indicator(
                inputs.as_ptr(),
                data_len,
                options.as_ptr(),
                optional.as_ptr(),
                optional.len(),
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_outputs, 3);
            assert!(!result.outputs.is_null());
            assert!(!result.output_lens.is_null());

            let state = result.state;
            tulip_ffi_result_free(result);
            natr_state_free(state);
        }
    }

    #[test]
    fn test_natr_batch() {
        let data_len = 60;
        let options: [f64; OPTIONS] = [14.0];

        unsafe {
            let high = build_synthetic_data(data_len);
            let low = build_synthetic_data(data_len);
            let close = build_synthetic_data(data_len);
            let inputs: [*const f64; INPUTS] = [high.as_ptr(), low.as_ptr(), close.as_ptr()];
            let result = natr_indicator(
                inputs.as_ptr(),
                data_len,
                options.as_ptr(),
                std::ptr::null(),
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            let state = result.state;
            tulip_ffi_result_free(result);

            let more_high = build_synthetic_data(data_len);
            let more_low = build_synthetic_data(data_len);
            let more_close = build_synthetic_data(data_len);
            let more_inputs: [*const f64; INPUTS] =
                [more_high.as_ptr(), more_low.as_ptr(), more_close.as_ptr()];

            let batch_result =
                natr_batch(state, more_inputs.as_ptr(), data_len, std::ptr::null(), 0);

            assert_eq!(batch_result.error, CIndicatorError::Ok);
            assert_eq!(batch_result.num_outputs, 1); // only natr (mandatory)

            tulip_ffi_batch_result_free(batch_result);
            natr_state_free(state);
        }
    }

    #[test]
    fn test_natr_simd_by_assets() {
        const NUM_ASSETS: usize = 4;
        let data_len = 60;
        let options: [f64; OPTIONS] = [14.0];

        unsafe {
            let h0 = build_synthetic_data(data_len);
            let l0 = build_synthetic_data(data_len);
            let c0 = build_synthetic_data(data_len);
            let h1 = build_synthetic_data(data_len);
            let l1 = build_synthetic_data(data_len);
            let c1 = build_synthetic_data(data_len);
            let h2 = build_synthetic_data(data_len);
            let l2 = build_synthetic_data(data_len);
            let c2 = build_synthetic_data(data_len);
            let h3 = build_synthetic_data(data_len);
            let l3 = build_synthetic_data(data_len);
            let c3 = build_synthetic_data(data_len);
            let asset_inputs: [[*const f64; INPUTS]; NUM_ASSETS] = [
                [h0.as_ptr(), l0.as_ptr(), c0.as_ptr()],
                [h1.as_ptr(), l1.as_ptr(), c1.as_ptr()],
                [h2.as_ptr(), l2.as_ptr(), c2.as_ptr()],
                [h3.as_ptr(), l3.as_ptr(), c3.as_ptr()],
            ];
            let inputs_ptr: [*const *const f64; NUM_ASSETS] = [
                asset_inputs[0].as_ptr(),
                asset_inputs[1].as_ptr(),
                asset_inputs[2].as_ptr(),
                asset_inputs[3].as_ptr(),
            ];

            let result = natr_simd_by_assets(
                inputs_ptr.as_ptr(),
                NUM_ASSETS,
                data_len,
                options.as_ptr(),
                std::ptr::null(),
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, NUM_ASSETS);
            assert_eq!(result.num_outputs, 1); // only natr (mandatory)

            for i in 0..NUM_ASSETS {
                natr_state_free(*result.states.add(i));
            }
            tulip_ffi_simd_result_free(result);
        }
    }

    #[test]
    fn test_natr_simd_by_options() {
        const NUM_OPTION_SETS: usize = 4;
        let data_len = 60;

        unsafe {
            let high = build_synthetic_data(data_len);
            let low = build_synthetic_data(data_len);
            let close = build_synthetic_data(data_len);
            let inputs: [*const f64; INPUTS] = [high.as_ptr(), low.as_ptr(), close.as_ptr()];
            let o0: [f64; OPTIONS] = [12.0];
            let o1: [f64; OPTIONS] = [14.0];
            let o2: [f64; OPTIONS] = [16.0];
            let o3: [f64; OPTIONS] = [20.0];
            // options is *const *const f64 - array of pointers to each option array
            let options_array: [*const f64; NUM_OPTION_SETS] =
                [o0.as_ptr(), o1.as_ptr(), o2.as_ptr(), o3.as_ptr()];
            let options: *const *const f64 = options_array.as_ptr();

            let result = natr_simd_by_options(
                inputs.as_ptr(),
                data_len,
                options,
                NUM_OPTION_SETS,
                std::ptr::null(),
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, NUM_OPTION_SETS);
            assert_eq!(result.num_outputs, 1); // only natr (mandatory)

            for i in 0..NUM_OPTION_SETS {
                natr_state_free(*result.states.add(i));
            }
            tulip_ffi_simd_result_free(result);
        }
    }
}
