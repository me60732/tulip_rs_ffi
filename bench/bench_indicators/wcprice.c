// wcprice (Weighted Close Price) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a multi-input indicator with no options and
// no optional_outputs. The flattened inputs buffer is built once per stock
// and reused across all option sets for that stock.

#include "tulip_rs_ffi.h"
#include "../bench_common.h"

typedef struct {
    const Stock *stock;
    const double *inputs_buf; // high ++ low ++ close, each stock->len long
} WcPriceCtx;

static void bench_wcprice(void *ctx_) {
    WcPriceCtx *ctx = ctx_;
    const double *inputs[WCPRICE_INPUTS] = {ctx->stock->high, ctx->stock->low, ctx->stock->close};
    // WCPRICE has 0 options
    double opts[0] = {};
    struct CIndicatorResult r = wcprice_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] wcprice_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    wcprice_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_wcprice.rs's bench_c_wcprice exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_wcprice(void *ctx_) {
    WcPriceCtx *ctx = ctx_;
    double options[0] = {};
    int start_index = ti_wcprice_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_wcprice_start returned negative index\n"); exit(1); }
    int output_len = (int) ctx->stock->len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[3] = {ctx->stock->high, ctx->stock->low, ctx->stock->close};
    double *outputs[1] = {output};
    int ret = ti_wcprice((int) ctx->stock->len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_wcprice returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// SIMD comparisons -- optional outputs always NULL (off).
// wcprice_simd_by_assets() runs one option set across 4 assets in a single call.
// Note: WCPRICE has no options (WCPRICE_OPTIONS=0), so there is NO simd_by_options variant.
// ---------------------------------------------------------------------------

typedef struct {
    const Stock *stocks;   // array of 4 stocks
    size_t data_len;       // bars per asset (shared across the 4)
} WcPriceSimdCtx;

static void bench_wcprice_simd_assets(void *ctx_) {
    WcPriceSimdCtx *ctx = ctx_;
    // Each asset has WCPRICE_INPUTS=3 input pointers (high, low, close)
    const double *inputs_per_asset[4][WCPRICE_INPUTS] = {
        {ctx->stocks[0].high, ctx->stocks[0].low, ctx->stocks[0].close},
        {ctx->stocks[1].high, ctx->stocks[1].low, ctx->stocks[1].close},
        {ctx->stocks[2].high, ctx->stocks[2].low, ctx->stocks[2].close},
        {ctx->stocks[3].high, ctx->stocks[3].low, ctx->stocks[3].close},
    };
    // inputs is an array of num_assets pointers to input arrays
    const double *inputs[4];
    for (int i = 0; i < 4; i++) {
        inputs[i] = (const double *)&inputs_per_asset[i][0];
    }
    // WCPRICE has no options (WCPRICE_OPTIONS=0); mirroring scalar call which passes NULL.
    struct CSimdResult r = wcprice_simd_by_assets((const double *const *const *)inputs, 4, ctx->data_len, NULL, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] wcprice_simd_by_assets failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) wcprice_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

static void run_wcprice(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][WCPRICE_OPTIONS] = {{}};
    printf("\n--- WCPRICE ---\n");
    for (int s = 0; s < num_stocks; s++) {
        size_t len = stocks[s].len;
        double *inputs_buf = malloc(sizeof(double) * len * 3);
        memcpy(inputs_buf, stocks[s].high, sizeof(double) * len);
        memcpy(inputs_buf + len, stocks[s].low, sizeof(double) * len);
        memcpy(inputs_buf + 2 * len, stocks[s].close, sizeof(double) * len);

        for (int o = 0; o < 4; o++) {
            WcPriceCtx ctx = {
                .stock = &stocks[s],
                .inputs_buf = inputs_buf,
            };

            TimingResult t = time_fn(bench_wcprice, &ctx, number, repeat, warmup);
            log_and_print("wcprice", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 0, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_wcprice, &ctx, number, repeat, warmup);
            log_and_print("wcprice", "C_tulip", stocks[s].symbol, option_sets[o], 0, t_c, (int) stocks[s].len);
        }

        free(inputs_buf);
    }

    // ---- SIMD runs (optional outputs off: NULL, 0) ----
    if (num_stocks >= 4) {
        size_t dlen = stocks[0].len;
        for (int i = 1; i < 4; i++) if (stocks[i].len < dlen) dlen = stocks[i].len;

        WcPriceSimdCtx ctx = { .stocks = stocks, .data_len = dlen };
        TimingResult t = time_fn(bench_wcprice_simd_assets, &ctx, number, repeat, warmup);
        log_and_print("wcprice", "tulip_rs_ffi_c_simd_by_assets", "All", NULL, 0, t, (int) dlen);
    }
}
