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

Currently covers two indicators (`adosc`, `macd`) as a proof of concept
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

```
CIndicatorResult adosc_indicator(high, low, close, volume, size,
                                  short_period, long_period,
                                  optional_outputs, num_optional);
// -> { error, outputs, output_lens, num_outputs, state }

CBatchResult adosc_batch(state, high, low, close, volume, size,
                          optional_outputs, num_optional);
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

## Verifying

`verify.c` is a small manual smoke test exercising both indicators,
including batch/streaming continuation:

```bash
cc -O2 -o verify verify.c -L target/release -ltulip_rs_ffi \
   -Wl,-rpath,target/release
./verify
```

## Status / next steps

- [x] `adosc` (4 inputs, 2 options, 1 mandatory + 3 optional outputs)
- [x] `macd` (1 input, 3 options, 3 mandatory + 2 optional outputs)
- [ ] Expand to full indicator coverage (candlestick patterns need separate
      handling, same as `tulip_rs_diplomat`)
- [ ] Auto-generate a C header (e.g. via `cbindgen`) instead of hand-declaring
      prototypes in consuming code
- [ ] Benchmark harness (mirroring `tulip_rs_diplomat/bench/c`)
