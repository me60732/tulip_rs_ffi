// marketfi (Market Facilitation Index) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating an indicator with multiple input series but
// no options and no requested optional outputs. The inputs array is built
// inside the timed region since constructing it is what a real C caller's
// hot loop looks like.

#include "tulip_rs_ffi.h"
#include "../bench_common.h"

typedef struct {
    const Stock *stock;
} MarketfiCtx;

static void bench_marketfi(void *ctx_) {
    MarketfiCtx *ctx = ctx_;
    const double *inputs[MARKETFI_INPUTS] = {ctx->stock->high, ctx->stock->low, ctx->stock->volume};
    struct CIndicatorResult r = marketfi_indicator(inputs, ctx->stock->len, NULL, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] marketfi_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    marketfi_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_marketfi.rs's bench_c_marketfi exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_marketfi(void *ctx_) {
    MarketfiCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    double options[0] = {};
    int start_index = ti_marketfi_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_marketfi_start returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[MARKETFI_INPUTS] = {ctx->stock->high, ctx->stock->low, ctx->stock->volume};
    double *outputs[1] = {output};
    int ret = ti_marketfi((int) len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_marketfi returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// SIMD comparisons -- optional outputs always NULL (off).
// marketfi_simd_by_assets() runs one option set across 4 assets in a single call.
// Note: MARKETFI has no options (MARKETFI_OPTIONS=0), so there is NO simd_by_options variant.
// ---------------------------------------------------------------------------

typedef struct {
    const Stock *stocks;   // array of 4 stocks
    size_t data_len;       // bars per asset (shared across the 4)
} MarketfiSimdCtx;

static void bench_marketfi_simd_assets(void *ctx_) {
    MarketfiSimdCtx *ctx = ctx_;
    // Each asset has MARKETFI_INPUTS=3 input pointers (high, low, volume)
    const double *inputs_per_asset[4][MARKETFI_INPUTS] = {
        {ctx->stocks[0].high, ctx->stocks[0].low, ctx->stocks[0].volume},
        {ctx->stocks[1].high, ctx->stocks[1].low, ctx->stocks[1].volume},
        {ctx->stocks[2].high, ctx->stocks[2].low, ctx->stocks[2].volume},
        {ctx->stocks[3].high, ctx->stocks[3].low, ctx->stocks[3].volume},
    };
    // inputs is an array of num_assets pointers to input arrays
    const double *inputs[4];
    for (int i = 0; i < 4; i++) {
        inputs[i] = (const double *)&inputs_per_asset[i][0];
    }
    // MARKETFI has no options (MARKETFI_OPTIONS=0); mirroring scalar call which passes NULL.
    struct CSimdResult r = marketfi_simd_by_assets((const double *const *const *)inputs, 4, ctx->data_len, NULL, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] marketfi_simd_by_assets failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) marketfi_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

static void run_marketfi(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][MARKETFI_OPTIONS] = {{}};
    printf("\n--- MARKETFI ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            MarketfiCtx ctx = {.stock = &stocks[s]};

            TimingResult t = time_fn(bench_marketfi, &ctx, number, repeat, warmup);
            log_and_print("marketfi", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], MARKETFI_OPTIONS, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_marketfi, &ctx, number, repeat, warmup);
            log_and_print("marketfi", "C_tulip", stocks[s].symbol, option_sets[o], MARKETFI_OPTIONS, t_c, (int) stocks[s].len);

            // No talib comparison available
        }
    }

    // ---- SIMD runs (optional outputs off: NULL, 0) ----
    if (num_stocks >= 4) {
        size_t dlen = stocks[0].len;
        for (int i = 1; i < 4; i++) if (stocks[i].len < dlen) dlen = stocks[i].len;

        MarketfiSimdCtx ctx = { .stocks = stocks, .data_len = dlen };
        TimingResult t = time_fn(bench_marketfi_simd_assets, &ctx, number, repeat, warmup);
        log_and_print("marketfi", "tulip_rs_ffi_c_simd_by_assets", "All", NULL, 0, t, (int) dlen);
    }
}
