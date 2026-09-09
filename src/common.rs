//! Shared C-ABI plumbing used by every per-indicator wrapper module:
//!   - error code mapping from `tulip_rs::types::IndicatorError`
//!   - packing a `Vec<Vec<f64>>` result into raw C-friendly buffers
//!   - freeing those buffers again
//!
//! Memory model:
//!   - `*_indicator()` / `*_batch()` allocate output buffers on the Rust
//!     side (the caller never pre-allocates anything).
//!   - `*_result_free()` / `*_batch_result_free()` release only the output
//!     buffers, never the indicator state.
//!   - `*_state_free()` (defined per-indicator, since dropping requires the
//!     concrete Rust type) releases the boxed state returned by
//!     `*_indicator()` whenever the caller is done streaming.

use std::os::raw::c_void;
use std::ptr;
use tulip_rs::types::IndicatorError;

/// C-ABI mirror of `tulip_rs::types::IndicatorError`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CIndicatorError {
    Ok = 0,
    InvalidInputs = 1,
    NotEnoughData = 2,
    InvalidOptions = 3,
    InvalidIndicatorState = 4,
}

impl From<IndicatorError> for CIndicatorError {
    fn from(e: IndicatorError) -> Self {
        match e {
            IndicatorError::InvalidInputs => CIndicatorError::InvalidInputs,
            IndicatorError::NotEnoughData => CIndicatorError::NotEnoughData,
            IndicatorError::InvalidOptions => CIndicatorError::InvalidOptions,
            IndicatorError::InvalidIndicatorState => CIndicatorError::InvalidIndicatorState,
        }
    }
}

/// Result of a fresh `*_indicator()` call: owns both the output buffers and
/// a freshly-boxed indicator state (for later `*_batch()` continuation).
#[repr(C)]
pub struct CIndicatorResult {
    pub error: CIndicatorError,
    pub outputs: *mut *mut f64,
    pub output_lens: *mut usize,
    pub num_outputs: usize,
    /// Opaque pointer to a boxed `<Name>::IndicatorState`. Null on error.
    pub state: *mut c_void,
}

impl CIndicatorResult {
    pub(crate) fn err(e: IndicatorError) -> Self {
        CIndicatorResult {
            error: e.into(),
            outputs: ptr::null_mut(),
            output_lens: ptr::null_mut(),
            num_outputs: 0,
            state: ptr::null_mut(),
        }
    }
}

/// Result of a `*_batch()` continuation call. The state pointer passed in
/// is mutated in place, so there's nothing to hand back here.
#[repr(C)]
pub struct CBatchResult {
    pub error: CIndicatorError,
    pub outputs: *mut *mut f64,
    pub output_lens: *mut usize,
    pub num_outputs: usize,
}

impl CBatchResult {
    pub(crate) fn err(e: IndicatorError) -> Self {
        CBatchResult {
            error: e.into(),
            outputs: ptr::null_mut(),
            output_lens: ptr::null_mut(),
            num_outputs: 0,
        }
    }
}

/// Leaks `rows` into raw (outputs, output_lens, num_outputs) triples for a
/// C-ABI result struct. Must be paired with `free_outputs`.
pub(crate) fn pack_outputs(rows: Vec<Vec<f64>>) -> (*mut *mut f64, *mut usize, usize) {
    let num_outputs = rows.len();
    let mut ptrs: Vec<*mut f64> = Vec::with_capacity(num_outputs);
    let mut lens: Vec<usize> = Vec::with_capacity(num_outputs);

    for row in rows {
        let len = row.len();
        let boxed = row.into_boxed_slice();
        // Thin pointer to the first element; length is tracked separately
        // in `lens` since the C side has no concept of a Rust fat pointer.
        let ptr = Box::into_raw(boxed) as *mut f64;
        ptrs.push(ptr);
        lens.push(len);
    }

    let outputs = Box::into_raw(ptrs.into_boxed_slice()) as *mut *mut f64;
    let output_lens = Box::into_raw(lens.into_boxed_slice()) as *mut usize;
    (outputs, output_lens, num_outputs)
}

/// Reconstructs and drops everything `pack_outputs` allocated. Safe to call
/// with null `outputs` (e.g. after an error result) -- it's a no-op then.
///
/// # Safety
/// `outputs`/`output_lens` must be exactly what `pack_outputs` returned
/// (same `num_outputs`), and must not have been freed already.
pub(crate) unsafe fn free_outputs(
    outputs: *mut *mut f64,
    output_lens: *mut usize,
    num_outputs: usize,
) {
    if outputs.is_null() {
        return;
    }
    debug_assert!(!output_lens.is_null());

    let lens_box: Box<[usize]> =
        Box::from_raw(ptr::slice_from_raw_parts_mut(output_lens, num_outputs));
    let ptrs_box: Box<[*mut f64]> =
        Box::from_raw(ptr::slice_from_raw_parts_mut(outputs, num_outputs));

    for (i, &row_ptr) in ptrs_box.iter().enumerate() {
        let row_len = lens_box[i];
        drop(Box::from_raw(ptr::slice_from_raw_parts_mut(
            row_ptr, row_len,
        )));
    }
    // `ptrs_box`/`lens_box` drop here, freeing the two outer arrays.
}

/// Frees the output buffers owned by a `CIndicatorResult`. Does **not**
/// touch `state` -- call the indicator-specific `*_state_free()` for that.
///
/// # Safety
/// Must be called at most once per `CIndicatorResult`, and only on a value
/// actually returned by one of this crate's `*_indicator()` functions.
#[no_mangle]
pub unsafe extern "C" fn tulip_ffi_result_free(result: CIndicatorResult) {
    free_outputs(result.outputs, result.output_lens, result.num_outputs);
}

/// Frees the output buffers owned by a `CBatchResult`.
///
/// # Safety
/// Must be called at most once per `CBatchResult`, and only on a value
/// actually returned by one of this crate's `*_batch()` functions.
#[no_mangle]
pub unsafe extern "C" fn tulip_ffi_batch_result_free(result: CBatchResult) {
    free_outputs(result.outputs, result.output_lens, result.num_outputs);
}

/// Reconstructs an `Option<&[bool]>` from a raw (possibly null) pointer.
///
/// # Safety
/// If non-null, `ptr` must point to a valid `[bool; len]`.
pub(crate) unsafe fn optional_outputs_slice<'a>(
    ptr: *const bool,
    len: usize,
) -> Option<&'a [bool]> {
    if ptr.is_null() {
        None
    } else {
        Some(std::slice::from_raw_parts(ptr, len))
    }
}
