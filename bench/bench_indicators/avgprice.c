// avgprice (Average Price) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating an indicator with multiple input series but
// no options and no requested optional outputs. The inputs array is built inside
// the timed region since constructing it is what a real C caller's hot loop looks like.

#include "tulip_rs_ffi.h"
#include "../bench_common.h"

typedef struct {
    const Stock *stock;
} AvgPriceCtx;

static void bench_avgprice(void *ctx_) {
    AvgPriceCtx *ctx = ctx_;
    const double *inputs[AVGPRICE_INPUTS] = {ctx->stock->open, ctx->stock->high, ctx->stock->low, ctx->stock->close};
    struct CIndicatorResult r = avgprice_indicator(inputs, ctx->stock->len, NULL, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] avgprice_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    avgprice_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_avgprice.rs's bench_c_avgprice exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_avgprice(void *ctx_) {
    AvgPriceCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    int start_index = ti_avgprice_start(NULL);
    if (start_index < 0) { fprintf(stderr, "[error] ti_avgprice_start returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[AVGPRICE_INPUTS] = {ctx->stock->open, ctx->stock->high, ctx->stock->low, ctx->stock->close};
    double *outputs[1] = {output};
    int ret = ti_avgprice((int) len, inputs, NULL, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_avgprice returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib". Mirrors
// tulip_test/benches/benchmark_avgprice.rs's bench_talib_avgprice exactly.
// ---------------------------------------------------------------------------

static void bench_talib_avgprice(void *ctx_) {
    AvgPriceCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    int start_index = TA_AVGPRICE_Lookback();
    if (start_index < 0) { fprintf(stderr, "[error] TA_AVGPRICE_Lookback returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    int out_begin = 0, out_nb_element = 0;
    TA_RetCode ret = TA_AVGPRICE(0, (int) len - 1,
                                 ctx->stock->open, ctx->stock->high, ctx->stock->low, ctx->stock->close,
                                 &out_begin, &out_nb_element, output);
    if (ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_AVGPRICE returned %d\n", (int) ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// SIMD comparisons -- optional outputs always NULL (off).
// avgprice_simd_by_assets() runs one option set across 4 assets in a single call.
// Note: AVGPRICE has no options (AVGPRICE_OPTIONS=0), so there is NO simd_by_options variant.
// ---------------------------------------------------------------------------

typedef struct {
    const Stock *stocks;   // array of 4 stocks
    size_t data_len;       // bars per asset (shared across the 4)
} AvgPriceSimdCtx;

static void bench_avgprice_simd_assets(void *ctx_) {
    AvgPriceSimdCtx *ctx = ctx_;
    // Each asset has AVGPRICE_INPUTS=4 input pointers (open, high, low, close)
    const double *inputs_per_asset[4][AVGPRICE_INPUTS] = {
        {ctx->stocks[0].open, ctx->stocks[0].high, ctx->stocks[0].low, ctx->stocks[0].close},
        {ctx->stocks[1].open, ctx->stocks[1].high, ctx->stocks[1].low, ctx->stocks[1].close},
        {ctx->stocks[2].open, ctx->stocks[2].high, ctx->stocks[2].low, ctx->stocks[2].close},
        {ctx->stocks[3].open, ctx->stocks[3].high, ctx->stocks[3].low, ctx->stocks[3].close},
    };
    // inputs is an array of num_assets pointers to input arrays
    const double *inputs[4];
    for (int i = 0; i < 4; i++) {
        inputs[i] = (const double *)&inputs_per_asset[i][0];
    }
    // AVGPRICE has no options (AVGPRICE_OPTIONS=0); mirroring scalar call which passes NULL.
    struct CSimdResult r = avgprice_simd_by_assets((const double *const *const *)inputs, 4, ctx->data_len, NULL, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] avgprice_simd_by_assets failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) avgprice_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

static void run_avgprice(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][AVGPRICE_OPTIONS] = {{}};
    printf("\n--- AVGPRICE ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            AvgPriceCtx ctx = {.stock = &stocks[s]};

            TimingResult t = time_fn(bench_avgprice, &ctx, number, repeat, warmup);
            log_and_print("avgprice", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 0, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_avgprice, &ctx, number, repeat, warmup);
            log_and_print("avgprice", "C_tulip", stocks[s].symbol, option_sets[o], 0, t_c, (int) stocks[s].len);

            TimingResult t_talib = time_fn(bench_talib_avgprice, &ctx, number, repeat, warmup);
            log_and_print("avgprice", "talib", stocks[s].symbol, option_sets[o], 0, t_talib, (int) stocks[s].len);
        }
    }

    // ---- SIMD runs (optional outputs off: NULL, 0) ----
    if (num_stocks >= 4) {
        size_t dlen = stocks[0].len;
        for (int i = 1; i < 4; i++) if (stocks[i].len < dlen) dlen = stocks[i].len;

        AvgPriceSimdCtx ctx = { .stocks = stocks, .data_len = dlen };
        TimingResult t = time_fn(bench_avgprice_simd_assets, &ctx, number, repeat, warmup);
        log_and_print("avgprice", "tulip_rs_ffi_c_simd_by_assets", "All", NULL, 0, t, (int) dlen);
    }
}
