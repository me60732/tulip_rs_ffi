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
use std::slice;
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

/// Result of a SIMD `*_simd_by_assets`/`*_simd_by_options` call: N parallel
/// results, each with its own output rows and its own ordinary indicator
/// state (reusable with the existing `*_batch()`/`*_state_free()` functions).
#[repr(C)]
pub struct CSimdResult {
    pub error: CIndicatorError,
    /// [num_results][num_outputs] -> pointer to an output row's f64 data.
    pub outputs: *mut *mut *mut f64,
    /// [num_results][num_outputs] -> length of that output row.
    pub output_lens: *mut *mut usize,
    /// Number of output rows per result (mandatory + requested optional outputs).
    pub num_outputs: usize,
    /// [num_results] -> opaque boxed state pointer, each usable with the
    /// matching indicator's existing `*_batch()`/`*_state_free()` functions.
    pub states: *mut *mut c_void,
    /// N: number of parallel results (assets or option sets).
    pub num_results: usize,
}

impl CSimdResult {
    /// Creates an error result with all nulls/zeros, mirroring `CIndicatorResult::err`.
    pub(crate) fn err(e: IndicatorError) -> Self {
        CSimdResult {
            error: e.into(),
            outputs: ptr::null_mut(),
            output_lens: ptr::null_mut(),
            num_outputs: 0,
            states: ptr::null_mut(),
            num_results: 0,
        }
    }
}

/// Leaks `results` (Vec<Vec<Vec<f64>>>) into raw C-friendly buffers.
/// Returns `(outputs, output_lens, num_outputs, num_results)`.
///
/// Each of the N outer results gets packed via `pack_outputs`, then those N
/// results' pointers are boxed into two more leaked arrays.
pub(crate) fn pack_simd_outputs(
    results: Vec<Vec<Vec<f64>>>,
) -> (*mut *mut *mut f64, *mut *mut usize, usize, usize) {
    let num_results = results.len();
    let mut outputs_ptrs: Vec<*mut *mut f64> = Vec::with_capacity(num_results);
    let mut output_lens_ptrs: Vec<*mut usize> = Vec::with_capacity(num_results);
    let mut num_outputs = 0;

    for result_rows in results {
        let (outs, lens, n_out) = pack_outputs(result_rows);
        outputs_ptrs.push(outs);
        output_lens_ptrs.push(lens);
        num_outputs = n_out; // all results have same num_outputs
    }

    let outputs = Box::into_raw(outputs_ptrs.into_boxed_slice()) as *mut *mut *mut f64;
    let output_lens = Box::into_raw(output_lens_ptrs.into_boxed_slice()) as *mut *mut usize;
    (outputs, output_lens, num_outputs, num_results)
}

/// Boxes each state individually and returns a leaked array of `*mut c_void` pointers.
pub(crate) fn pack_states<S>(states: Vec<S>) -> *mut *mut c_void {
    let mut ptrs: Vec<*mut c_void> = Vec::with_capacity(states.len());
    for s in states {
        ptrs.push(Box::into_raw(Box::new(s)) as *mut c_void);
    }
    Box::into_raw(ptrs.into_boxed_slice()) as *mut *mut c_void
}

/// Frees the output buffers and the states outer array wrapper owned by a `CSimdResult`.
/// Does NOT drop/free the individual boxed states pointed to by each `states[i]`
/// (the caller must call the indicator-specific `*_state_free()` on each one first).
///
/// # Safety
/// Must be called at most once per `CSimdResult`, and only on a value actually
/// returned by one of this crate's `*_simd_by_assets()`/`*_simd_by_options()` functions.
/// The individual state pointers in `states[i]` must still be valid (not freed already)
/// when calling this function; they are not dropped here.
#[no_mangle]
pub unsafe extern "C" fn tulip_ffi_simd_result_free(result: CSimdResult) {
    if result.outputs.is_null() {
        return;
    }
    debug_assert!(!result.output_lens.is_null());
    debug_assert!(!result.states.is_null());

    let states_box: Box<[*mut c_void]> = Box::from_raw(ptr::slice_from_raw_parts_mut(
        result.states,
        result.num_results,
    ));
    drop(states_box);

    let lens_box: Box<[*mut usize]> = Box::from_raw(ptr::slice_from_raw_parts_mut(
        result.output_lens,
        result.num_results,
    ));
    let ptrs_box: Box<[*mut *mut f64]> = Box::from_raw(ptr::slice_from_raw_parts_mut(
        result.outputs,
        result.num_results,
    ));

    for i in 0..result.num_results {
        free_outputs(ptrs_box[i], lens_box[i], result.num_outputs);
    }
}

/// Reconstructs [&[f64]; N] from a Tulip-style `inputs` array of pointers:
/// N series, each `data_len` f64s long. Shared by every indicator's non-SIMD
/// wrapper (and, with N = INPUTS, by the SIMD by-assets/by-options helpers
/// below).
///
/// # Safety
/// `inputs` must point to exactly N valid, non-null `*const f64`s, each
/// itself pointing to `data_len` valid f64s.
pub(crate) unsafe fn read_inputs<'a, const N: usize>(
    inputs: *const *const f64,
    data_len: usize,
) -> [&'a [f64]; N] {
    let ptrs = slice::from_raw_parts(inputs, N);
    std::array::from_fn(|i| slice::from_raw_parts(ptrs[i], data_len))
}

/// Reconstructs owned [[&[f64]; INPUTS]; N] from a SIMD by-assets `inputs`
/// pointer: N assets, each with INPUTS input series, each `data_len` long.
/// Returns the owned nested array rather than the `&[&[f64]; INPUTS]; N]`
/// references the core `indicator_by_assets` API wants, since those
/// references must borrow from storage that outlives this call -- build
/// them from the returned array in the caller's own stack frame (see
/// `adosc.rs`/`macd.rs` for the two-line pattern).
///
/// # Safety
/// `inputs` must point to exactly N valid pointers, each itself pointing to
/// INPUTS valid non-null `*const f64`s, each pointing to `data_len` valid
/// f64s.
pub(crate) unsafe fn read_simd_assets_inputs<'a, const N: usize, const INPUTS: usize>(
    inputs: *const *const *const f64,
    data_len: usize,
) -> [[&'a [f64]; INPUTS]; N] {
    let assets = slice::from_raw_parts(inputs, N);
    std::array::from_fn(|i| read_inputs::<INPUTS>(assets[i], data_len))
}

/// Reconstructs [&[f64; OPTIONS]; N] from a SIMD by-options `options`
/// pointer: N option sets, each OPTIONS values long.
///
/// # Safety
/// `options` must point to exactly N valid pointers, each itself pointing
/// to OPTIONS valid f64s.
pub(crate) unsafe fn read_simd_options<'a, const N: usize, const OPTIONS: usize>(
    options: *const *const f64,
) -> [&'a [f64; OPTIONS]; N] {
    let ptrs = slice::from_raw_parts(options, N);
    std::array::from_fn(|i| &*(ptrs[i] as *const [f64; OPTIONS]))
}
