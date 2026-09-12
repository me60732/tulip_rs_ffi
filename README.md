# tulip_rs_ffi

A hand-rolled `extern "C"` FFI wrapper around the core [`tulip_rs`](../tulip_rs)
indicator library, using a Tulip-Indicators-style calling convention (raw
pointers in, a `Result`-style struct out) instead of Diplomat's generated
`DiplomatF64View` ABI (see `tulip_rs_diplomat` for that approach).

This was created after concluding that generating a Diplomat wrapper *around*
a hand-shaped Tulip-style signature would just be a wrapper around a wrapper
-- the same C-ABI ergonomics are achievable directly, with no Diplomat
dependency at all, by exposing `#[no_mangle] extern "C" fn`s straight from a
thin crate that depends on `tulip_rs` itself.

Currently covers two indicators (`adosc`, `macd`) with SIMD support as a proof of concept
before expanding to full coverage.

## Design

- **Output allocation stays inside the indicator function** (never
  caller-supplied), matching the core Rust API's `Vec<Vec<f64>>` ownership
  model -- the caller only ever receives pointers to Rust-allocated memory
  and must free them via the provided `*_result_free`/`*_batch_result_free`
  functions.
- **State and results are separate.** `<name>_indicator()` returns a
  `CIndicatorResult` containing both the output buffers *and* an opaque
  `state` pointer (a boxed `<Name>::IndicatorState`). Freeing the result
  (`tulip_ffi_result_free`) only releases the output buffers -- the state
  pointer stays valid until you explicitly call `<name>_state_free()`.
- **Streaming continuation mirrors the core API exactly.** The core
  library's `IndicatorState::batch_indicator(&mut self, ...)` mutates
  state in place and returns just the new outputs (no new state to hand
  back). `<name>_batch()` does the same: pass in the state pointer from
  `<name>_indicator()`, get a `CBatchResult` with the next chunk of
  outputs, and the same state pointer keeps working for further calls.

```c
// inputs: array of pointers, one per input series (Tulip-Indicators style,
// e.g. https://tulipindicators.org/adosc's ti_adosc(data_len, inputs, options, outputs))
// options: flat array, one entry per option (e.g. {short_period, long_period})
// Parameter order: every pointer parameter is immediately followed by the
// count(s) that describe it.
CIndicatorResult adosc_indicator(
    double const *const *inputs, size_t data_len, double const *options,
    bool const *optional_outputs, size_t num_optional);
// -> { error, outputs, output_lens, num_outputs, state }

CBatchResult adosc_batch(
    void *state, double const *const *inputs, size_t data_len,
    bool const *optional_outputs, size_t num_optional);
// -> { error, outputs, output_lens, num_outputs }   (state mutated in place)

tulip_ffi_result_free(result);        // frees outputs only
tulip_ffi_batch_result_free(result);  // frees outputs only
adosc_state_free(state);              // frees state, once you're done streaming
```

Output row order is fixed per indicator: mandatory outputs first, then
optional outputs in the order they're declared in the core `Indicator::INFO`
(`adosc`: `adosc`, then `short_ema`/`long_ema`/`ad` if requested; `macd`:
`macd_line`/`signal_line`/`histogram`, then `short_ema`/`long_ema` if
requested).

## Building

```bash
cargo build --release
```

Produces `target/release/libtulip_rs_ffi.{so,a}` (crate-type is
`cdylib`+`staticlib`+`lib`). Requires the same nightly toolchain as the core
`tulip_rs` crate (see `rust-toolchain.toml`), since `tulip_rs` uses the
unstable `portable_simd` feature internally.

This also runs `build.rs`, which (re)generates `include/tulip_rs_ffi_counts.h`
-- one `#define <NAME>_INPUTS N` / `#define <NAME>_OPTIONS N` pair per
indicator, read directly from the real `tulip_rs::indicators::<name>::{INPUTS,
OPTIONS}` constants (via a `[build-dependencies]` copy of `tulip_rs`), so
those counts can never drift from the core crate. `include/tulip_rs_ffi.h`
pulls it in automatically (`after_includes` in `cbindgen.toml`). Always run
`cargo build` at least once before compiling anything against these headers.

## Verifying

`verify.c` is a small manual smoke test exercising both indicators,
including batch/streaming continuation:

```bash
cc -O2 -o verify verify.c -L target/release -ltulip_rs_ffi \
   -Wl,-rpath,target/release
./verify
```

## Examples

`examples/` contains one example per indicator, using the same sample data
and options as the equivalent `tulip_rs_python/examples/ti_*_example.py`
script, so output can be sanity-compared across bindings. Each example:

