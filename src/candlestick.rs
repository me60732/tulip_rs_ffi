//! extern "C" wrapper for `candlestick`, mirroring the core `tulip_rs`
//! crate's `CandleStick::indicator` / `IndicatorState::batch_indicator`
//! interface with a Tulip-Indicators-style C calling convention: `inputs` is
//! an array of `INPUTS` pointers (one per input series, each `data_len` long,
//! in the same order as
//! `tulip_rs::indicators::candlestick::CandleStick::INFO.inputs`), `options`
//! is a flat array of `OPTIONS` values. Output allocation stays inside the
//! indicator function (never caller-supplied).
//!
//! # Candlestick output is not `f64` data
//!
//! Unlike every other indicator in this crate, `candlestick` produces, per
//! bar, *zero or more named candlestick patterns*
//! (`Option<Vec<CandlePattern>>`). This wrapper flattens that into a
//! CSR-style (compressed sparse row) representation over integer pattern
//! ids:
//!
//! - A process-lifetime **pattern table** is built on first use from the
//!   core crate's `PATTERN_DEFINITIONS` registry, sorted by the patterns'
//!   short names so the ids are stable regardless of build-script scan order.
//! - A result carries `bar_offsets` (`num_bars + 1` entries) and
//!   `pattern_ids` (`total_patterns` entries): the patterns detected on bar
//!   `i` are `pattern_ids[bar_offsets[i]..bar_offsets[i + 1]]`. A bar with no
//!   pattern (and a bar whose core output entry is `None`) yields an empty
//!   slice.
//! - Pattern metadata (the `name`/`full_name`/`japanese_name`/`bars`/
//!   `forecast` dict the docs describe) is looked up by id via
//!   `candlestick_num_patterns()`, `candlestick_pattern_info(id)` and
//!   `candlestick_pattern_names()`. The returned strings are leaked,
//!   process-lifetime C strings -- read them, don't free them.
//!
//! # Forecast filter
//!
//! The core API's `Option<ForecastType>` filter is passed as a plain `i32`:
//! `0..=5` select a `CForecastType` variant; any other value (use `-1` to
//! say "no filter") returns every detected pattern.
//!
//! # No optional outputs, no SIMD
//!
//! `candlestick` has a single output (`cdl_pattern`), so there are no
//! `optional_outputs`/`numoptional` parameters, and unlike the other
//! indicators here it is scalar-only (no `*_simd_by_assets`/
//! `*_simd_by_options` entry points).
//!
//! Parameter order convention (kept consistent across every function in
//! this crate): each pointer parameter is immediately followed by the
//! count(s) that describe it.

use std::ffi::CString;
use std::os::raw::{c_char, c_void};
use std::ptr;
use std::sync::OnceLock;

use tulip_rs::candle_indicators::candle_patterns::{CandlePattern, PATTERN_DEFINITIONS};
use tulip_rs::indicators::candlestick::{
    CandleStick, ForecastType, IndicatorState as CandleStickState, INPUTS, OPTIONS,
};
use tulip_rs::types::IndicatorError;

use crate::common::{pack_info, read_inputs, CIndicatorError, CIndicatorInfo, CStringArray};

/// Opaque state handle returned by `candlestick_indicator()` and consumed by
/// `candlestick_batch()` / `candlestick_state_free()`.
pub type CandleStickHandle = CandleStickState;

/// C-ABI mirror of `tulip_rs::candle_indicators::types::ForecastType`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CForecastType {
    BearishReversal = 0,
    BullishReversal = 1,
    BearishContinuation = 2,
    BullishContinuation = 3,
    BearishReversalOrContinuation = 4,
    BullishReversalOrContinuation = 5,
}

fn forecast_to_c(f: ForecastType) -> CForecastType {
    match f {
        ForecastType::BearishReversal => CForecastType::BearishReversal,
        ForecastType::BullishReversal => CForecastType::BullishReversal,
        ForecastType::BearishContinuation => CForecastType::BearishContinuation,
        ForecastType::BullishContinuation => CForecastType::BullishContinuation,
        ForecastType::BearishReversalOrContinuation => CForecastType::BearishReversalOrContinuation,
        ForecastType::BullishReversalOrContinuation => CForecastType::BullishReversalOrContinuation,
    }
}

