//! Single source of truth for the per-indicator lists this crate needs,
//! evaluated against the real `tulip_rs` core crate at *build time* (this
//! crate depends on `tulip_rs` as a build-dependency purely for this).
//!
//! Emits three files from the one `indicators!` list below:
//!   1. `include/tulip_rs_ffi_counts.h` -- one `#define <NAME>_INPUTS N` /
//!      `#define <NAME>_OPTIONS N` pair per indicator, read from the real
//!      `tulip_rs::indicators::<name>::{INPUTS, OPTIONS}` constants.
//!   2. `src/state_ids_generated.rs` -- the `CIndicatorId` enum, its
//!      `TryFrom<u32>`, name lookups, and the serialize/deserialize/clone
//!      dispatchers consumed by `src/state_registry.rs` (via `include!`).
//!   3. `include/tulip_rs_ffi_state_ids.h` -- `#define C_INDICATOR_ID_<NAME>`
//!      constants for C consumers (cbindgen cannot see items behind an
//!      `include!`, so the C side is emitted here, not from the enum).
//!
//! This is deliberately not done via `cbindgen`: cbindgen only *parses*
//! Rust source, it never evaluates it, so a `pub const ADOSC_INPUTS: usize
//! = tulip_rs::indicators::adosc::INPUTS;` in `src/adosc.rs` would come out
//! as the broken macro `#define ADOSC_INPUTS INPUTS` (verified against
//! cbindgen 0.29.4). Compiling this build script, by contrast, actually
//! runs the real Rust code, so the numbers it writes out can never drift
//! from the core crate -- there's nothing to keep in sync by hand.
//!
//! `include/tulip_rs_ffi.h` (cbindgen's generated header, the single header
//! both external consumers and the bundled examples build against) pulls
//! the generated headers in automatically via `after_includes` in
//! `cbindgen.toml`.
//!
//! The only thing maintained by hand here is the list of indicator names
//! below -- and that's not new upkeep: adding an indicator already requires
//! adding `pub mod <name>;` to `src/lib.rs` and writing `src/<name>.rs`, so
//! adding one more line to this list is the same class of edit, not an
//! extra source of truth for the *values*.
//!
//! # Indicator ids (the `CIndicatorId` discriminants)
//!
//! Ids are NOT positional: each is `fnv1a32(<lowercase indicator name>)`, a
//! pure function of the indicator's identity. The lowercase name doubles as
//! the authoritative wire identity (embedded in the `TRFS` blob header --
//! see `src/state_registry.rs`). Consequences:
//!   - Adding or removing indicators never renumbers any existing id, so
//!     persisted state blobs stay valid regardless of where a new name
//!     lands in the list below (the list may freely stay alphabetical).
//!   - Two maintainers adding different indicators concurrently can never
//!     race for "the next id" -- there is no next id.
//!   - If two names ever hashed to the same u32 (birthday odds over a few
//!     hundred names: ~0.005%), this build script panics with both names
//!     listed; the escape hatch is to pin one of them explicitly in
//!     `ID_OVERRIDES` below. The panic is the *only* failure mode -- a
//!     silent collision cannot reach a compiler (duplicate enum
//!     discriminants are themselves a hard compile error).
//!   - Because the hash function IS the id space, it must never change.
//!     `fnv1a32` below is pinned by reference test vectors, mirrored (and
//!     re-pinned) in `src/state_registry.rs`'s tests. A hash change is a
//!     wire-format breaking change (bump the TRFS schema version with it).

use std::collections::HashMap;
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

/// Fixed width of the indicator-name field in the TRFS blob header. Names
/// longer than this fail the build (see check in `id_entries`).
const NAME_FIELD: usize = 32;

/// Explicit id pins, keyed by lowercase indicator name. Empty by design:
/// only add an entry here if `fnv1a32` actually collided (build panic will
/// name the two indicators). Once pinned, an override id is wire data and
/// may never change.
const ID_OVERRIDES: &[(&str, u32)] = &[];

