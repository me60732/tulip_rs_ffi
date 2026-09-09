//! extern "C" wrapper for `adosc`, mirroring the core `tulip_rs` crate's
//! `Adosc::indicator` / `IndicatorState::batch_indicator` interface with a
//! Tulip-Indicators-style C calling convention (raw pointers in, `Result`
//! struct out; output allocation stays inside the indicator function).

use std::os::raw::c_void;
use std::slice;

use tulip_rs::indicator_types::{Indicator, TIndicatorState};
use tulip_rs::indicators::adosc::{Adosc, IndicatorState as AdoscState, INPUTS, OPTIONS};

use crate::common::{optional_outputs_slice, pack_outputs, CBatchResult, CIndicatorResult};

/// Opaque state handle returned by `adosc_indicator()` and consumed by
/// `adosc_batch()` / `adosc_state_free()`.
pub type AdoscStateHandle = AdoscState;

/// Runs `adosc` over `size` bars of `high`/`low`/`close`/`volume`, returning
/// the mandatory `adosc` output plus any requested optional outputs
/// (`short_ema`, `long_ema`, `ad`, in that fixed order) and a fresh
/// continuation state.
///
/// # Safety
/// - `high`, `low`, `close`, `volume` must each point to `size` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `num_optional` valid
///   `bool`s (pass null + 0 to request no optional outputs).
#[no_mangle]
pub unsafe extern "C" fn adosc_indicator(
    high: *const f64,
    low: *const f64,
    close: *const f64,
    volume: *const f64,
    size: usize,
    short_period: f64,
    long_period: f64,
    optional_outputs: *const bool,
    num_optional: usize,
) -> CIndicatorResult {
    let inputs: [&[f64]; INPUTS] = [
        slice::from_raw_parts(high, size),
        slice::from_raw_parts(low, size),
        slice::from_raw_parts(close, size),
        slice::from_raw_parts(volume, size),
    ];
    let options: [f64; OPTIONS] = [short_period, long_period];
    let optional = optional_outputs_slice(optional_outputs, num_optional);

    match Adosc::indicator(&inputs, &options, optional) {
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

/// Continues an `adosc` calculation from `state`, mutating it in place so
/// it's ready for the next call. `state` must have come from
/// `adosc_indicator()` and not yet have been passed to `adosc_state_free()`.
///
/// # Safety
/// - `state` must be a live pointer previously returned by
///   `adosc_indicator()`.
/// - `high`/`low`/`close`/`volume` must each point to `size` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `num_optional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn adosc_batch(
    state: *mut c_void,
    high: *const f64,
    low: *const f64,
    close: *const f64,
    volume: *const f64,
    size: usize,
    optional_outputs: *const bool,
    num_optional: usize,
) -> CBatchResult {
    if state.is_null() {
        return CBatchResult::err(tulip_rs::types::IndicatorError::InvalidIndicatorState);
    }
    let state = &mut *(state as *mut AdoscStateHandle);

    let inputs: [&[f64]; INPUTS] = [
        slice::from_raw_parts(high, size),
        slice::from_raw_parts(low, size),
        slice::from_raw_parts(close, size),
        slice::from_raw_parts(volume, size),
    ];
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

/// Frees a state handle returned by `adosc_indicator()`. Call this once
/// you're done streaming (after your last `adosc_batch()` call).
///
/// # Safety
/// `state` must be a live pointer previously returned by
/// `adosc_indicator()`, and must not be used again after this call.
#[no_mangle]
pub unsafe extern "C" fn adosc_state_free(state: *mut c_void) {
    if !state.is_null() {
        drop(Box::from_raw(state as *mut AdoscStateHandle));
    }
}