/// Parses the C `forecast` filter parameter into the core API's
/// `Option<ForecastType>`. Anything outside `0..=5` (canonical: `-1`)
/// means "no filter".
fn parse_forecast(v: i32) -> Option<ForecastType> {
    match v {
        0 => Some(ForecastType::BearishReversal),
        1 => Some(ForecastType::BullishReversal),
        2 => Some(ForecastType::BearishContinuation),
        3 => Some(ForecastType::BullishContinuation),
        4 => Some(ForecastType::BearishReversalOrContinuation),
        5 => Some(ForecastType::BullishReversalOrContinuation),
        _ => None,
    }
}

/// All detectable patterns, sorted by short name, defining the id space
/// used by `pattern_ids` and `candlestick_pattern_info()`.
fn all_patterns() -> &'static [CandlePattern] {
    static TABLE: OnceLock<Vec<CandlePattern>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut v: Vec<CandlePattern> = Vec::new();
        v.extend(PATTERN_DEFINITIONS.one_bar.iter().map(|d| d.pattern));
        v.extend(PATTERN_DEFINITIONS.two_bar.iter().map(|d| d.pattern));
        v.extend(PATTERN_DEFINITIONS.three_bar.iter().map(|d| d.pattern));
        v.extend(PATTERN_DEFINITIONS.four_bar.iter().map(|d| d.pattern));
        v.extend(PATTERN_DEFINITIONS.five_bar.iter().map(|d| d.pattern));
        v.sort_by_key(|p| p.get_info().name);
        v
    })
}

/// Id of `pattern` within [`all_patterns`], or `u32::MAX` if it is somehow
/// not in the registry (should be unreachable).
fn pattern_id(pattern: CandlePattern) -> u32 {
    all_patterns()
        .iter()
        .position(|&p| p == pattern)
        .map_or(u32::MAX, |id| id as u32)
}

/// C-ABI mirror of `tulip_rs::candle_indicators::types::CandleInfo`, with
/// the pattern's table `id` attached. All string fields point to leaked,
/// process-lifetime buffers -- read them, don't free them.
///
/// A `u32::MAX` `id` (with null strings) is returned for out-of-range lookups.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct CCandlePatternInfo {
    pub id: u32,
    pub name: *const c_char,
    pub full_name: *const c_char,
    pub japanese_name: *const c_char,
    pub forecast: CForecastType,
    pub bars: u32,
}

// SAFETY: the string pointers inside are leaked, read-only,
// process-lifetime C strings, published once via `OnceLock` and never
// mutated afterwards.
unsafe impl Sync for CCandlePatternInfo {}
unsafe impl Send for CCandlePatternInfo {}

impl Default for CCandlePatternInfo {
    fn default() -> Self {
        CCandlePatternInfo {
            id: u32::MAX,
            name: ptr::null(),
            full_name: ptr::null(),
            japanese_name: ptr::null(),
            forecast: CForecastType::BearishReversal,
            bars: 0,
        }
    }
}

/// The pattern table with its C strings materialized once and leaked.
fn pattern_infos() -> &'static [CCandlePatternInfo] {
    static INFOS: OnceLock<Vec<CCandlePatternInfo>> = OnceLock::new();
    INFOS.get_or_init(|| {
        all_patterns()
            .iter()
            .enumerate()
            .map(|(id, p)| {
                let info = p.get_info();
                CCandlePatternInfo {
                    id: id as u32,
                    name: CString::new(info.name).unwrap().into_raw(),
                    full_name: CString::new(info.full_name).unwrap().into_raw(),
                    japanese_name: CString::new(info.japanese_name).unwrap().into_raw(),
                    forecast: forecast_to_c(info.forecast),
                    bars: info.bars as u32,
                }
            })
            .collect()
    })
}

/// Result of a fresh `candlestick_indicator()` call: CSR-packed pattern
/// output plus a freshly-boxed continuation state.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct CCandleStickResult {
    pub error: CIndicatorError,
    pub num_bars: usize,
    pub total_patterns: usize,
    /// `num_bars + 1` entries; row offsets into `pattern_ids`.
    pub bar_offsets: *mut u32,
    /// `total_patterns` entries; pattern table ids.
    pub pattern_ids: *mut u32,
    /// Opaque pointer to a boxed `CandleStick::IndicatorState`. Null on error.
    pub state: *mut c_void,
}

