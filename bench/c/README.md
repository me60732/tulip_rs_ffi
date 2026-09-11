# tulip_rs_ffi C Benchmark Harness

A small C program that benchmarks the `tulip_rs_ffi` hand-rolled `extern "C"`
bindings directly — no wrapper, no other language runtime — and logs results
into the same shared `indicator_benchmark` Postgres database used by every
other tulip-rs binding's benchmark suite (Python, Node, Swift/UniFFI, and the
former Diplomat C harness), so results are directly comparable via SQL.

All 94 non-candlestick indicators are covered (candlestick pattern
recognition is out of scope for this harness -- it uses a different,
hand-coded CSR-packed API shape and would need its own driver).

For each indicator it benchmarks, it also runs up to two independent
reference implementations for comparison, in the same process, on the same
data, where a genuine equivalent exists:

- **Tulip Indicators (C)** — `implementation_type = 'C_tulip'` — the same C
  library the core `tulip_rs` crate compares itself against in
  `tulip_rs/tulip_test/benches/`.
- **TA-Lib** — `implementation_type = 'talib'` — same as above.

Both are built from the exact same git submodules the core Rust criterion
benches use (`../../tulip_rs/tulip_test/{tulip_indicators,ta_lib_src}`), so
results are directly comparable across the Rust, Python, Node, Swift,
Diplomat-C, and this FFI-C suite in the same `indicator_benchmark` database.

This harness was ported from `tulip_rs_diplomat/bench/c` (kept here as a
self-contained copy; no files link to the diplomat directory).

## Layout

```
bench/c/
  bench.c                Benchmark harness: shared infra (DB fetch via libpq, timing,
                         result recording/printing, main()) plus one
                         #include "bench_indicators/<name>.c" per indicator (94 total,
                         alphabetical order), each measured against
                         tulip_rs_ffi_c, and (where available) C_tulip and talib,
                         across 4 stocks x 4 option sets
  bench_indicators/      One file per indicator (<name>.c), each providing a
                         <Name>Ctx struct, bench_<name>()/bench_tulipc_<name>()/
                         bench_talib_<name>() timed closures, and a run_<name>()
                         driver. NOT named "indicators/" -- that would collide with
                         the vendored Tulip Indicators C library's own indicators/
                         subfolder via -I search paths.
  Makefile
  run_bench.sh           Build cdylib + harness + run, end to end
  vendor/                Vendored reference libraries (gitignored, built automatically by `make`):
                         libtulipc.a (Tulip Indicators), ta-lib/ (extracted TA-Lib .deb),
                         libpq.so (Postgres client library via libpq)
```

## Setup

### 1. Build the `tulip_rs_ffi` cdylib

```bash
cd ../..                # tulip_rs_ffi/
cargo build --release
```

### 2. (Re)generate the C header (only needed after changing the `extern "C"` surface)

```bash
cbindgen --config cbindgen.toml --output include/tulip_rs_ffi.h
```

The harness compiles against `include/tulip_rs_ffi.h`; do not hand-edit it.

### 3. Build the harness

```bash
make
```

This also builds all vendored libraries automatically the first time:
`vendor/libtulipc.a` (compiled from `tulip_test/tulip_indicators/tiamalgamation.c`,
same flags as `tulip_test/build.rs`), `vendor/ta-lib/` (extracted from the
prebuilt `.deb` already vendored in the `tulip_test/ta_lib_src` submodule),
and `vendor/libpq.so` (copied from the system's libpq.so.5, used for direct
Postgres connectivity).

The harness fetches stock data directly from the `stocks` Postgres database at
runtime via libpq; no pre-fetched CSV files are needed.

## Running

Copy `.env.example` to `.env` and edit it (or export env vars inline), then
either run the binary directly or use `./run_bench.sh` for the full
build-and-run pipeline:

```bash
./tulip_rs_ffi_bench        # or: ./run_bench.sh
```