/// FNV-1a, 32-bit. Reference: http://www.isthe.com/chongo/tech/comp/fnv/
///
/// MUST stay in sync with `state_registry.rs` tests (pinned by vectors).
/// Hand-rolled arithmetic on purpose: `std::hash` explicitly guarantees no
/// stability across releases and must never back a wire identity.
fn fnv1a32(name: &[u8]) -> u32 {
    let mut hash: u32 = 0x811c9dc5;
    for &b in name {
        hash ^= b as u32;
        hash = hash.wrapping_mul(0x01000193);
    }
    hash
}

/// Build-time sanity of the hash itself (also asserted in src tests).
const _: () = {
    // empty string is the FNV offset basis; these three are the official
    // FNV-1a 32-bit reference vectors.
    assert!(fnv1a32_const(b"") == 0x811c9dc5);
    assert!(fnv1a32_const(b"a") == 0xe40c292c);
    assert!(fnv1a32_const(b"foobar") == 0xbf9cf968);
};

const fn fnv1a32_const(name: &[u8]) -> u32 {
    let mut hash: u32 = 0x811c9dc5;
    let mut i = 0;
    while i < name.len() {
        hash ^= name[i] as u32;
        hash = hash.wrapping_mul(0x01000193);
        i += 1;
    }
    hash
}

/// ADX -> Adx, AROONOSC -> Aroonosc (variant names: unique, so long as the
/// prefixes themselves are, which they are by construction).
fn prefix_to_variant_name(prefix: &str) -> String {
    let mut chars = prefix.chars();
    let first = chars
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_default();
    format!("{}{}", first, chars.as_str().to_lowercase())
}

/// One (prefix, inputs, options) row per indicator -- THE list.
macro_rules! indicators {
    ($($prefix:literal => ($inputs:expr, $options:expr)),+ $(,)?) => {{
        vec![$(( $prefix, $inputs, $options ),)+]
    }};
}