impl CCandleStickResult {
    fn err(e: IndicatorError) -> Self {
        CCandleStickResult {
            error: e.into(),
            num_bars: 0,
            total_patterns: 0,
            bar_offsets: ptr::null_mut(),
            pattern_ids: ptr::null_mut(),
            state: ptr::null_mut(),
        }
    }
}

/// Result of a `candlestick_batch()` continuation call (same CSR layout as
/// `CCandleStickResult`, minus the state -- the state passed in is mutated
/// in place).
#[repr(C)]
#[derive(Copy, Clone)]
pub struct CCandleStickBatchResult {
    pub error: CIndicatorError,
    pub num_bars: usize,
    pub total_patterns: usize,
    pub bar_offsets: *mut u32,
    pub pattern_ids: *mut u32,
}

impl CCandleStickBatchResult {
    fn err(e: IndicatorError) -> Self {
        CCandleStickBatchResult {
            error: e.into(),
            num_bars: 0,
            total_patterns: 0,
            bar_offsets: ptr::null_mut(),
            pattern_ids: ptr::null_mut(),
        }
    }
}

/// Flattens `output` into leaked CSR buffers:
/// `(num_bars, total_patterns, bar_offsets, pattern_ids)`.
/// Must be paired with `free_candlestick_output`.
fn pack_candlestick_output(
    output: Vec<Option<Vec<CandlePattern>>>,
) -> (usize, usize, *mut u32, *mut u32) {
    let num_bars = output.len();
    let mut offsets: Vec<u32> = Vec::with_capacity(num_bars + 1);
    let mut ids: Vec<u32> = Vec::new();
    offsets.push(0);
    for slot in output {
        if let Some(patterns) = slot {
            ids.extend(patterns.iter().copied().map(pattern_id));
        }
        offsets.push(ids.len() as u32);
    }
    debug_assert_eq!(offsets.len(), num_bars + 1);
    let total_patterns = ids.len();
    let bar_offsets = Box::into_raw(offsets.into_boxed_slice()) as *mut u32;
    let pattern_ids = Box::into_raw(ids.into_boxed_slice()) as *mut u32;
    (num_bars, total_patterns, bar_offsets, pattern_ids)
}

/// Reconstructs and drops everything `pack_candlestick_output` allocated.
/// Safe to call with null `bar_offsets` (e.g. after an error result).
///
/// # Safety
/// `bar_offsets`/`pattern_ids` must be exactly what `pack_candlestick_output`
/// returned for the given lengths, and must not have been freed already.
unsafe fn free_candlestick_output(
    bar_offsets: *mut u32,
    num_bars: usize,
    pattern_ids: *mut u32,
    total_patterns: usize,
) {
    if bar_offsets.is_null() {
        return;
    }
    debug_assert!(!pattern_ids.is_null());
    drop(Box::from_raw(ptr::slice_from_raw_parts_mut(
        bar_offsets,
        num_bars + 1,
    )));
    drop(Box::from_raw(ptr::slice_from_raw_parts_mut(
        pattern_ids,
        total_patterns,
    )));
}

/// Returns static metadata about the `candlestick` indicator: its name,
/// input names, option names, and output names, mirroring
/// `CandleStick::INFO`.
///
/// The returned strings are leaked, process-lifetime C strings -- read them,
/// don't free them.
#[no_mangle]
pub extern "C" fn candlestick_info() -> CIndicatorInfo {
    pack_info(&CandleStick::INFO)
}

/// Returns the minimum number of bars `candlestick` needs to produce any
/// output at all, given `options`.
///
/// # Safety
/// `options` must point to `OPTIONS` (3) valid `f64`s.
#[no_mangle]
pub unsafe extern "C" fn candlestick_min_data(options: *const f64) -> usize {
    let options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    CandleStick::min_data(&options)
}

/// Number of detectable candlestick patterns (ids `0..len`, see
/// `candlestick_pattern_info()`).
#[no_mangle]
pub extern "C" fn candlestick_num_patterns() -> usize {
    all_patterns().len()
}

