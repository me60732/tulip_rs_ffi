//! extern "C" wrapper for `ad`, mirroring the core `tulip_rs` crate's
//! `Ad::indicator` / `IndicatorState::batch_indicator` interface with a
//! Tulip-Indicators-style C calling convention: `inputs` is an array of
//! `INPUTS` pointers (one per input series, each `data_len` long, in the same
//! order as `tulip_rs::indicators::ad::Ad::INFO.inputs`), `options`
//! is a flat array of `OPTIONS` values -- exactly like `ti_ad`'s
//! `double const *const *inputs, double const *options`. Output allocation
//! stays inside the indicator function (never caller-supplied).
//!
//! Parameter order convention (kept consistent across every function in
//! this crate): each pointer parameter is immediately followed by the
//! count(s) that describe it, e.g. `inputs, data_len, options, ...,
//! optional_outputs, num_optional`.
//!
//! SIMD entry points:
//! - `ad_simd_by_assets`: compute AD for N assets simultaneously,
//!   sharing a single options array. N must be 2, 4, 8, or 16.
//!
//! Note: `ad` does not support SIMD by-options (no `indicator_by_options` impl).
//!
//! `#[no_mangle]` functions can't be generic, so each SIMD entry point is a
//! thin non-generic dispatcher that matches the runtime `N` against
//! {2,4,8,16} and calls a private generic `_n::<N>` helper that does the
//! real work.

use std::os::raw::c_void;

use tulip_rs::indicator_types::{Indicator, TIndicatorState};
use tulip_rs::indicators::ad::{Ad, IndicatorState as AdState, INPUTS, OPTIONS};
use tulip_rs::types::IndicatorError;

use crate::common::{
    optional_outputs_slice, pack_outputs, pack_simd_outputs, pack_states, read_inputs,
    read_simd_assets_inputs, CBatchResult, CIndicatorError, CIndicatorResult, CSimdResult,
};

/// Opaque state handle returned by `ad_indicator()` and consumed by
/// `ad_batch()` / `ad_state_free()`.
pub type AdStateHandle = AdState;

