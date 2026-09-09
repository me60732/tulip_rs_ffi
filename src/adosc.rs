//! extern "C" wrapper for `adosc`, mirroring the core `tulip_rs` crate's
//! `Adosc::indicator` / `IndicatorState::batch_indicator` interface with a
//! Tulip-Indicators-style C calling convention: `inputs` is an array of
//! `INPUTS` pointers (one per input series, each `size` long, in the same
//! order as `tulip_rs::indicators::adosc::Adosc::INFO.inputs`), `options`
//! is a flat array of `OPTIONS` values -- exactly like `ti_adosc`'s
//! `double const *const *inputs, double const *options`. Output allocation
//! stays inside the indicator function (never caller-supplied).

use std::os::raw::c_void;
use std::slice;

use tulip_rs::indicator_types::{Indicator, TIndicatorState};
use tulip_rs::indicators::adosc::{Adosc, IndicatorState as AdoscState, INPUTS, OPTIONS};

use crate::common::{optional_outputs_slice, pack_outputs, CBatchResult, CIndicatorResult};

/// Opaque state handle returned by `adosc_indicator()` and consumed by
/// `adosc_batch()` / `adosc_state_free()`.
pub type AdoscStateHandle = AdoscState;

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

/// Runs `adosc` over `size` bars.
///
/// `inputs` must point to `INPUTS` (4) pointers, in order:
/// `high, low, close, volume`, each `size` `f64`s long.
/// `options` must point to `OPTIONS` (2) values: `short_period, long_period`.
///
/// Returns the mandatory `adosc` output plus any requested optional outputs
/// (`short_ema`, `long_ema`, `ad`, in that fixed order) and a fresh
/// continuation state.
///
/// # Safety
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `size` valid `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `num_optional` valid
///   `bool`s (pass null + 0 to request no optional outputs).
#[no_mangle]
pub unsafe extern "C" fn adosc_indicator(
    size: usize,
    inputs: *const *const f64,
    options: *const f64,
    optional_outputs: *const bool,
    num_optional: usize,
) -> CIndicatorResult {
    let inputs = read_inputs(inputs, size);
    let options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
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
/// `inputs` must point to `INPUTS` (4) pointers (`high, low, close,
/// volume`), each `size` `f64`s long.
///
/// # Safety
/// - `state` must be a live pointer previously returned by
///   `adosc_indicator()`.
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `size` valid `f64`s.
/// - `optional_outputs`, if non-null, must point to `num_optional` valid
///   `bool`s.
#[no_mangle]
pub unsafe extern "C" fn adosc_batch(
    state: *mut c_void,
    size: usize,
    inputs: *const *const f64,
    optional_outputs: *const bool,
    num_optional: usize,
) -> CBatchResult {
    if state.is_null() {
        return CBatchResult::err(tulip_rs::types::IndicatorError::InvalidIndicatorState);
    }
    let state = &mut *(state as *mut AdoscStateHandle);

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