/// Metadata for the pattern with table id `id`. Returns a struct with
/// `id == u32::MAX` and null strings if `id` is out of range.
///
/// The returned strings are leaked, process-lifetime C strings -- read them,
/// don't free them.
#[no_mangle]
pub extern "C" fn candlestick_pattern_info(id: u32) -> CCandlePatternInfo {
    match pattern_infos().get(id as usize) {
        Some(info) => *info,
        None => CCandlePatternInfo::default(),
    }
}

/// All pattern short names (by table id) as a C array of null-terminated
/// strings, for the common "I just want the name" case. Backing memory is
/// leaked -- read it, don't free it.
#[no_mangle]
pub extern "C" fn candlestick_pattern_names() -> CStringArray {
    static NAMES: OnceLock<CStringArray> = OnceLock::new();
    *NAMES.get_or_init(|| {
        let ptrs: Vec<*const c_char> = pattern_infos().iter().map(|i| i.name).collect();
        let len = ptrs.len();
        let ptr = Box::into_raw(ptrs.into_boxed_slice()) as *const *const c_char;
        CStringArray { ptr, len }
    })
}

/// Runs `candlestick` over `data_len` bars.
///
/// `inputs` must point to `INPUTS` (4) pointers: `open`, `high`, `low`,
/// `close`, each `data_len` `f64`s long.
/// `options` must point to `OPTIONS` (3) values: `candle_period`,
/// `trend_period`, `trend_signal_period`.
/// `forecast` filters the detected patterns by forecast direction
/// (`0..=5` = `CForecastType` variant; `-1` or anything else = no filter).
///
/// Returns the CSR-packed pattern output (see the module docs) plus a fresh
/// continuation state.
///
/// # Safety
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
/// - `options` must point to `OPTIONS` valid `f64`s.
#[no_mangle]
pub unsafe extern "C" fn candlestick_indicator(
    inputs: *const *const f64,
    data_len: usize,
    options: *const f64,
    forecast: i32,
) -> CCandleStickResult {
    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let options: [f64; OPTIONS] = *(options as *const [f64; OPTIONS]);
    let forecast = parse_forecast(forecast);

    match CandleStick::indicator(&inputs, &options, forecast) {
        Ok((output, state)) => {
            let (num_bars, total_patterns, bar_offsets, pattern_ids) =
                pack_candlestick_output(output);
            let state = Box::into_raw(Box::new(state)) as *mut c_void;
            CCandleStickResult {
                error: CIndicatorError::Ok,
                num_bars,
                total_patterns,
                bar_offsets,
                pattern_ids,
                state,
            }
        }
        Err(e) => CCandleStickResult::err(e),
    }
}

/// Continues a `candlestick` calculation from `state`, mutating it in place
/// so it's ready for the next call. `state` must have come from
/// `candlestick_indicator()` and not yet have been passed to
/// `candlestick_state_free()`.
///
/// `inputs` must point to `INPUTS` (4) pointers: `open`, `high`, `low`,
/// `close`, each `data_len` `f64`s long (just the new bars).
/// `forecast` filters the detected patterns (see `candlestick_indicator()`).
///
/// # Safety
/// - `state` must be a live pointer previously returned by
///   `candlestick_indicator()`.
/// - `inputs` must point to `INPUTS` valid `*const f64`s, each pointing to
///   `data_len` valid `f64`s.
#[no_mangle]
pub unsafe extern "C" fn candlestick_batch(
    state: *mut c_void,
    inputs: *const *const f64,
    data_len: usize,
    forecast: i32,
) -> CCandleStickBatchResult {
    if state.is_null() {
        return CCandleStickBatchResult::err(IndicatorError::InvalidIndicatorState);
    }
    let state = &mut *(state as *mut CandleStickHandle);

    let inputs = read_inputs::<INPUTS>(inputs, data_len);
    let forecast = parse_forecast(forecast);

    match state.batch_indicator(&inputs, forecast) {
        Ok(output) => {
            let (num_bars, total_patterns, bar_offsets, pattern_ids) =
                pack_candlestick_output(output);
            CCandleStickBatchResult {
                error: CIndicatorError::Ok,
                num_bars,
                total_patterns,
                bar_offsets,
                pattern_ids,
            }
        }
        Err(e) => CCandleStickBatchResult::err(e),
    }
}

