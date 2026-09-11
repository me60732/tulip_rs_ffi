#!/usr/bin/env bash
# Build and run the tulip_rs_ffi C benchmark harness end-to-end:
#   1. Build the tulip_rs_ffi cdylib in release mode
#   2. Build the C harness (and its vendored reference libraries: Tulip
#      Indicators C, TA-Lib, libpq) via `make`
#   3. Run it
#
# Usage:
#   ./run_bench.sh                 # build + run with whatever .env/defaults apply
#   ./run_bench.sh --no-build      # skip steps 1-2, just run the existing binary
#   ./run_bench.sh --clean         # `make clean` first (forces full rebuild)
#
# Any environment variables recognized by bench.c (BENCH_NUMBER, BENCH_REPEAT,
# BENCH_WARMUP, BENCHMARK_LOG_TO_DB, DATABASE_URL, BENCHMARK_DATABASE_URL,
# DOTENV_PATH) can be exported before calling this script, or set in
# bench/c/.env -- see README.md.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
FFI_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"   # tulip_rs_ffi

DO_BUILD=1
DO_CLEAN=0

for arg in "$@"; do
    case "$arg" in
        --no-build) DO_BUILD=0 ;;
        --clean) DO_CLEAN=1 ;;
        -h|--help)
            sed -n '2,20p' "$0"
            exit 0
            ;;
        *)
            echo "[error] unknown argument: $arg" >&2
            exit 1
            ;;
    esac
done

if [ "$DO_BUILD" -eq 1 ]; then
    echo "==> Building tulip_rs_ffi cdylib (release)"
    (cd "$FFI_ROOT" && cargo build --release)

    if [ "$DO_CLEAN" -eq 1 ]; then
        echo "==> make clean"
        (cd "$SCRIPT_DIR" && make clean)
    fi

    echo "==> Building C benchmark harness (make)"
    (cd "$SCRIPT_DIR" && make)
else
    echo "==> Skipping build (--no-build)"
fi

BIN="$SCRIPT_DIR/tulip_rs_ffi_bench"
if [ ! -x "$BIN" ]; then
    echo "[error] benchmark binary not found at $BIN" >&2
    exit 1
fi

echo "==> Running benchmark harness"
(cd "$SCRIPT_DIR" && exec ./tulip_rs_ffi_bench)
