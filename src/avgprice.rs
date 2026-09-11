//! extern "C" wrapper for `avgprice`, mirroring the core `tulip_rs` crate's
//! `AvgPrice::indicator` / `IndicatorState::batch_indicator` interface. See
//! `adosc.rs` for the general Tulip-Indicators-style `inputs`/`options`
//! calling convention this follows, including the parameter-order rule
//! (every pointer parameter is immediately followed by the count(s) that
//! describe it).
//!
//! SIMD entry points:
//! - `avgprice_simd_by_assets`: compute AVGPRICE for N assets simultaneously,
//!   sharing a single options array. N must be 2, 4, 8, or 16.
//!
//! Note: AVGPRICE has no options (OPTIONS = 0), so `indicator_by_options`
//! is not available for this indicator.

use std::os::raw::c_void;

use tulip_rs::indicator_types::{Indicator, TIndicatorState};
use tulip_rs::indicators::avgprice::{AvgPrice, IndicatorState as AvgPriceState, INPUTS, OPTIONS};
use tulip_rs::types::IndicatorError;

use crate::common::{
    optional_outputs_slice, pack_info, pack_outputs, pack_simd_outputs, pack_states, read_inputs,
    read_simd_assets_inputs, CBatchResult, CIndicatorError, CIndicatorInfo, CIndicatorResult,
    CSimdResult,
};

/// Opaque state handle returned by `avgprice_indicator()` and consumed by
/// `avgprice_batch()` / `avgprice_state_free()`.
pub type AvgPriceStateHandle = AvgPriceState;

/// Returns static metadata about the `avgprice` indicator: its name, input
/// names, option names, and (mandatory/optional) output names, mirroring
/// `AvgPrice::INFO`.
///
/// The returned strings are leaked, process-lifetime C strings -- read them,
/// don't free them.
#[no_mangle]
pub extern "C" fn avgprice_info() -> CIndicatorInfo {
    pack_info(&AvgPrice::INFO)
}

/// Returns the minimum number of bars `avgprice` needs to produce any output at
/// all, given `options`.
#[no_mangle]
pub extern "C" fn avgprice_min_data(_options: *const f64) -> usize {
    AvgPrice::min_data(&[])
}

