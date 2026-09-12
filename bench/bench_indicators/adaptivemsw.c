// adaptivemsw (Adaptive Mesa Sine Wave) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating an indicator with a single input but no
// options, with optional_outputs requested (passing bool array of true).

#include "tulip_rs_ffi.h"
#include "../bench_common.h"

#define ADAPTIVEMSW_INPUTS 1
#define ADAPTIVEMSW_OPTIONS 0

typedef struct {
    const Stock *stock;
} AdaptiveMSWCtx;

static void bench_adaptivemsw(void *ctx_) {
    AdaptiveMSWCtx *ctx = ctx_;
    const double *inputs[ADAPTIVEMSW_INPUTS] = {ctx->stock->close};
    struct CIndicatorResult r = adaptivemsw_indicator(inputs, ctx->stock->len, NULL, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] adaptivemsw_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    adaptivemsw_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip".
// NOTE: No ti_adaptivemsw_start/ti_adaptivemsw pair exists in tulip_test/src/c_bindings.rs.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib".
// NOTE: TA-Lib does not provide an equivalent for adaptivemsw.
// ---------------------------------------------------------------------------

// SIMD comparisons -- optional outputs always NULL (off).
// adaptivemsw_simd_by_assets() runs one option set across 4 assets in a single call.
// Note: ADAPTIVEMSW has no options (ADAPTIVEMSW_OPTIONS=0), so there is NO simd_by_options variant.
// ---------------------------------------------------------------------------

typedef struct {
    const Stock *stocks;   // array of 4 stocks
    size_t data_len;       // bars per asset (shared across the 4)
} AdaptiveMSWSimdCtx;

static void bench_adaptivemsw_simd_assets(void *ctx_) {
    AdaptiveMSWSimdCtx *ctx = ctx_;
    // Each asset has ADAPTIVEMSW_INPUTS=1 input pointer (close price)
    const double *inputs_per_asset[4][ADAPTIVEMSW_INPUTS] = {
        {ctx->stocks[0].close}, {ctx->stocks[1].close},
        {ctx->stocks[2].close}, {ctx->stocks[3].close},
    };
    // inputs is an array of num_assets pointers to input arrays
    const double *inputs[4];
    for (int i = 0; i < 4; i++) {
        inputs[i] = (const double *)&inputs_per_asset[i][0];
    }
    // ADAPTIVEMSW has no options (ADAPTIVEMSW_OPTIONS=0); mirroring scalar call which passes NULL.
    struct CSimdResult r = adaptivemsw_simd_by_assets((const double *const *const *)inputs, 4, ctx->data_len, NULL, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] adaptivemsw_simd_by_assets failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) adaptivemsw_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

static void run_adaptivemsw(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][ADAPTIVEMSW_OPTIONS] = {{}};
    printf("\n--- ADAPTIVEMSW ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            AdaptiveMSWCtx ctx = {.stock = &stocks[s]};

            TimingResult t = time_fn(bench_adaptivemsw, &ctx, number, repeat, warmup);
            log_and_print("adaptivemsw", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 0, t, (int) stocks[s].len);

            // No C_tulip comparison available
            // No talib comparison available
        }
    }

    // ---- SIMD runs (optional outputs off: NULL, 0) ----
    if (num_stocks >= 4) {
        size_t dlen = stocks[0].len;
        for (int i = 1; i < 4; i++) if (stocks[i].len < dlen) dlen = stocks[i].len;

        AdaptiveMSWSimdCtx ctx = { .stocks = stocks, .data_len = dlen };
        TimingResult t = time_fn(bench_adaptivemsw_simd_assets, &ctx, number, repeat, warmup);
        log_and_print("adaptivemsw", "tulip_rs_ffi_c_simd_by_assets", "All", NULL, 0, t, (int) dlen);
    }
}