fn id_entries(rows: &[(&'static str, usize, usize)]) -> Vec<(&'static str, String, String, u32)> {
    let overrides: HashMap<&str, u32> = ID_OVERRIDES.iter().copied().collect();
    let mut out: Vec<(&'static str, String, String, u32)> = Vec::with_capacity(rows.len());
    let mut seen: HashMap<u32, &'static str> = HashMap::new();

    for &(prefix, _inputs, _options) in rows {
        let name = prefix.to_lowercase();
        assert!(
            name.len() <= NAME_FIELD,
            "indicator name {name:?} ({}/{} chars) does not fit the {NAME_FIELD}-byte TRFS header name field",
            name.len(),
            NAME_FIELD,
        );
        let id = match overrides.get(name.as_str()) {
            Some(&pinned) => pinned,
            None => fnv1a32(name.as_bytes()),
        };
        if let Some(prev) = seen.insert(id, prefix) {
            panic!(
                "indicator id collision: {prev} and {prefix} both hash to {:#x}. \
                 Pin one of them in ID_OVERRIDES in build.rs (key = lowercase name).",
                id
            );
        }
        out.push((prefix, name, prefix_to_variant_name(prefix), id));
    }
    // An override for a name not in the list is stale bookkeeping -- fail
    // rather than let it rot silently.
    for &(name, _) in ID_OVERRIDES.iter() {
        assert!(
            rows.iter().any(|(p, _, _)| p.to_lowercase() == name),
            "ID_OVERRIDES contains an entry for {name:?} which is not in the indicator list",
        );
    }
    out
}

/// Byte-for-byte the same content the old `counts_header!` emitted.
fn counts_header(rows: &[(&'static str, usize, usize)]) -> String {
    let mut out = String::new();
    out.push_str("// Auto-generated by tulip_rs_ffi/build.rs from the corresponding\n");
    out.push_str("// tulip_rs::indicators::<name>::{INPUTS, OPTIONS} constants.\n");
    out.push_str("// Do not edit by hand -- edit the core crate instead and rebuild.\n\n");
    out.push_str("#ifndef TULIP_RS_FFI_COUNTS_H\n#define TULIP_RS_FFI_COUNTS_H\n\n");
    for (prefix, inputs, options) in rows {
        writeln!(
            out,
            "#define {p}_INPUTS {i}\n#define {p}_OPTIONS {o}\n",
            p = prefix,
            i = inputs,
            o = options,
        )
        .unwrap();
    }
    out.push_str("#endif // TULIP_RS_FFI_COUNTS_H\n");
    out
}

fn state_ids_rs(entries: &[(&'static str, String, String, u32)]) -> String {
    let mut out = String::new();
    out.push_str("// DO NOT EDIT -- generated by build.rs on every build.\n");
    out.push_str("//\n");
    out.push_str("// `CIndicatorId` discriminants are `fnv1a32(<lowercase name>)` -- a\n");
    out.push_str("// pure function of each indicator's identity, so adding or removing\n");
    out.push_str("// indicators never renumbers an existing id (see build.rs docs).\n");
    out.push_str("// The lowercase name is also the authoritative wire identity embedded\n");
    out.push_str("// in TRFS state blobs.\n\n");
    out.push_str(
        "#[repr(u32)]\n#[derive(Clone, Copy, Debug, PartialEq, Eq)]\npub enum CIndicatorId {\n",
    );
    for (prefix, name, variant, id) in entries {
        writeln!(out, "    /// {prefix} (id = fnv1a32({name:?}))").unwrap();
        writeln!(out, "    {variant} = {id:#x},").unwrap();
    }
    out.push_str("}\n\n");

    out.push_str("impl CIndicatorId {\n");
    out.push_str("    /// Every indicator id, in build.rs list order.\n");
    out.push_str("    pub(crate) const ALL: &'static [CIndicatorId] = &[\n");
    for (_, _, variant, _) in entries {
        writeln!(out, "        Self::{variant},").unwrap();
    }
    out.push_str("    ];\n\n");
    out.push_str("    /// Canonical wire name for this indicator (lowercase, unpadded).\n");
    out.push_str("    pub(crate) fn name(self) -> &'static [u8] {\n        match self {\n");
    for (_, name, variant, _) in entries {
        writeln!(out, "            Self::{variant} => b\"{name}\",").unwrap();
    }
    out.push_str("        }\n    }\n\n");
    out.push_str("    /// Reverse lookup: canonical wire name -> indicator id.\n");
    out.push_str("    pub(crate) fn from_name(name: &[u8]) -> Option<CIndicatorId> {\n");
    out.push_str("        Self::ALL.iter().copied().find(|id| id.name() == name)\n");
    out.push_str("    }\n}\n\n");

    out.push_str("impl TryFrom<u32> for CIndicatorId {\n    type Error = ();\n\n    fn try_from(value: u32) -> Result<Self, Self::Error> {\n        match value {\n");
    for (_, _, variant, id) in entries {
        writeln!(out, "            {id:#x} => Ok(Self::{variant}),").unwrap();
    }
    out.push_str("            _ => Err(()),\n        }\n    }\n}\n\n");

    out.push_str("/// Dispatch serialization to the concrete state type for `id`.\n");
    out.push_str("///\n/// # Safety\n/// `state` must be null or a valid boxed state pointer of `id`'s\n/// concrete `IndicatorState` type (see `state_registry.rs`).\n");
    out.push_str("pub(crate) unsafe fn serialize_dispatch(\n    id: CIndicatorId,\n    fmt: CStateFormat,\n    state: *const c_void,\n) -> crate::common::CBytes {\n    match id {\n");
    for (_, _, variant, _) in entries {
        writeln!(out, "        CIndicatorId::{variant} => serialize_state::<tulip_rs::indicators::{}::IndicatorState>(id, fmt, state),", variant.to_lowercase()).unwrap();
    }
    out.push_str("    }\n}\n\n");

    out.push_str("/// Dispatch deserialization to the concrete state type for `id`.\n///\n/// # Safety\n/// `payload`/`payload_len` must be a dereferenceable byte range (any\n/// contents are safe: malformed data yields null, never UB).\n");
    out.push_str("pub(crate) unsafe fn deserialize_dispatch(\n    id: CIndicatorId,\n    fmt: CStateFormat,\n    payload: *const u8,\n    payload_len: usize,\n) -> *mut c_void {\n    match id {\n");
    for (_, _, variant, _) in entries {
        writeln!(out, "        CIndicatorId::{variant} => deserialize_state::<tulip_rs::indicators::{}::IndicatorState>(fmt, payload, payload_len),", variant.to_lowercase()).unwrap();
    }
    out.push_str("    }\n}\n\n");

    out.push_str("/// Dispatch a deep copy to the concrete state type for `id`.\n///\n/// # Safety\n/// `state` must be a valid boxed state pointer of `id`'s concrete\n/// `IndicatorState` type.\n");
    out.push_str("pub(crate) unsafe fn clone_dispatch(id: CIndicatorId, state: *const c_void) -> *mut c_void {\n    match id {\n");
    for (_, _, variant, _) in entries {
        writeln!(out, "        CIndicatorId::{variant} => clone_state::<tulip_rs::indicators::{}::IndicatorState>(state),", variant.to_lowercase()).unwrap();
    }
    out.push_str("    }\n}\n");
    out
}

fn state_ids_h(entries: &[(&'static str, String, String, u32)]) -> String {
    let mut out = String::new();
    out.push_str("// Auto-generated by tulip_rs_ffi/build.rs. Do not edit by hand.\n");
    out.push_str("//\n// Indicator ids for tulip_state_serialize()/tulip_state_clone().\n// Each value is fnv1a32(<lowercase indicator name>) and is stable for the\n// lifetime of that name: adding indicators never renumbers existing ids.\n// Serialized state blobs embed the name itself (TRFS header), so ids are\n// an API convenience, not the wire authority.\n\n");
    out.push_str("#ifndef TULIP_RS_FFI_STATE_IDS_H\n#define TULIP_RS_FFI_STATE_IDS_H\n\n");
    for (prefix, name, _variant, id) in entries {
        writeln!(
            out,
            "#define C_INDICATOR_ID_{prefix} {id:#x} /* fnv1a32(\"{name}\") */"
        )
        .unwrap();
    }
    out.push_str("\n#endif // TULIP_RS_FFI_STATE_IDS_H\n");
    out
}

fn main() {
    let rows = indicators! {
        "AD" => (tulip_rs::indicators::ad::INPUTS, tulip_rs::indicators::ad::OPTIONS),
        "ADAPTIVEMSW" => (tulip_rs::indicators::adaptivemsw::INPUTS, tulip_rs::indicators::adaptivemsw::OPTIONS),
        "ADOSC" => (tulip_rs::indicators::adosc::INPUTS, tulip_rs::indicators::adosc::OPTIONS),
        "ADX" => (tulip_rs::indicators::adx::INPUTS, tulip_rs::indicators::adx::OPTIONS),
        "ADXR" => (tulip_rs::indicators::adxr::INPUTS, tulip_rs::indicators::adxr::OPTIONS),
        "AO" => (tulip_rs::indicators::ao::INPUTS, tulip_rs::indicators::ao::OPTIONS),
        "APO" => (tulip_rs::indicators::apo::INPUTS, tulip_rs::indicators::apo::OPTIONS),
        "AROON" => (tulip_rs::indicators::aroon::INPUTS, tulip_rs::indicators::aroon::OPTIONS),
        "AROONOSC" => (tulip_rs::indicators::aroonosc::INPUTS, tulip_rs::indicators::aroonosc::OPTIONS),
        "ATR" => (tulip_rs::indicators::atr::INPUTS, tulip_rs::indicators::atr::OPTIONS),
        "AVGPRICE" => (tulip_rs::indicators::avgprice::INPUTS, tulip_rs::indicators::avgprice::OPTIONS),
        "BBANDS" => (tulip_rs::indicators::bbands::INPUTS, tulip_rs::indicators::bbands::OPTIONS),
        "BOP" => (tulip_rs::indicators::bop::INPUTS, tulip_rs::indicators::bop::OPTIONS),
        "CANDLESTICK" => (tulip_rs::indicators::candlestick::INPUTS, tulip_rs::indicators::candlestick::OPTIONS),
        "CCFISHER" => (tulip_rs::indicators::ccfisher::INPUTS, tulip_rs::indicators::ccfisher::OPTIONS),
        "CCI" => (tulip_rs::indicators::cci::INPUTS, tulip_rs::indicators::cci::OPTIONS),
        "CHAIKINMF" => (tulip_rs::indicators::chaikinmf::INPUTS, tulip_rs::indicators::chaikinmf::OPTIONS),
        "CHANDELIEREXIT" => (tulip_rs::indicators::chandelierexit::INPUTS, tulip_rs::indicators::chandelierexit::OPTIONS),
        "CMO" => (tulip_rs::indicators::cmo::INPUTS, tulip_rs::indicators::cmo::OPTIONS),
        "CVI" => (tulip_rs::indicators::cvi::INPUTS, tulip_rs::indicators::cvi::OPTIONS),
        "CYBERCYCLE" => (tulip_rs::indicators::cybercycle::INPUTS, tulip_rs::indicators::cybercycle::OPTIONS),
        "DEMA" => (tulip_rs::indicators::dema::INPUTS, tulip_rs::indicators::dema::OPTIONS),
        "DI" => (tulip_rs::indicators::di::INPUTS, tulip_rs::indicators::di::OPTIONS),
        "DM" => (tulip_rs::indicators::dm::INPUTS, tulip_rs::indicators::dm::OPTIONS),
        "DONCHIANCHANNEL" => (tulip_rs::indicators::donchianchannel::INPUTS, tulip_rs::indicators::donchianchannel::OPTIONS),
        "DPO" => (tulip_rs::indicators::dpo::INPUTS, tulip_rs::indicators::dpo::OPTIONS),
        "DX" => (tulip_rs::indicators::dx::INPUTS, tulip_rs::indicators::dx::OPTIONS),
        "EF" => (tulip_rs::indicators::ef::INPUTS, tulip_rs::indicators::ef::OPTIONS),
        "ELDERRAY" => (tulip_rs::indicators::elderray::INPUTS, tulip_rs::indicators::elderray::OPTIONS),
        "EMA" => (tulip_rs::indicators::ema::INPUTS, tulip_rs::indicators::ema::OPTIONS),
        "EMV" => (tulip_rs::indicators::emv::INPUTS, tulip_rs::indicators::emv::OPTIONS),
        "FISHER" => (tulip_rs::indicators::fisher::INPUTS, tulip_rs::indicators::fisher::OPTIONS),
        "FOSC" => (tulip_rs::indicators::fosc::INPUTS, tulip_rs::indicators::fosc::OPTIONS),
        "HIGHPASS" => (tulip_rs::indicators::highpass::INPUTS, tulip_rs::indicators::highpass::OPTIONS),
        "HILBERTTRANSFORM" => (tulip_rs::indicators::hilberttransform::INPUTS, tulip_rs::indicators::hilberttransform::OPTIONS),
        "HMA" => (tulip_rs::indicators::hma::INPUTS, tulip_rs::indicators::hma::OPTIONS),
        "HOMODYNEDISCRIMINATOR" => (tulip_rs::indicators::homodynediscriminator::INPUTS, tulip_rs::indicators::homodynediscriminator::OPTIONS),
        "ICHIMOKU" => (tulip_rs::indicators::ichimoku::INPUTS, tulip_rs::indicators::ichimoku::OPTIONS),
        "INSTANTANEOUSTRENDLINE" => (tulip_rs::indicators::instantaneoustrendline::INPUTS, tulip_rs::indicators::instantaneoustrendline::OPTIONS),
        "KAMA" => (tulip_rs::indicators::kama::INPUTS, tulip_rs::indicators::kama::OPTIONS),
        "KELTNERCHANNEL" => (tulip_rs::indicators::keltnerchannel::INPUTS, tulip_rs::indicators::keltnerchannel::OPTIONS),
        "KVO" => (tulip_rs::indicators::kvo::INPUTS, tulip_rs::indicators::kvo::OPTIONS),
        "LINREG" => (tulip_rs::indicators::linreg::INPUTS, tulip_rs::indicators::linreg::OPTIONS),
        "MACD" => (tulip_rs::indicators::macd::INPUTS, tulip_rs::indicators::macd::OPTIONS),
        "MAMA" => (tulip_rs::indicators::mama::INPUTS, tulip_rs::indicators::mama::OPTIONS),
        "MARKETFI" => (tulip_rs::indicators::marketfi::INPUTS, tulip_rs::indicators::marketfi::OPTIONS),
        "MASS" => (tulip_rs::indicators::mass::INPUTS, tulip_rs::indicators::mass::OPTIONS),
        "MAX" => (tulip_rs::indicators::max::INPUTS, tulip_rs::indicators::max::OPTIONS),
        "MD" => (tulip_rs::indicators::md::INPUTS, tulip_rs::indicators::md::OPTIONS),
        "MEDPRICE" => (tulip_rs::indicators::medprice::INPUTS, tulip_rs::indicators::medprice::OPTIONS),
        "MFI" => (tulip_rs::indicators::mfi::INPUTS, tulip_rs::indicators::mfi::OPTIONS),
        "MIN" => (tulip_rs::indicators::min::INPUTS, tulip_rs::indicators::min::OPTIONS),
        "MOM" => (tulip_rs::indicators::mom::INPUTS, tulip_rs::indicators::mom::OPTIONS),
        "MSW" => (tulip_rs::indicators::msw::INPUTS, tulip_rs::indicators::msw::OPTIONS),
        "NATR" => (tulip_rs::indicators::natr::INPUTS, tulip_rs::indicators::natr::OPTIONS),
        "NVI" => (tulip_rs::indicators::nvi::INPUTS, tulip_rs::indicators::nvi::OPTIONS),
        "OBV" => (tulip_rs::indicators::obv::INPUTS, tulip_rs::indicators::obv::OPTIONS),
        "PIVOTPOINT" => (tulip_rs::indicators::pivotpoint::INPUTS, tulip_rs::indicators::pivotpoint::OPTIONS),
        "PPO" => (tulip_rs::indicators::ppo::INPUTS, tulip_rs::indicators::ppo::OPTIONS),
        "PSAR" => (tulip_rs::indicators::psar::INPUTS, tulip_rs::indicators::psar::OPTIONS),
        "PVI" => (tulip_rs::indicators::pvi::INPUTS, tulip_rs::indicators::pvi::OPTIONS),
        "QSTICK" => (tulip_rs::indicators::qstick::INPUTS, tulip_rs::indicators::qstick::OPTIONS),
        "ROC" => (tulip_rs::indicators::roc::INPUTS, tulip_rs::indicators::roc::OPTIONS),
        "ROCR" => (tulip_rs::indicators::rocr::INPUTS, tulip_rs::indicators::rocr::OPTIONS),
        "ROOFINGFILTER" => (tulip_rs::indicators::roofingfilter::INPUTS, tulip_rs::indicators::roofingfilter::OPTIONS),
        "RSI" => (tulip_rs::indicators::rsi::INPUTS, tulip_rs::indicators::rsi::OPTIONS),
        "SMA" => (tulip_rs::indicators::sma::INPUTS, tulip_rs::indicators::sma::OPTIONS),
        "SMAENVELOPE" => (tulip_rs::indicators::smaenvelope::INPUTS, tulip_rs::indicators::smaenvelope::OPTIONS),
        "STDDEV" => (tulip_rs::indicators::stddev::INPUTS, tulip_rs::indicators::stddev::OPTIONS),
        "STOCH" => (tulip_rs::indicators::stoch::INPUTS, tulip_rs::indicators::stoch::OPTIONS),
        "STOCHRSI" => (tulip_rs::indicators::stochrsi::INPUTS, tulip_rs::indicators::stochrsi::OPTIONS),
        "SUPERSMOOTHER" => (tulip_rs::indicators::supersmoother::INPUTS, tulip_rs::indicators::supersmoother::OPTIONS),
        "SUPERTREND" => (tulip_rs::indicators::supertrend::INPUTS, tulip_rs::indicators::supertrend::OPTIONS),
        "TEMA" => (tulip_rs::indicators::tema::INPUTS, tulip_rs::indicators::tema::OPTIONS),
        "TR" => (tulip_rs::indicators::tr::INPUTS, tulip_rs::indicators::tr::OPTIONS),
        "TRENDMODE" => (tulip_rs::indicators::trendmode::INPUTS, tulip_rs::indicators::trendmode::OPTIONS),
        "TRIMA" => (tulip_rs::indicators::trima::INPUTS, tulip_rs::indicators::trima::OPTIONS),
        "TRIX" => (tulip_rs::indicators::trix::INPUTS, tulip_rs::indicators::trix::OPTIONS),
        "TRVI" => (tulip_rs::indicators::trvi::INPUTS, tulip_rs::indicators::trvi::OPTIONS),
        "TSF" => (tulip_rs::indicators::tsf::INPUTS, tulip_rs::indicators::tsf::OPTIONS),
        "TYPPRICE" => (tulip_rs::indicators::typprice::INPUTS, tulip_rs::indicators::typprice::OPTIONS),
        "ULTOSC" => (tulip_rs::indicators::ultosc::INPUTS, tulip_rs::indicators::ultosc::OPTIONS),
        "VHF" => (tulip_rs::indicators::vhf::INPUTS, tulip_rs::indicators::vhf::OPTIONS),
        "VIDYA" => (tulip_rs::indicators::vidya::INPUTS, tulip_rs::indicators::vidya::OPTIONS),
        "VOLATILITY" => (tulip_rs::indicators::volatility::INPUTS, tulip_rs::indicators::volatility::OPTIONS),
        "VORTEX" => (tulip_rs::indicators::vortex::INPUTS, tulip_rs::indicators::vortex::OPTIONS),
        "VOSC" => (tulip_rs::indicators::vosc::INPUTS, tulip_rs::indicators::vosc::OPTIONS),
        "VWAP" => (tulip_rs::indicators::vwap::INPUTS, tulip_rs::indicators::vwap::OPTIONS),
        "VWMA" => (tulip_rs::indicators::vwma::INPUTS, tulip_rs::indicators::vwma::OPTIONS),
        "WAD" => (tulip_rs::indicators::wad::INPUTS, tulip_rs::indicators::wad::OPTIONS),
        "WCPRICE" => (tulip_rs::indicators::wcprice::INPUTS, tulip_rs::indicators::wcprice::OPTIONS),
        "WILDERS" => (tulip_rs::indicators::wilders::INPUTS, tulip_rs::indicators::wilders::OPTIONS),
        "WILLR" => (tulip_rs::indicators::willr::INPUTS, tulip_rs::indicators::willr::OPTIONS),
        "WMA" => (tulip_rs::indicators::wma::INPUTS, tulip_rs::indicators::wma::OPTIONS),
        "ZLEMA" => (tulip_rs::indicators::zlema::INPUTS, tulip_rs::indicators::zlema::OPTIONS),
    };

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();

    let counts = counts_header(&rows);
    let counts_dest = Path::new(&manifest_dir)
        .join("include")
        .join("tulip_rs_ffi_counts.h");
    fs::write(&counts_dest, &counts)
        .unwrap_or_else(|e| panic!("failed to write {}: {}", counts_dest.display(), e));

    let entries = id_entries(&rows);

    let ids_rs = state_ids_rs(&entries);
    let ids_rs_dest = Path::new(&manifest_dir)
        .join("src")
        .join("state_ids_generated.rs");
    fs::write(&ids_rs_dest, &ids_rs)
        .unwrap_or_else(|e| panic!("failed to write {}: {}", ids_rs_dest.display(), e));

    let ids_h = state_ids_h(&entries);
    let ids_h_dest = Path::new(&manifest_dir)
        .join("include")
        .join("tulip_rs_ffi_state_ids.h");
    fs::write(&ids_h_dest, &ids_h)
        .unwrap_or_else(|e| panic!("failed to write {}: {}", ids_h_dest.display(), e));

    println!("cargo:rerun-if-changed=../tulip_rs/tulip_rs/src");
    println!("cargo:rerun-if-changed=build.rs");
}
