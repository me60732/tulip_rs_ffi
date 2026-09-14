//! `tulip_rs_ffi` -- a hand-rolled `extern "C"` FFI layer around the core
//! `tulip_rs` indicator library.
//!
//! This crate exposes plain C functions with a Tulip-Indicators-style
//! calling convention: raw pointers in, a `Result`-style struct out. Output
//! buffers are always allocated by the indicator function itself (never by
//! the caller).
//!
//! See `common.rs` for the shared `CIndicatorResult`/`CBatchResult` shapes
//! and memory-ownership rules, and `adosc.rs`/`macd.rs` for the canonical
//! per-indicator wrapper pattern.
pub mod ad;
pub mod adaptivemsw;
pub mod adosc;
pub mod adx;
pub mod adxr;
pub mod ao;
pub mod apo;
pub mod aroon;
pub mod aroonosc;
pub mod atr;
pub mod avgprice;
pub mod bbands;
pub mod bop;
pub mod candlestick;
pub mod ccfisher;
pub mod cci;
pub mod chaikinmf;
pub mod chandelierexit;
pub mod cmo;
pub mod common;
pub mod cvi;
pub mod cybercycle;
pub mod dema;
pub mod di;
pub mod dm;
pub mod donchianchannel;
pub mod dpo;
pub mod dx;
pub mod ef;
pub mod elderray;
pub mod ema;
pub mod emv;
pub mod fisher;
pub mod fosc;
pub mod highpass;
pub mod hilberttransform;
pub mod hma;
pub mod homodynediscriminator;
pub mod ichimoku;
pub mod instantaneoustrendline;
pub mod kama;
pub mod keltnerchannel;
pub mod kvo;
pub mod linreg;
pub mod macd;
pub mod mama;
pub mod marketfi;
pub mod mass;
pub mod max;
pub mod md;
pub mod medprice;
pub mod mfi;
pub mod min;
pub mod mom;
pub mod msw;
pub mod natr;
pub mod nvi;
pub mod obv;
pub mod pivotpoint;
pub mod ppo;
pub mod psar;
pub mod pvi;
pub mod qstick;
pub mod roc;
pub mod rocr;
pub mod roofingfilter;
pub mod rsi;
pub mod sma;
pub mod smaenvelope;
pub mod state_registry;
pub mod stddev;
pub mod stoch;
pub mod stochrsi;
pub mod supersmoother;
pub mod supertrend;
pub mod tema;
pub mod tr;
pub mod trendmode;
pub mod trima;
pub mod trix;
pub mod trvi;
pub mod tsf;
pub mod typprice;
pub mod ultosc;
pub mod vhf;
pub mod vidya;
pub mod volatility;
pub mod vortex;
pub mod vosc;
pub mod vwap;
pub mod vwma;
pub mod wad;
pub mod wcprice;
pub mod wilders;
pub mod willr;
pub mod wma;
pub mod zlema;

pub use common::{
    tulip_ffi_batch_result_free, tulip_ffi_bytes_free, tulip_ffi_result_free,
    tulip_ffi_simd_result_free, CBatchResult, CBytes, CIndicatorError, CIndicatorResult,
    CSimdResult,
};
pub use state_registry::{tulip_state_clone, tulip_state_deserialize, tulip_state_serialize};
