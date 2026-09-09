//! extern "C" wrapper for `macd`, mirroring the core `tulip_rs` crate's
//! `Macd::indicator` / `IndicatorState::batch_indicator` interface. See
//! `adosc.rs` for the general Tulip-Indicators-style `inputs`/`options`
//! calling convention this follows.

use std::os::raw::c_void;
use std::slice;

use tulip_rs::indicator_types::{Indicator, TIndicatorState};
use tulip_rs::indicators::macd::{IndicatorState as MacdState, Macd, INPUTS, OPTIONS};

use crate::common::{optional_outputs_slice, pack_outputs, CBatchResult, CIndicatorResult};

/// Opaque state handle returned by `macd_indicator()` and consumed by
/// `macd_batch()` / `macd_state_free()`.
pub type MacdStateHandle = MacdState;

/// Reconstructs `[&[f64]; INPUTS]` from a Tulip-style `inputs` array of
/// pointers.
///
/// # Safety
/// `inputs` must point to exactly `INPUTS` valid, non-null `*const f64`s,
/// each itself pointing to `size` valid `f64`s.
unsafe fn read_inputs<'a>(inputs: *const *const f64, size: usize) -> [&'a [f64]; INPUTS] {
    let ptrs = slice::from_raw_parts(inputs, INPUTS);
    std::array::from_fn(|i| slice::from_raw_parts(ptrs[i], size))
}

/// Runs `macd` over `size` bars.
///
/// `inputs` must point to `INPUTS` (1) pointer: `real`, `size` `f64`s long.
/// `options` must point to `OPTIONS` (3) values: `short_period,
/// long_period, signal_period`.
///
/// Returns the mandatory `macd_line`/`signal_line`/`histogram` outputs plus
/// any requested optional outputs (`short_ema`, `long_ema`, in that fixed
/// order) and a fresh continuation state.
///
/// # Safety
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `size` valid `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `num_optional` valid
///   `bool`s (pass null + 0 to request no optional outputs).
#[no_mangle]
pub unsafe extern "C" fn macd_indicator(
    size: usize,
    inputs: *const *const f64,
    options: *const f64,
    optional_outputs: *const bool,
    num_optional: usize,
) -> CIndicatorResult {
    let inputs = read_inputs(inputs, size);
    let options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
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
/// `inputs` must point to `INPUTS` (1) pointer: `real`, `size` `f64`s long.
///
/// # Safety
/// - `state` must be a live pointer previously returned by
///   `macd_indicator()`.
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `size` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `num_optional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn macd_batch(
    state: *mut c_void,
    size: usize,
    inputs: *const *const f64,
    optional_outputs: *const bool,
    num_optional: usize,
) -> CBatchResult {
    if state.is_null() {
        return CBatchResult::err(tulip_rs::types::IndicatorError::InvalidIndicatorState);
    }
    let state = &mut *(state as *mut MacdStateHandle);

    let inputs = read_inputs(inputs, size);
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
