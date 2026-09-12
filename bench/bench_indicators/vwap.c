// vwap (Volume-Weighted Average Price) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a multi-input indicator with no options and
// optional_outputs requested.

#include "tulip_rs_ffi.h"
#include "../bench_common.h"

typedef struct {
    const Stock *stock;
} VwapCtx;

static void bench_vwap(void *ctx_) {
    VwapCtx *ctx = ctx_;
    const double *inputs[VWAP_INPUTS] = {ctx->stock->high, ctx->stock->low, ctx->stock->close, ctx->stock->volume};
    // VWAP has no options (VWAP_OPTIONS=0)
    struct CIndicatorResult r = vwap_indicator(inputs, ctx->stock->len, NULL, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] vwap_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    vwap_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- not available. The vendored Tulip
// Indicators C library has no ti_vwap function, so this comparison is omitted.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// SIMD comparisons -- optional outputs always NULL (off).
// vwap_simd_by_assets() runs one option set across 4 assets in a single call.
// Note: VWAP has no options (VWAP_OPTIONS=0), so there is NO simd_by_options variant.
// ---------------------------------------------------------------------------

typedef struct {
    const Stock *stocks;   // array of 4 stocks
    size_t data_len;       // bars per asset (shared across the 4)
} VwapSimdCtx;

static void bench_vwap_simd_assets(void *ctx_) {
    VwapSimdCtx *ctx = ctx_;
    // Each asset has VWAP_INPUTS=4 input pointers (high, low, close, volume)
    const double *inputs_per_asset[4][VWAP_INPUTS] = {
        {ctx->stocks[0].high, ctx->stocks[0].low, ctx->stocks[0].close, ctx->stocks[0].volume},
        {ctx->stocks[1].high, ctx->stocks[1].low, ctx->stocks[1].close, ctx->stocks[1].volume},
        {ctx->stocks[2].high, ctx->stocks[2].low, ctx->stocks[2].close, ctx->stocks[2].volume},
        {ctx->stocks[3].high, ctx->stocks[3].low, ctx->stocks[3].close, ctx->stocks[3].volume},
    };
    // inputs is an array of num_assets pointers to input arrays
    const double *inputs[4];
    for (int i = 0; i < 4; i++) {
        inputs[i] = (const double *)&inputs_per_asset[i][0];
    }
    // VWAP has no options (VWAP_OPTIONS=0); mirroring scalar call which passes NULL.
    struct CSimdResult r = vwap_simd_by_assets((const double *const *const *)inputs, 4, ctx->data_len, NULL, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] vwap_simd_by_assets failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) vwap_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

static void run_vwap(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][VWAP_OPTIONS] = {{}};
    printf("\n--- VWAP ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            VwapCtx ctx = {.stock = &stocks[s]};

            TimingResult t = time_fn(bench_vwap, &ctx, number, repeat, warmup);
            log_and_print("vwap", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 0, t, (int) stocks[s].len);

            // No Tulip C comparison available
        }
    }

    // ---- SIMD runs (optional outputs off: NULL, 0) ----
    if (num_stocks >= 4) {
        size_t dlen = stocks[0].len;
        for (int i = 1; i < 4; i++) if (stocks[i].len < dlen) dlen = stocks[i].len;

        VwapSimdCtx ctx = { .stocks = stocks, .data_len = dlen };
        TimingResult t = time_fn(bench_vwap_simd_assets, &ctx, number, repeat, warmup);
        log_and_print("vwap", "tulip_rs_ffi_c_simd_by_assets", "All", NULL, 0, t, (int) dlen);
    }
}