/// Runs `ad` over `data_len` bars.
///
/// `inputs` must point to `INPUTS` (4) pointers, in order:
/// `high, low, close, volume`, each `data_len` `f64`s long.
/// `options` must point to `OPTIONS` (0) values (no options).
///
/// Returns the mandatory `ad` output plus any requested optional outputs
/// (none for this indicator) and a fresh continuation state.
///
/// # Safety
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
#[no_mangle]
pub unsafe extern "C" fn ad_indicator(
    inputs: *const *const f64,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    num_optional: usize,
) -> CIndicatorResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let _options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    let optional = optional_outputs_slice(optional_outputs, num_optional);

    match Ad::indicator(&inputs, &_options, optional) {
        Ok((rows, state)) => {
            let (outputs, output_lens, num_outputs) = pack_outputs(rows);
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

/// Continues an `ad` calculation from `state`, mutating it in place so
/// it's ready for the next call. `state` must have come from
/// `ad_indicator()` and not yet have been passed to `ad_state_free()`.
///
/// `inputs` must point to `INPUTS` (4) pointers (`high, low, close,
/// volume`), each `data_len` `f64`s long.
///
/// # Safety
/// - `state` must be a live pointer previously returned by
///   `ad_indicator()`.
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
#[no_mangle]
pub unsafe extern "C" fn ad_batch(
    state: *mut c_void,
    inputs: *const *const f64,
    data_len: usize,
    optional_outputs: *const bool,
    num_optional: usize,
) -> CBatchResult {
    if state.is_null() {
        return CBatchResult::err(IndicatorError::InvalidIndicatorState);
    }
    let state = &mut *(state as *mut AdStateHandle);

    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let optional = optional_outputs_slice(optional_outputs, num_optional);

    match state.batch_indicator(&inputs, optional) {
        Ok(rows) => {
            let (outputs, output_lens, num_outputs) = pack_outputs(rows);
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

/// Frees a state handle returned by `ad_indicator()` (or one of the
/// `states[i]` entries from `ad_simd_by_assets()`).
/// Call this once you're done streaming (after your last `ad_batch()` call).
///
/// # Safety
/// `state` must be a live pointer previously returned by `ad_indicator()`
/// or found in a `CSimdResult::states` array, and must not be used again
/// after this call.
#[no_mangle]
pub unsafe extern "C" fn ad_state_free(state: *mut c_void) {
    if !state.is_null() {
        drop(Box::from_raw(state as *mut AdStateHandle));
    }
}

/// Computes AD for `N` assets simultaneously (SIMD), sharing a single
/// options array. `num_assets` must be 2, 4, 8, or 16 -- anything else
/// returns `InvalidInputs`.
///
/// `inputs` must point to `num_assets` pointers, each itself pointing to
/// `INPUTS` (4) pointers (`high, low, close, volume`), each `data_len` `f64`s long.
/// `options` must point to `OPTIONS` (0) shared values.
///
/// Returns a `CSimdResult` with `num_assets` results, each with its own
/// output rows and its own ordinary `ad` continuation state (reusable
/// with `ad_batch()`/`ad_state_free()`). Free each state via
/// `ad_state_free()`, then free the rest via `tulip_ffi_simd_result_free()`.
///
/// # Safety
/// - `inputs` must point to `num_assets` valid pointers, each pointing to
///   `INPUTS` valid non-null `*const f64`s, each pointing to `data_len` valid
///   `f64`s.
#[no_mangle]
pub unsafe extern "C" fn ad_simd_by_assets(
    inputs: *const *const *const f64,
    num_assets: usize,
    data_len: usize,
    options: *const f64,
    optional_outputs: *const bool,
    num_optional: usize,
) -> CSimdResult {
    match num_assets {
        2 => ad_simd_by_assets_n::<2>(inputs, data_len, options, optional_outputs, num_optional),
        4 => ad_simd_by_assets_n::<4>(inputs, data_len, options, optional_outputs, num_optional),
        8 => ad_simd_by_assets_n::<8>(inputs, data_len, options, optional_outputs, num_optional),
        16 => ad_simd_by_assets_n::<16>(inputs, data_len, options, optional_outputs, num_optional),
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn ad_simd_by_assets_n<const N: usize>(
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
    let _options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    let optional = optional_outputs_slice(optional_outputs, num_optional);

    match Ad::indicator_by_assets::<N>(&refs, &_options, optional) {
        Ok((results, states)) => {
            let (outputs, output_lens, num_outputs, num_results) = pack_simd_outputs(results);
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
    fn test_ad_indicator() {
        unsafe {
            let data_len = 20;
            let high: Vec<f64> = (1..=data_len).map(|i| i as f64 + 1.0).collect();
            let low: Vec<f64> = (1..=data_len).map(|i| i as f64 - 1.0).collect();
            let close: Vec<f64> = (1..=data_len).map(|i| i as f64).collect();
            let volume: Vec<f64> = (1..=data_len).map(|i| i as f64 * 10.0).collect();

            // Create a proper array of input pointers
            let inputs_ptr: [*const f64; INPUTS] =
                [high.as_ptr(), low.as_ptr(), close.as_ptr(), volume.as_ptr()];
            let inputs = inputs_ptr.as_ptr();
            let options = [0f64; OPTIONS].as_ptr();
            let result = ad_indicator(inputs, data_len, options, std::ptr::null(), 0);

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_outputs, 1);
            tulip_ffi_result_free(result);
        }
    }

    #[test]
    fn test_ad_batch() {
        unsafe {
            let data_len = 20;
            let high: Vec<f64> = (1..=data_len).map(|i| i as f64 + 1.0).collect();
            let low: Vec<f64> = (1..=data_len).map(|i| i as f64 - 1.0).collect();
            let close: Vec<f64> = (1..=data_len).map(|i| i as f64).collect();
            let volume: Vec<f64> = (1..=data_len).map(|i| i as f64 * 10.0).collect();

            // Create a proper array of input pointers
            let inputs_ptr: [*const f64; INPUTS] =
                [high.as_ptr(), low.as_ptr(), close.as_ptr(), volume.as_ptr()];
            let inputs = inputs_ptr.as_ptr();
            let options = [0f64; OPTIONS].as_ptr();
            let result = ad_indicator(inputs, data_len, options, std::ptr::null(), 0);

            assert_eq!(result.error, CIndicatorError::Ok);
            let state = result.state;

            // Second batch with more data
            let extra_data_len = 10;
            let high_extra: Vec<f64> = (21..=20 + extra_data_len).map(|i| i as f64 + 1.0).collect();
            let low_extra: Vec<f64> = (21..=20 + extra_data_len).map(|i| i as f64 - 1.0).collect();
            let close_extra: Vec<f64> = (21..=20 + extra_data_len).map(|i| i as f64).collect();
            let volume_extra: Vec<f64> = (21..=20 + extra_data_len)
                .map(|i| i as f64 * 10.0)
                .collect();

            let inputs_ptr_extra: [*const f64; INPUTS] = [
                high_extra.as_ptr(),
                low_extra.as_ptr(),
                close_extra.as_ptr(),
                volume_extra.as_ptr(),
            ];
            let inputs_extra = inputs_ptr_extra.as_ptr();
            let batch_result = ad_batch(state, inputs_extra, extra_data_len, std::ptr::null(), 0);

            assert_eq!(batch_result.error, CIndicatorError::Ok);
            assert_eq!(batch_result.num_outputs, 1);

            tulip_ffi_batch_result_free(batch_result);
            ad_state_free(state);
        }
    }

    #[test]
    fn test_ad_simd_by_assets() {
        unsafe {
            let data_len = 20;
            let high: Vec<f64> = (1..=data_len).map(|i| i as f64 + 1.0).collect();
            let low: Vec<f64> = (1..=data_len).map(|i| i as f64 - 1.0).collect();
            let close: Vec<f64> = (1..=data_len).map(|i| i as f64).collect();
            let volume: Vec<f64> = (1..=data_len).map(|i| i as f64 * 10.0).collect();

            // Two different "assets"
            let high2: Vec<f64> = (21..=40).map(|i| i as f64 + 1.0).collect();
            let low2: Vec<f64> = (21..=40).map(|i| i as f64 - 1.0).collect();
            let close2: Vec<f64> = (21..=40).map(|i| i as f64).collect();
            let volume2: Vec<f64> = (21..=40).map(|i| i as f64 * 10.0).collect();

            // Create a proper nested array of asset pointers
            // For SIMD by assets with INPUTS=4:
            // - Each asset has 4 input pointers: [high, low, close, volume]
            // - The inputs pointer is to an array of N asset arrays
            let assets_array: [[*const f64; INPUTS]; 2] = [
                [high.as_ptr(), low.as_ptr(), close.as_ptr(), volume.as_ptr()],
                [
                    high2.as_ptr(),
                    low2.as_ptr(),
                    close2.as_ptr(),
                    volume2.as_ptr(),
                ],
            ];
            // Cast to the expected pointer type
            let assets_ptrs =
                &assets_array as *const [[*const f64; INPUTS]; 2] as *const *const *const f64;
            let options = [0f64; OPTIONS].as_ptr();

            let result = ad_simd_by_assets(assets_ptrs, 2, data_len, options, std::ptr::null(), 0);

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, 2);
            assert_eq!(result.num_outputs, 1);

            // Don't free individual states - tulip_ffi_simd_result_free handles that
            tulip_ffi_simd_result_free(result);
        }
    }
}
