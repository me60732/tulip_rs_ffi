//! `tulip_rs_ffi` -- a hand-rolled `extern "C"` FFI layer around the core
//! `tulip_rs` indicator library.
//!
//! Unlike `tulip_rs_diplomat` (which generates bindings via the Diplomat
//! tool and its `DiplomatF64View` ABI), this crate exposes plain C
//! functions with a Tulip-Indicators-style calling convention: raw
//! pointers in, a `Result`-style struct out. Output buffers are always
//! allocated by the indicator function itself (never by the caller).
//!
//! See `common.rs` for the shared `CIndicatorResult`/`CBatchResult` shapes
//! and memory-ownership rules, and `adosc.rs`/`macd.rs` for the two
//! indicators currently wrapped.
pub mod ad;
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
pub mod macd;

pub use common::{
    tulip_ffi_batch_result_free, tulip_ffi_result_free, tulip_ffi_simd_result_free, CBatchResult,
    CIndicatorError, CIndicatorResult, CSimdResult,
};