1. Runs a full calculation with all optional outputs requested.
2. Runs a partial calculation (no optional outputs) to get a state handle.
3. Feeds the remaining data through `<name>_batch()` to continue streaming.
4. Verifies the partial+continued result matches a full recompute.

Each example includes the single generated `include/tulip_rs_ffi.h` (compile
with `-Iinclude`), so it always matches the current `libtulip_rs_ffi` build.

```bash
cargo build --release
cbindgen --config cbindgen.toml --output include/tulip_rs_ffi.h
cc -O2 -o adosc_example examples/adosc_example.c -Iinclude \
   -L target/release -ltulip_rs_ffi -Wl,-rpath,target/release
cc -O2 -o macd_example examples/macd_example.c -Iinclude \
   -L target/release -ltulip_rs_ffi -Wl,-rpath,target/release
cc -O2 -o candlestick_example examples/candlestick_example.c -Iinclude \
   -L target/release -ltulip_rs_ffi -Wl,-rpath,target/release

./adosc_example
./macd_example
./candlestick_example
```

`candlestick` is the exception to the `Vec<Vec<f64>>` output model (and has
no SIMD entry points): per bar it reports zero or more matched pattern ids,
CSR-packed into `bar_offsets`/`pattern_ids`, with metadata (name, Japanese
name, forecast, bars) resolved via `candlestick_pattern_info()`/
`candlestick_pattern_names()`. See `examples/candlestick_example.c`.

(`info()`/`min_data()` metadata functions -- exposed in the Python/Node/
Diplomat bindings -- aren't wrapped here yet, so the examples hardcode the
known input/option counts instead of querying them.)

## SIMD

The FFI exposes two SIMD entry points per indicator:
- `<name>_simd_by_assets`: compute the indicator for N assets simultaneously using SIMD,
  sharing a single options array. N must be 2, 4, 8, or 16 (SIMD lane width).
- `<name>_simd_by_options`: compute the indicator for one asset with N different option sets
  simultaneously using SIMD. N must be 2, 4, 8, or 16.

Both return a `CSimdResult` struct:
```c
typedef struct {
    CIndicatorError error;
    double ***outputs;        // [num_results][num_outputs] -> output row pointers
    size_t **output_lens;     // [num_results][num_outputs] -> row lengths
    size_t num_outputs;       // number of output rows per result
    void **states;            // [num_results] -> opaque state handles
    size_t num_results;       // N: number of parallel results (2/4/8/16)
} CSimdResult;
```

Memory ownership:
- `tulip_ffi_simd_result_free(result)` frees the outputs and the states array wrapper,
  but **does not** drop/free the individual boxed state pointers in `states[i]`.
- The caller must call `<name>_state_free()` on each `result.states[i]` before calling
  `tulip_ffi_simd_result_free(result)` to free everything correctly.
- Each `result.states[i]` is an ordinary `<Name>::IndicatorState` that can also be used
  with the non-SIMD `*_batch()`/`*_state_free()` functions.

## Status / next steps

- [x] `adosc` (4 inputs, 2 options, 1 mandatory + 3 optional outputs)
- [x] `macd` (1 input, 3 options, 3 mandatory + 2 optional outputs)
- [x] SIMD support for `adosc`/`macd` (`*_simd_by_assets`, `*_simd_by_options`)
- [x] Examples for `adosc`/`macd` matching the Python binding's example data
- [x] `candlestick` (special-cased: CSR-packed pattern-id output, no optional
      outputs, scalar-only) + matching C example
- [x] Expand to full indicator coverage: all 92 core `tulip_rs` indicators
      wrapped (each with `*_indicator`/`*_batch`/`*_state_free`/
      `*_simd_by_assets`/`*_simd_by_options` where the core supports it, plus
      `*_info`/`*_min_data`), with per-module Rust unit tests (550 passing,
      `cargo test --release`)
- [x] Auto-generated C header: `include/tulip_rs_ffi.h`, generated with
      `cbindgen --config cbindgen.toml --output include/tulip_rs_ffi.h`
      (regenerate after any change to the `#[no_mangle] extern "C"` surface)
- [x] Expose `info()`/`min_data()` metadata functions (`<name>_info` /
      `<name>_min_data`, mirroring the core `Info`/`min_data`) 
- [x] Benchmark harness (`bench/`, mirroring the former `tulip_rs_diplomat/bench/c`;
      all 94 non-candlestick indicators, with reference C_tulip/TA-Lib
      comparisons built from the `bench/tulip_indicators` and `bench/ta_lib_src`
      git submodules — see `bench/README.md`)
