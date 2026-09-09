//! extern "C" wrapper for `macd`, mirroring the core `tulip_rs` crate's
//! `Macd::indicator` / `IndicatorState::batch_indicator` interface. See
//! `adosc.rs` for the general pattern this follows.

use std::os::raw::c_void;
use std::slice;

use tulip_rs::indicator_types::{Indicator, TIndicatorState};
use tulip_rs::indicators::macd::{IndicatorState as MacdState, Macd, INPUTS, OPTIONS};

use crate::common::{optional_outputs_slice, pack_outputs, CBatchResult, CIndicatorResult};

/// Opaque state handle returned by `macd_indicator()` and consumed by
/// `macd_batch()` / `macd_state_free()`.
pub type MacdStateHandle = MacdState;

/// Runs `macd` over `size` bars of `real`, returning the mandatory
/// `macd_line`/`signal_line`/`histogram` outputs plus any requested
/// optional outputs (`short_ema`, `long_ema`, in that fixed order) and a
/// fresh continuation state.
///
/// # Safety
/// - `real` must point to `size` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `num_optional` valid
///   `bool`s (pass null + 0 to request no optional outputs).
#[no_mangle]
pub unsafe extern "C" fn macd_indicator(
    real: *const f64,
    size: usize,
    short_period: f64,
    long_period: f64,
    signal_period: f64,
    optional_outputs: *const bool,
    num_optional: usize,
) -> CIndicatorResult {
    let inputs: [&[f64]; INPUTS] = [slice::from_raw_parts(real, size)];
    let options: [f64; OPTIONS] = [short_period, long_period, signal_period];
    let optional = optional_outputs_slice(optional_outputs, num_optional);

    match Macd::indicator(&inputs, &options, optional) {
        Ok((rows, state)) => {
            let (outputs, output_lens, num_outputs) = pack_outputs(rows);
            let state = Box::into_raw(Box::new(state)) as *mut c_void;
            CIndicatorResult {
                error: crate::common::CIndicatorError::Ok,
                outputs,
                output_lens,
                num_outputs,
                state,
            }
        }
        Err(e) => CIndicatorResult::err(e),
    }
}

/// Continues a `macd` calculation from `state`, mutating it in place so
/// it's ready for the next call. `state` must have come from
/// `macd_indicator()` and not yet have been passed to `macd_state_free()`.
///
/// # Safety
/// - `state` must be a live pointer previously returned by
///   `macd_indicator()`.
/// - `real` must point to `size` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `num_optional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn macd_batch(
    state: *mut c_void,
    real: *const f64,
    size: usize,
    optional_outputs: *const bool,
    num_optional: usize,
) -> CBatchResult {
    if state.is_null() {
        return CBatchResult::err(tulip_rs::types::IndicatorError::InvalidIndicatorState);
    }
    let state = &mut *(state as *mut MacdStateHandle);

    let inputs: [&[f64]; INPUTS] = [slice::from_raw_parts(real, size)];
    let optional = optional_outputs_slice(optional_outputs, num_optional);

    match state.batch_indicator(&inputs, optional) {
        Ok(rows) => {
            let (outputs, output_lens, num_outputs) = pack_outputs(rows);
            CBatchResult {
                error: crate::common::CIndicatorError::Ok,
                outputs,
                output_lens,
                num_outputs,
            }
        }
        Err(e) => CBatchResult::err(e),
    }
}

/// Frees a state handle returned by `macd_indicator()`. Call this once
/// you're done streaming (after your last `macd_batch()` call).
///
/// # Safety
/// `state` must be a live pointer previously returned by
/// `macd_indicator()`, and must not be used again after this call.
#[no_mangle]
pub unsafe extern "C" fn macd_state_free(state: *mut c_void) {
    if !state.is_null() {
        drop(Box::from_raw(state as *mut MacdStateHandle));
    }
}