/// Runs `avgprice` over `data_len` bars.
///
/// `inputs` must point to `INPUTS` (4) pointers, in order:
/// `open, high, low, close`, each `data_len` `f64`s long.
/// `options` is empty for this indicator (OPTIONS = 0).
///
/// Returns the mandatory `avgprice` output plus any requested optional outputs
/// (none for AvgPrice).
///
/// # Safety
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s (pass null + 0 to request no optional outputs).
#[no_mangle]
pub unsafe extern "C" fn avgprice_indicator(
    inputs: *const *const f64,
    data_len: usize,
    _options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CIndicatorResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    // AVGPRICE has no options (OPTIONS=0), so we create an empty array
    let _options: [f64; OPTIONS] = [];
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match AvgPrice::indicator(&inputs, &_options, optional) {
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

/// Continues an `avgprice` calculation from `state`, mutating it in place so
/// it's ready for the next call. `state` must have come from
/// `avgprice_indicator()` and not yet have been passed to `avgprice_state_free()`.
///
/// `inputs` must point to `INPUTS` (4) pointers (`open, high, low, close`),
/// each `data_len` `f64`s long.
///
/// # Safety
/// - `state` must be a live pointer previously returned by
///   `avgprice_indicator()`.
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn avgprice_batch(
    state: *mut c_void,
    inputs: *const *const f64,
    data_len: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CBatchResult {
    if state.is_null() {
        return CBatchResult::err(IndicatorError::InvalidIndicatorState);
    }
    let state = &mut *(state as *mut AvgPriceStateHandle);

    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    // AVGPRICE has no options (OPTIONS=0), so we create an empty array
    let _options: [f64; OPTIONS] = [];
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

/// Frees a state handle returned by `avgprice_indicator()` (or one of the
/// `states[i]` entries from `avgprice_simd_by_assets()`).
/// Call this once you're done streaming (after your last `avgprice_batch()` call).
///
/// # Safety
/// `state` must be a live pointer previously returned by `avgprice_indicator()`
/// or found in a `CSimdResult::states` array, and must not be used again
/// after this call.
#[no_mangle]
pub unsafe extern "C" fn avgprice_state_free(state: *mut c_void) {
    if !state.is_null() {
        drop(Box::from_raw(state as *mut AvgPriceStateHandle));
    }
}

/// Computes AVGPRICE for `N` assets simultaneously (SIMD), sharing a single
/// options array. `num_assets` must be 2, 4, 8, or 16 -- anything else
/// returns `InvalidInputs`.
///
/// `inputs` must point to `num_assets` pointers, each itself pointing to
/// `INPUTS` (4) pointers (`open, high, low, close`), each `data_len` `f64`s
/// long. `options` is empty for this indicator.
///
/// Returns a `CSimdResult` with `num_assets` results, each with its own
/// output rows and its own ordinary `avgprice` continuation state (reusable
/// with `avgprice_batch()`/`avgprice_state_free()`). Free each state via
/// `avgprice_state_free()`, then free the rest via `tulip_ffi_simd_result_free()`.
///
/// # Safety
/// - `inputs` must point to `num_assets` valid pointers, each pointing to
///   `INPUTS` valid non-null `*const f64`s, each pointing to `data_len` valid
///   `f64`s.
/// - `optional_outputs`, if non-null, must point to `numoptional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn avgprice_simd_by_assets(
    inputs: *const *const *const f64,
    num_assets: usize,
    data_len: usize,
    _options: *const f64,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    match num_assets {
        2 => avgprice_simd_by_assets_n::<2>(inputs, data_len, optional_outputs, numoptional),
        4 => avgprice_simd_by_assets_n::<4>(inputs, data_len, optional_outputs, numoptional),
        8 => avgprice_simd_by_assets_n::<8>(inputs, data_len, optional_outputs, numoptional),
        16 => avgprice_simd_by_assets_n::<16>(inputs, data_len, optional_outputs, numoptional),
        _ => CSimdResult::err(IndicatorError::InvalidInputs),
    }
}

unsafe fn avgprice_simd_by_assets_n<const N: usize>(
    inputs: *const *const *const f64,
    data_len: usize,
    optional_outputs: *const bool,
    numoptional: usize,
) -> CSimdResult {
    // `owned` holds the per-asset input slices; `refs` borrows from it, so
    // both must live in this stack frame for the duration of the call.
    let owned = read_simd_assets_inputs::<N, INPUTS>(inputs, data_len);
    let refs: [&[&[f64]; INPUTS]; N] = std::array::from_fn(|i| &owned[i]);
    // AVGPRICE has no options (OPTIONS=0), so we create an empty array
    let _options: [f64; OPTIONS] = [];
    let optional = optional_outputs_slice(optional_outputs, numoptional);

    match AvgPrice::indicator_by_assets::<N>(&refs, &_options, optional) {
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

    #[test]
    fn test_avgprice_info() {
        let info = avgprice_info();
        assert!(info.inputs.len > 0);
        assert_eq!(info.options.len, 0);
        assert!(info.outputs.len > 0);
        assert_eq!(info.optional_outputs.len, 0);
    }

    #[test]
    fn test_avgprice_min_data() {
        let min = avgprice_min_data(std::ptr::null());
        assert!(min > 0);
    }

    #[test]
    fn test_avgprice_indicator() {
        unsafe {
            // Create test data: open, high, low, close prices
            let open = vec![10.0, 11.0, 12.0, 13.0, 14.0, 15.0];
            let high = vec![10.5, 11.5, 12.5, 13.5, 14.5, 15.5];
            let low = vec![9.5, 10.5, 11.5, 12.5, 13.5, 14.5];
            let close = vec![10.2, 11.2, 12.2, 13.2, 14.2, 15.2];

            let inputs_array = [open.as_ptr(), high.as_ptr(), low.as_ptr(), close.as_ptr()];
            let inputs_ptr = &inputs_array as *const *const f64;

            // No options (null pointer)
            let result = avgprice_indicator(
                inputs_ptr,
                open.len(),
                std::ptr::null(),
                std::ptr::null(),
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_outputs, 1);

            // Free results
            tulip_ffi_result_free(result);
        }
    }

    #[test]
    fn test_avgprice_batch() {
        unsafe {
            // Create test data: open, high, low, close prices
            let open = vec![10.0, 11.0, 12.0, 13.0, 14.0, 15.0];
            let high = vec![10.5, 11.5, 12.5, 13.5, 14.5, 15.5];
            let low = vec![9.5, 10.5, 11.5, 12.5, 13.5, 14.5];
            let close = vec![10.2, 11.2, 12.2, 13.2, 14.2, 15.2];

            let inputs_array = [open.as_ptr(), high.as_ptr(), low.as_ptr(), close.as_ptr()];
            let inputs_ptr = &inputs_array as *const *const f64;

            // First call to get state
            let result = avgprice_indicator(
                inputs_ptr,
                open.len(),
                std::ptr::null(),
                std::ptr::null(),
                0,
            );
            assert_eq!(result.error, CIndicatorError::Ok);
            let state = result.state;

            // Second call with batch
            let open2 = vec![16.0, 17.0];
            let high2 = vec![16.5, 17.5];
            let low2 = vec![15.5, 16.5];
            let close2 = vec![16.2, 17.2];
            let inputs_array2 = [
                open2.as_ptr(),
                high2.as_ptr(),
                low2.as_ptr(),
                close2.as_ptr(),
            ];
            let inputs_ptr2 = &inputs_array2 as *const *const f64;

            let batch_result = avgprice_batch(state, inputs_ptr2, open2.len(), std::ptr::null(), 0);

            assert_eq!(batch_result.error, CIndicatorError::Ok);
            assert_eq!(batch_result.num_outputs, 1);

            // Free results
            tulip_ffi_batch_result_free(batch_result);
            avgprice_state_free(state);
        }
    }

    #[test]
    fn test_avgprice_simd_by_assets() {
        unsafe {
            // Two assets, each with open, high, low, close prices
            let open1 = vec![10.0, 11.0, 12.0, 13.0];
            let high1 = vec![10.5, 11.5, 12.5, 13.5];
            let low1 = vec![9.5, 10.5, 11.5, 12.5];
            let close1 = vec![10.2, 11.2, 12.2, 13.2];

            let open2 = vec![20.0, 21.0, 22.0, 23.0];
            let high2 = vec![20.5, 21.5, 22.5, 23.5];
            let low2 = vec![19.5, 20.5, 21.5, 22.5];
            let close2 = vec![20.2, 21.2, 22.2, 23.2];

            // Asset inputs: each asset has INPUTS(4) pointers
            let assets_inputs1 = [
                open1.as_ptr(),
                high1.as_ptr(),
                low1.as_ptr(),
                close1.as_ptr(),
            ];
            let assets_inputs2 = [
                open2.as_ptr(),
                high2.as_ptr(),
                low2.as_ptr(),
                close2.as_ptr(),
            ];
            // Create array of pointers to each asset's input array
            let ptr_to_assets1 = &assets_inputs1 as *const [_; 4] as *const *const f64;
            let ptr_to_assets2 = &assets_inputs2 as *const [_; 4] as *const *const f64;
            let asset_pointers = [ptr_to_assets1, ptr_to_assets2];
            let inputs_ptr = asset_pointers.as_ptr();

            // No options
            let result = avgprice_simd_by_assets(
                inputs_ptr,
                2,
                open1.len(),
                std::ptr::null(),
                std::ptr::null(),
                0,
            );

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(result.num_results, 2);
            assert_eq!(result.num_outputs, 1);

            // Free results
            tulip_ffi_simd_result_free(result);
        }
    }
}