`.env` is discovered by walking up from the current working directory (like
`python-dotenv`'s `find_dotenv(usecwd=True)`), so this also works from the
repo root or any other subdirectory. Set `DOTENV_PATH=/path/to/file` to point
at a specific file instead.

You can still override anything inline, which takes precedence over both
`.env` and its own defaults:

```bash
BENCHMARK_LOG_TO_DB=1 \
BENCHMARK_DATABASE_URL=postgres://tulip:tulip@HOST:PORT/indicator_benchmark \
./tulip_rs_ffi_bench
```

## Environment variables

Set these in `bench/c/.env` (copy from `.env.example`) or export them inline —
same variable names as `tulip_rs_python/bench/.env` / `tulip_rs_node`:

| Variable | Default | Description |
|----------|---------|-------------|
| `BENCH_NUMBER` | `500` | Back-to-back calls per timing sample |
| `BENCH_REPEAT` | `10` | Number of independent timing samples |
| `BENCH_WARMUP` | `500` | Warm-up calls before timing starts |
| `BENCHMARK_LOG_TO_DB` | `0` | Set to `1` to write results to the DB |
| `DATABASE_URL` | `postgres://tulip:tulip@localhost:5432/stocks` | Used by `bench.c` directly at runtime for fetching stock OHLCV data from Postgres |
| `BENCHMARK_DATABASE_URL` | `postgres://tulip:tulip@localhost:5432/indicator_benchmark` | Result DB |
| `DOTENV_PATH` | (walks up from CWD for `.env`) | Override to point at a specific env file |

Results are logged under `implementation_type` = `tulip_rs_ffi_c`,
`C_tulip`, or `talib`.

## Reference implementations

For every stock/option-set combination, `bench.c` times three implementations
back-to-back in the same process:

| `implementation_type` | Library | Source |
|---|---|---|
| `tulip_rs_ffi_c` | `tulip_rs_ffi` (this crate) | `../../target/release/libtulip_rs_ffi.so` |
| `C_tulip` | [Tulip Indicators](https://tulipindicators.org/) | `tulip_test/tulip_indicators` submodule, compiled from `tiamalgamation.c` |
| `talib` | [TA-Lib](https://ta-lib.org/) | `tulip_test/ta_lib_src` submodule's vendored `dist/ta-lib_0.7.1_amd64.deb`, extracted (its current checkout has no buildable `src/ta_func` tree, so we reuse the prebuilt static archive instead of recompiling from source) |

Not every indicator has a genuine equivalent in both reference libraries --
where one doesn't exist, that comparison is simply omitted for that
indicator (never approximated with an unrelated function). A few
indicators need special handling:

- **`di`**: the core `tulip_rs` `di` indicator returns both +DI and -DI in
  one call, but TA-Lib exposes these as two separate functions
  (`TA_PLUS_DI`/`TA_MINUS_DI`). To keep the comparison apples-to-apples, the
  `talib` closure for `di` calls both back-to-back and times them together
  as a single `talib` sample.
- Indicators with **no TA-Lib equivalent** (grepped against `ta_func.h`, not
  assumed) or **no Tulip Indicators (C) equivalent** (e.g. `ef`, `trvi`,
  `vortex`) simply omit that closure.

## Methodology

Same as `tulip_rs_python`/`tulip_rs_node`/`tulip_rs_swift`:

| Item | Detail |
|------|--------|
| Data | Real OHLCV, 6,705 bars per stock (BHP/ASX, CBA/ASX, AAPL/NYSE, MSFT/NYSE), fetched live from Postgres via libpq at startup |
| Options | 4 option sets per indicator, identical to every other binding's bench suite |
| Timed region | One full call-and-destroy cycle (`<name>_indicator(...)` + `tulip_ffi_result_free(...)` + `<name>_state_free(...)`) — FFI result allocation/packing/teardown is included in the measurement |
| Samples | `BENCHMARK_REPEAT` independent samples, each averaging `BENCH_NUMBER` back-to-back calls |
| Reported time | Mean of samples; nanoseconds in the DB, microseconds on screen |
| Clock | `clock_gettime(CLOCK_MONOTONIC, ...)` |

## Adding a new indicator to this harness

1. Make sure `include/tulip_rs_ffi.h` exposes it (regenerate via cbindgen if
   the Rust side changed).
2. Create `bench_indicators/<name>.c` (NOT `indicators/<name>.c` -- that
   collides with the vendored Tulip Indicators C library's own `indicators/`
   subfolder via `-I` search paths and will silently include the wrong
   file). Follow the pattern in `bench_indicators/kama.c` (single-input),
   `macd.c` (multi-option), `stoch.c` (multi-input): a `<Name>Ctx` struct, a
   `bench_<name>()` timed closure calling `<name>_indicator()` with a
   `const double *inputs[INPUTS]` array (input order: see the module doc
   comment in `src/<name>.rs`, which mirrors the core `INFO.inputs`), then
   `tulip_ffi_result_free()` + `<name>_state_free()` inside the timed region;
   optionally `bench_tulipc_<name>()` / `bench_talib_<name>()` closures if a
   genuine equivalent exists in the corresponding reference library (check
   `tulip_test/src/c_bindings.rs` / grep the real uppercase `TA_<NAME>` API
   in `vendor/ta-lib/usr/include/ta-lib/ta_func.h` -- never assume no
   equivalent exists without grepping, and never call the private lowercase
   Rust wrapper functions in `tulip_test/src/talib_bindings.rs`, they aren't
   real C symbols), and a `run_<name>()` driver using the same 4 option sets
   as the Python/Node/Swift bench suites for that indicator (see
   `tulip_rs_python/bench/tulip_rs_bench/indicators/bench_<name>.py`).
   Use the `log_and_print()` helper (not `record_row`/`print_row` directly),
   with `implementation_type` string `"tulip_rs_ffi_c"`.
3. Add `#include "bench_indicators/<name>.c"` to `bench.c` in alphabetical
   order among the existing includes, and call `run_<name>(...)` from
   `main()` in the same alphabetical position.
4. Run `make` and fix any compile/link errors.
