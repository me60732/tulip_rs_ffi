# tulip_rs_ffi

A hand-rolled `extern "C"` FFI layer over [`tulip_rs`](https://github.com/me60732/tulip_rs) (crates.io: `tulip_rs`), exposing 95 indicator modules — 94 technical indicators plus 77+ candlestick patterns — via raw-pointer, Tulip-Indicators-style calling conventions.

No wrapper library or language runtime required — just `#[no_mangle] extern "C"` functions consumable from C, C++, or any language with a C ABI.

📖 **[Full documentation for the tulip_rs project](https://me60732.github.io/tulip_rs/)**

---

## Coverage

**95 indicator modules** — the 94 standard f64-output indicators of `tulip_rs`, plus the **candlestick patterns module** (77+ patterns, CSR-packed ids, its own API family documented below):

- Each indicator exposes a 6-function family:
  - `<ind>_info()` — metadata (name, inputs, options, outputs)
  - `<ind>_min_data(options)` — minimum input bars required
  - `<ind>_indicator(inputs, data_len, options, optional_outputs, num_optional)` — full calculation
  - `<ind>_batch(state, inputs, data_len, optional_outputs, num_optional)` — streaming continuation (stateful)
  - `<ind>_state_free(state)` — free the indicator state
- **SIMD variants** where supported:
  - `<ind>_simd_by_assets(inputs, num_assets, data_len, options, optional_outputs, numoptional)`
    - compute one option set across N=2/4/8/16 assets simultaneously
  - `<ind>_simd_by_options(inputs, data_len, options, num_option_sets, optional_outputs, numoptional)`
    - compute one asset across N=2/4/8/16 different option sets simultaneously
- **Option-less indicators** (ad, ao, bop, obv, ...) have `*_by_assets` only.

**Candlestick** uses a different API family:
- `<ind>_num_patterns()` — total pattern count
- `<ind>_pattern_info(id)` — metadata for a specific pattern
- `<ind>_indicator(inputs, data_len, options)` — CSR-packed output (bar_offsets + pattern_ids)
- `<ind>_batch(state, inputs, data_len, options)` — streaming continuation
- `candlestick_result_free(result)` / `candlestick_batch_result_free(result)` / `candlestick_state_free(state)`

See `include/tulip_rs_ffi.h` for the complete generated API surface.

---

## Design

### Allocation-in-wrapper

All output buffers are allocated by the indicator function itself (never caller-supplied). The caller only receives pointers to Rust-allocated memory and must free them via `tulip_ffi_result_free()`, `tulip_ffi_batch_result_free()`, or `tulip_ffi_simd_result_free()`.

### State/results separation

`<ind>_indicator()` returns a `CIndicatorResult` containing:
- output buffers (`outputs[i]` + `output_lens[i]`)
- an opaque `state` pointer (boxed `<Name>::IndicatorState`)

Calling `tulip_ffi_result_free(result)` frees **only** the outputs — the state remains valid for subsequent `<ind>_batch()` calls.

### Streaming continuation

The core Rust API's `IndicatorState::batch_indicator(&mut self, ...)` mutates state in place and returns new outputs only. The FFI mirrors this:
- `<ind>_indicator()` creates state and returns first outputs
- `<ind>_batch(state, ...)` continues streaming, mutating the same state pointer

### Parameter-order convention

Tulip-Indicators style: every pointer parameter is immediately followed by its count(s).

```c
CIndicatorResult ema_indicator(
    double const *const *inputs,   // [EMA_INPUTS] pointers
    size_t data_len,               // bars per series
    double const *options,         // [EMA_OPTIONS] flat array
    bool const *optional_outputs,  // [num_optional]
    size_t num_optional);          // count of optional outputs
```

### Output row order

Mandatory outputs first, then optional outputs in the order declared in `tulip_rs::indicators::<name>::INFO`.

---

## Building

```bash
# Initialize reference-library submodules (bench only, for C/TA-Lib comparison)
git submodule update --init bench/tulip_indicators bench/ta_lib_src

# Build the cdylib/staticlib
cargo build --release
```

Produces `target/release/libtulip_rs_ffi.{so,a}` (crate-type: `cdylib`, `staticlib`, `lib`).

Requires **nightly** (`rust-toolchain.toml`: pinned `nightly-2026-08-31`) due to `portable_simd` in `tulip_rs`.

**Critical**: `.cargo/config.toml` sets `-C target-cpu=native`. This is required for benchmark parity with the reference libraries (Tulip Indicators C and TA-Lib). Do not override this when building benchmarks.

---

## Prebuilt release binaries

`.github/workflows/release.yml`: on every `v*` tag it builds the cdylib for
`linux-amd64`, `linux-arm64`, `darwin-amd64`, `darwin-arm64`, and a Windows
static lib for `windows-amd64`, attaching
`tulip_rs_ffi-<os>-<arch>.tar.gz` (`lib/` + `include/`) to the GitHub release.
Windows ships `libtulip_rs_ffi.a` (not the DLL): cgo there is mingw-w64 GCC
and PE has no rpath, so static linking avoids DLL-next-to-exe games.

CI **must not** inherit `target-cpu=native` — the runner's CPU is not the
user's, and the artifact would `SIGILL` at runtime. The workflow therefore
overrides `RUSTFLAGS` to portable baselines: `-C target-cpu=x86-64-v3` on
x86_64 (AVX2-class, 2013+) and `-C target-cpu=generic` on aarch64 (NEON is
mandatory since ARMv8). Language bindings expose both consumption paths — a
portable prebuilt download and a native source build (see each binding's
bootstrap docs).

---

## Headers

Two headers live in `include/`:

1. **`tulip_rs_ffi.h`** — cbindgen-generated API surface
2. **`tulip_rs_ffi_counts.h`** — auto-generated `#define <NAME>_INPUTS N` / `#define <NAME>_OPTIONS N` per indicator

Consumers only need to `#include "tulip_rs_ffi.h"` — it pulls in the counts header itself, so the `*_INPUTS` / `*_OPTIONS` constants are always available alongside the API.

### Regenerating headers

```bash
# Counts header (regenerated automatically by cargo build via build.rs)
cargo build

# API header (only needed after changing extern "C" surface)
cbindgen --config cbindgen.toml --output include/tulip_rs_ffi.h
```

Always run `cargo build` at least once before compiling anything against these headers.

---

## Quick start

```c
#include <stdio.h>
#include "../include/tulip_rs_ffi.h"

int main(void) {
    const double close[] = {81.59, 81.06, 82.87, 83.00, 83.61};
    
    // EMA with period=5 (EMA_INPUTS=1, EMA_OPTIONS=1)
    const double options[EMA_OPTIONS] = {5.0};
    const double *inputs[EMA_INPUTS] = {close};

    CIndicatorResult r = ema_indicator(inputs, 5, options, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "error=%d\n", r.error);
        return 1;
    }

    // Use outputs[0]... (ema result)
    for (size_t i = 0; i < r.output_lens[0]; i++) {
        printf("%.4f\n", r.outputs[0][i]);
    }

    // Free only the outputs
    tulip_ffi_result_free(r);
    // Free the state if you kept it for streaming
    ema_state_free(r.state);

    return 0;
}
```

Build:
```bash
cc -O2 -o example example.c \
    -I include \
    -L target/release -ltulip_rs_ffi -Wl,-rpath,target/release
./example
```

See `examples/ema_example.c` for a complete example (full + partial+batch continuation + SIMD).

---

## Examples

`examples/` contains one `.c` file per indicator, mirroring the Python binding's example data and flow:

1. Full calculation with all optional outputs requested
2. Partial calculation → state handle → batch continuation
3. Verification that partial+continued matches a full recompute
4. SIMD demonstrations (by-assets/by-options where supported)

Candlestick (`examples/candlestick_example.c`) demonstrates the CSR-packed pattern-id output and metadata lookup via `candlestick_pattern_info()`.

---

## Benchmarks

See [`bench/README.md`](bench/README.md) for complete methodology.

A C benchmark harness that:
- Benchmarks all 94 non-candlestick indicators
- Compares against Tulip Indicators (C) and TA-Lib in the same process
- Logs results to the shared `indicator_benchmark` Postgres database

Build and run everything end-to-end with `bench/run_bench.sh` (cargo build →
make → full suite). Submodules: `bench/tulip_indicators`, `bench/ta_lib_src`

---

## Layout

```
tulip_rs_ffi/
  src/           One module per indicator (plus ffi_common, candlestick)
  include/       tulip_rs_ffi.h (cbindgen) + tulip_rs_ffi_counts.h (build.rs)
  examples/      One .c example per indicator
  bench/         C benchmark harness vs Tulip Indicators / TA-Lib
    bench_indicators/  One <name>.c per indicator (driver closures)
    tulip_indicators/  Git submodule: reference C implementation
    ta_lib_src/        Git submodule: prebuilt TA-Lib .deb
  build.rs       Generates counts header from real tulip_rs constants
  cbindgen.toml  Header generation config
  rust-toolchain.toml  Nightly pinned (portable_simd requirement)
  .cargo/config.toml   target-cpu=native (benchmark parity requirement)
```

---

## License

MIT
