// medprice (Median Price) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a multi-input indicator with no options.

#include "tulip_rs_ffi.h"
#include "../bench_common.h"

// ---------------------------------------------------------------------------
// SIMD comparisons -- optional outputs always NULL (off).
// medprice_simd_by_assets() runs one option set across 4 assets in a single call.
// Note: MEDPRICE has no options (MEDPRICE_OPTIONS=0), so there is NO simd_by_options variant.
// ---------------------------------------------------------------------------

typedef struct {
    const Stock *stocks;   // array of 4 stocks
    size_t data_len;       // bars per asset (shared across the 4)
} MedpriceSimdCtx;

static void bench_medprice_simd_assets(void *ctx_) {
    MedpriceSimdCtx *ctx = ctx_;
    // Each asset has MEDPRICE_INPUTS=2 input pointers (high, low)
    const double *inputs_per_asset[4][MEDPRICE_INPUTS] = {
        {ctx->stocks[0].high, ctx->stocks[0].low},
        {ctx->stocks[1].high, ctx->stocks[1].low},
        {ctx->stocks[2].high, ctx->stocks[2].low},
        {ctx->stocks[3].high, ctx->stocks[3].low},
    };
    // inputs is an array of num_assets pointers to input arrays
    const double *inputs[4];
    for (int i = 0; i < 4; i++) {
        inputs[i] = (const double *)&inputs_per_asset[i][0];
    }
    // MEDPRICE has no options (MEDPRICE_OPTIONS=0); mirroring scalar call which passes NULL.
    struct CSimdResult r = medprice_simd_by_assets((const double *const *const *)inputs, 4, ctx->data_len, NULL, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] medprice_simd_by_assets failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) medprice_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

typedef struct {
    const Stock *stock;
    const double *inputs_buf; // high ++ low, each stock->len long
} MedpriceCtx;

static void bench_medprice(void *ctx_) {
    MedpriceCtx *ctx = ctx_;
    const double *inputs[MEDPRICE_INPUTS] = {ctx->stock->high, ctx->stock->low};
    struct CIndicatorResult r = medprice_indicator(inputs, ctx->stock->len, NULL, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] medprice_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    medprice_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_medprice.rs's bench_c_medprice exactly.
// Note: medprice has no options, so ti_medprice_start is called with null pointer.
// ---------------------------------------------------------------------------

static void bench_tulipc_medprice(void *ctx_) {
    MedpriceCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    double options[0] = {};
    int start_index = ti_medprice_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_medprice_start returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[2] = {ctx->stock->high, ctx->stock->low};
    double *outputs[1] = {output};
    int ret = ti_medprice((int) len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_medprice returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib". Mirrors C_tulip pattern.
// TA_MEDPRICE_Lookback() returns void (no parameters), uses high/low arrays.
// ---------------------------------------------------------------------------

static void bench_talib_medprice(void *ctx_) {
    MedpriceCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    int start_index = TA_MEDPRICE_Lookback();
    if (start_index < 0) { fprintf(stderr, "[error] TA_MEDPRICE_Lookback returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    int out_begin = 0, out_nb_element = 0;
    TA_RetCode ret = TA_MEDPRICE(0, (int) len - 1,
                                 ctx->stock->high, ctx->stock->low,
                                 &out_begin, &out_nb_element, output);
    if (ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_MEDPRICE returned %d\n", (int) ret); exit(1); }
    free(output);
}

static void run_medprice(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][MEDPRICE_OPTIONS] = {{}};
    printf("\n--- MEDPRICE ---\n");
    for (int s = 0; s < num_stocks; s++) {
        size_t len = stocks[s].len;
        double *inputs_buf = malloc(sizeof(double) * len * 2);
        memcpy(inputs_buf, stocks[s].high, sizeof(double) * len);
        memcpy(inputs_buf + len, stocks[s].low, sizeof(double) * len);

        for (int o = 0; o < 1; o++) {
            MedpriceCtx ctx = {
                .stock = &stocks[s],
                .inputs_buf = inputs_buf,
            };

            TimingResult t = time_fn(bench_medprice, &ctx, number, repeat, warmup);
            log_and_print("medprice", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 0, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_medprice, &ctx, number, repeat, warmup);
            log_and_print("medprice", "C_tulip", stocks[s].symbol, option_sets[o], 0, t_c, (int) stocks[s].len);

            TimingResult t_talib = time_fn(bench_talib_medprice, &ctx, number, repeat, warmup);
            log_and_print("medprice", "talib", stocks[s].symbol, option_sets[o], 0, t_talib, (int) stocks[s].len);
        }

        free(inputs_buf);
    }

    // ---- SIMD runs (optional outputs off: NULL, 0) ----
    if (num_stocks >= 4) {
        size_t dlen = stocks[0].len;
        for (int i = 1; i < 4; i++) if (stocks[i].len < dlen) dlen = stocks[i].len;

        MedpriceSimdCtx ctx = { .stocks = stocks, .data_len = dlen };
        TimingResult t = time_fn(bench_medprice_simd_assets, &ctx, number, repeat, warmup);
        log_and_print("medprice", "tulip_rs_ffi_c_simd_by_assets", "All", NULL, 0, t, (int) dlen);
    }
}