/// Frees the CSR buffers owned by a `CCandleStickResult`. Does **not** touch
/// `state` -- call `candlestick_state_free()` for that.
///
/// # Safety
/// Must be called at most once per `CCandleStickResult`, and only on a value
/// actually returned by `candlestick_indicator()`.
#[no_mangle]
pub unsafe extern "C" fn candlestick_result_free(result: CCandleStickResult) {
    free_candlestick_output(
        result.bar_offsets,
        result.num_bars,
        result.pattern_ids,
        result.total_patterns,
    );
}

/// Frees the CSR buffers owned by a `CCandleStickBatchResult`.
///
/// # Safety
/// Must be called at most once per `CCandleStickBatchResult`, and only on a
/// value actually returned by `candlestick_batch()`.
#[no_mangle]
pub unsafe extern "C" fn candlestick_batch_result_free(result: CCandleStickBatchResult) {
    free_candlestick_output(
        result.bar_offsets,
        result.num_bars,
        result.pattern_ids,
        result.total_patterns,
    );
}

/// Frees a state handle returned by `candlestick_indicator()`. Call this
/// once you're done streaming (after your last `candlestick_batch()` call).
///
/// # Safety
/// `state` must be a live pointer previously returned by
/// `candlestick_indicator()`, and must not be used again after this call.
#[no_mangle]
pub unsafe extern "C" fn candlestick_state_free(state: *mut c_void) {
    if !state.is_null() {
        drop(Box::from_raw(state as *mut CandleStickHandle));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;
    use std::slice;

    /// Synthetic OHLC series with plausible bar geometry (high >= max(o,c),
    /// low <= min(o,c)) so pattern classification sees valid candles.
    fn build_synthetic_ohlc(len: usize) -> (Vec<f64>, Vec<f64>, Vec<f64>, Vec<f64>) {
        let mut open = Vec::with_capacity(len);
        let mut high = Vec::with_capacity(len);
        let mut low = Vec::with_capacity(len);
        let mut close = Vec::with_capacity(len);
        for i in 0..len {
            let x = i as f64;
            let o = (x + 1.0) * 100.0 + 10.0 * (x * 0.35).sin();
            let c = (x + 1.0) * 100.0 + 12.0 * (x * 0.5 + 1.7).sin();
            open.push(o);
            close.push(c);
            high.push(o.max(c) + 4.0 + 2.0 * (x * 0.9).sin().abs());
            low.push(o.min(c) - 4.0 - 2.0 * (x * 1.3).cos().abs());
        }
        (open, high, low, close)
    }

    unsafe fn input_ptrs(ohlc: &(Vec<f64>, Vec<f64>, Vec<f64>, Vec<f64>)) -> [*const f64; INPUTS] {
        [
            ohlc.0.as_ptr(),
            ohlc.1.as_ptr(),
            ohlc.2.as_ptr(),
            ohlc.3.as_ptr(),
        ]
    }

    /// CSR invariant: offsets start at 0, are non-decreasing, end at
    /// `total_patterns`, and every id is in range.
    unsafe fn assert_csr_valid(
        num_bars: usize,
        total: usize,
        offsets: *const u32,
        ids: *const u32,
    ) {
        assert!(!offsets.is_null());
        let offsets = slice::from_raw_parts(offsets, num_bars + 1);
        let ids = slice::from_raw_parts(ids, total);
        assert_eq!(offsets[0], 0);
        assert_eq!(offsets[num_bars] as usize, total);
        for w in offsets.windows(2) {
            assert!(w[0] <= w[1]);
        }
        assert!(ids
            .iter()
            .all(|&id| (id as usize) < candlestick_num_patterns()));
    }

    #[test]
    fn test_candlestick_info() {
        let info = candlestick_info();
        assert_eq!(info.inputs.len, INPUTS);
        assert_eq!(info.options.len, OPTIONS);
        assert_eq!(info.outputs.len, 1);
        assert_eq!(info.optional_outputs.len, 0);
    }

    #[test]
    fn test_candlestick_min_data() {
        unsafe {
            let options: [f64; OPTIONS] = [5.0, 10.0, 10.0];
            let min = candlestick_min_data(options.as_ptr());
            assert!(min > 0);
        }
    }

    #[test]
    fn test_candlestick_pattern_table() {
        let num = candlestick_num_patterns();
        assert!(num > 0);
        let info = candlestick_pattern_info(0);
        assert_eq!(info.id, 0);
        assert!(!info.name.is_null());
        // Names are sorted, so lookups must be stable and unique.
        let names = candlestick_pattern_names();
        assert_eq!(names.len, num);
        let first = unsafe { CStr::from_ptr(*names.ptr) };
        assert_eq!(first, unsafe { CStr::from_ptr(info.name) });
        // Out-of-range id yields the null sentinel.
        let bad = candlestick_pattern_info(num as u32);
        assert_eq!(bad.id, u32::MAX);
        assert!(bad.name.is_null());
    }

    #[test]
    fn test_candlestick_indicator() {
        unsafe {
            let data_len = 200;
            let ohlc = build_synthetic_ohlc(data_len);
            let inputs = input_ptrs(&ohlc);
            let options: [f64; OPTIONS] = [5.0, 10.0, 10.0];

            let result = candlestick_indicator(inputs.as_ptr(), data_len, options.as_ptr(), -1);

            assert_eq!(result.error, CIndicatorError::Ok);
            assert_eq!(
                result.num_bars,
                CandleStick::output_length(data_len, &options)
            );
            assert!(!result.state.is_null());
            assert_csr_valid(
                result.num_bars,
                result.total_patterns,
                result.bar_offsets,
                result.pattern_ids,
            );

            candlestick_result_free(result);
            candlestick_state_free(result.state);
        }
    }

    #[test]
    fn test_candlestick_indicator_with_forecast_filter() {
        unsafe {
            let data_len = 200;
            let ohlc = build_synthetic_ohlc(data_len);
            let inputs = input_ptrs(&ohlc);
            let options: [f64; OPTIONS] = [5.0, 10.0, 10.0];

            let all = candlestick_indicator(inputs.as_ptr(), data_len, options.as_ptr(), -1);
            let bull = candlestick_indicator(inputs.as_ptr(), data_len, options.as_ptr(), 1);
            assert_eq!(all.error, CIndicatorError::Ok);
            assert_eq!(bull.error, CIndicatorError::Ok);
            // A filtered run can never detect more patterns than an unfiltered one.
            assert!(bull.total_patterns <= all.total_patterns);
            // Every filtered pattern really has the requested forecast.
            let offsets = slice::from_raw_parts(bull.bar_offsets, bull.num_bars + 1);
            let ids = slice::from_raw_parts(bull.pattern_ids, bull.total_patterns);
            for (start, end) in offsets[..bull.num_bars].iter().zip(&offsets[1..]) {
                for &id in &ids[*start as usize..*end as usize] {
                    assert_eq!(
                        candlestick_pattern_info(id).forecast,
                        CForecastType::BullishReversal
                    );
                }
            }

            candlestick_result_free(all);
            candlestick_result_free(bull);
            candlestick_state_free(all.state);
            candlestick_state_free(bull.state);
        }
    }

    #[test]
    fn test_candlestick_indicator_not_enough_data() {
        unsafe {
            let ohlc = build_synthetic_ohlc(10);
            let inputs = input_ptrs(&ohlc);
            let options: [f64; OPTIONS] = [5.0, 10.0, 10.0];

            let result = candlestick_indicator(inputs.as_ptr(), 10, options.as_ptr(), -1);
            assert_eq!(result.error, CIndicatorError::NotEnoughData);
            assert!(result.state.is_null());
        }
    }

    #[test]
    fn test_candlestick_batch() {
        unsafe {
            let data_len = 200;
            let options: [f64; OPTIONS] = [5.0, 10.0, 10.0];
            let full_ohlc = build_synthetic_ohlc(data_len);
            let full_inputs = input_ptrs(&full_ohlc);

            let result =
                candlestick_indicator(full_inputs.as_ptr(), data_len, options.as_ptr(), -1);
            assert_eq!(result.error, CIndicatorError::Ok);

            let more = build_synthetic_ohlc(50);
            let batch_inputs = input_ptrs(&more);
            let batch = candlestick_batch(result.state, batch_inputs.as_ptr(), 50, -1);
            assert_eq!(batch.error, CIndicatorError::Ok);
            assert_eq!(batch.num_bars, 50);
            assert_csr_valid(
                batch.num_bars,
                batch.total_patterns,
                batch.bar_offsets,
                batch.pattern_ids,
            );

            candlestick_batch_result_free(batch);
            candlestick_result_free(result);
            candlestick_state_free(result.state);
        }
    }

    #[test]
    fn test_candlestick_batch_null_state() {
        unsafe {
            let ohlc = build_synthetic_ohlc(20);
            let inputs = input_ptrs(&ohlc);
            let result = candlestick_batch(ptr::null_mut(), inputs.as_ptr(), 20, -1);
            assert_eq!(result.error, CIndicatorError::InvalidIndicatorState);
        }
    }

    /// Regression test (core API only, no FFI): seed a Three Black Crows
    /// series, then deliver the final bars with various batch splits. All
    /// splits must agree with the full recompute on the last bar.
    ///
    /// Guards a core bug that made any `batch_indicator()` call after the
    /// first diverge from the full recompute (missed or false patterns);
    /// the core fix landed 2026-09-10. Run with `-- --nocapture` to see the
    /// per-split detections.
    #[test]
    fn core_batch_split_matrix() {
        let open: Vec<f64> = vec![
            81.85, 81.20, 81.55, 82.91, 83.10, 83.41, 82.71, 82.70, 84.20, 84.25, 84.03, 85.45,
            86.18, 88.00, 87.30, 87.50, 87.00, 86.50,
        ];
        let high: Vec<f64> = vec![
            82.15, 81.89, 83.03, 83.30, 83.85, 83.90, 83.33, 84.30, 84.84, 85.00, 85.90, 86.58,
            86.98, 88.00, 87.31, 87.55, 87.15, 86.60,
        ];
        let low: Vec<f64> = vec![
            81.29, 80.64, 81.31, 82.65, 83.07, 83.11, 82.49, 82.30, 84.15, 84.11, 84.03, 85.39,
            85.76, 87.17, 87.20, 86.10, 85.90, 85.20,
        ];
        let close: Vec<f64> = vec![
            81.59, 81.06, 82.87, 83.00, 83.61, 83.15, 82.84, 83.99, 84.55, 84.36, 85.53, 86.54,
            86.89, 87.77, 87.29, 86.50, 86.00, 85.50,
        ];
        let options = [5.0_f64, 2.0, 3.0];

        let full_in = [&open[..], &high[..], &low[..], &close[..]];
        let (full, _) = CandleStick::indicator(&full_in, &options, None).unwrap();
        let last_has = |v: &Vec<Option<Vec<CandlePattern>>>| {
            v.last()
                .and_then(|o| o.as_ref())
                .map(|p| {
                    p.iter()
                        .map(|x| format!("{:?}", x))
                        .collect::<Vec<_>>()
                        .join(",")
                })
                .unwrap_or_else(|| "-".into())
        };
        let expected = last_has(&full);
        assert!(expected.contains("ThreeBlackCrows"));
        println!("full(18): {}", expected);

        // seed = first `seed` bars; batches = consecutive bar ranges.
        for (label, seed, batches) in [
            ("15 + [3]      ", 15usize, vec![(15usize, 18usize)]),
            ("15 + [1,1,1]  ", 15, vec![(15, 16), (16, 17), (17, 18)]),
            ("16 + [1,1]    ", 16, vec![(16, 17), (17, 18)]),
            ("17 + [1]      ", 17, vec![(17, 18)]),
            ("15 + [2,1]    ", 15, vec![(15, 17), (17, 18)]),
            ("15 + [1,2]    ", 15, vec![(15, 16), (16, 18)]),
        ] {
            let seed_in = [&open[..seed], &high[..seed], &low[..seed], &close[..seed]];
            let (_, mut st) = CandleStick::indicator(&seed_in, &options, None).unwrap();
            let mut out = Vec::new();
            for (a, b) in batches {
                let one = [&open[a..b], &high[a..b], &low[a..b], &close[a..b]];
                out = st.batch_indicator(&one, None).unwrap();
            }
            let got = last_has(&out);
            println!("{}: last bar -> {}", label, got);
            assert_eq!(got, expected, "split {label} diverged from full recompute");
        }
    }
}
