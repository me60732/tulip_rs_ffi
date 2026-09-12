// obv (On Balance Volume) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a two-input indicator with no options and
// no optional_outputs requested.

#include "tulip_rs_ffi.h"
#include "../bench_common.h"

typedef struct {
    const Stock *stock;
} ObvCtx;

static void bench_obv(void *ctx_) {
    ObvCtx *ctx = ctx_;
    const double *inputs[OBV_INPUTS] = {ctx->stock->close, ctx->stock->volume};
    struct CIndicatorResult r = obv_indicator(inputs, ctx->stock->len, NULL, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] obv_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    obv_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_obv.rs's bench_c_obv exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_obv(void *ctx_) {
    ObvCtx *ctx = ctx_;
    double options[0] = {};
    int start_index = ti_obv_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_obv_start returned negative index\n"); exit(1); }
    int output_len = (int) ctx->stock->len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[OBV_INPUTS] = {ctx->stock->close, ctx->stock->volume};
    double *outputs[1] = {output};
    int ret = ti_obv((int) ctx->stock->len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_obv returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib". Mirrors
// tulip_test/benches/benchmark_obv.rs's bench_talib_obv exactly.
// ---------------------------------------------------------------------------

static void bench_talib_obv(void *ctx_) {
    ObvCtx *ctx = ctx_;
    int size = (int) ctx->stock->len;
    int start_index = TA_OBV_Lookback();
    if (start_index < 0) { fprintf(stderr, "[error] TA_OBV_Lookback returned negative index\n"); exit(1); }
    int output_len = size - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    int out_begin = 0, out_nb_element = 0;
    TA_RetCode ret =
        TA_OBV(0, size - 1, ctx->stock->close, ctx->stock->volume,
               &out_begin, &out_nb_element, output);
    if (ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_OBV returned %d\n", (int) ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// SIMD comparisons -- optional outputs always NULL (off).
// obv_simd_by_assets() runs one option set across 4 assets in a single call.
// Note: OBV has no options (OBV_OPTIONS=0), so there is NO simd_by_options variant.
// ---------------------------------------------------------------------------

typedef struct {
    const Stock *stocks;   // array of 4 stocks
    size_t data_len;       // bars per asset (shared across the 4)
} ObvSimdCtx;

static void bench_obv_simd_assets(void *ctx_) {
    ObvSimdCtx *ctx = ctx_;
    // Each asset has OBV_INPUTS=2 input pointers (close, volume)
    const double *inputs_per_asset[4][OBV_INPUTS] = {
        {ctx->stocks[0].close, ctx->stocks[0].volume},
        {ctx->stocks[1].close, ctx->stocks[1].volume},
        {ctx->stocks[2].close, ctx->stocks[2].volume},
        {ctx->stocks[3].close, ctx->stocks[3].volume},
    };
    // inputs is an array of num_assets pointers to input arrays
    const double *inputs[4];
    for (int i = 0; i < 4; i++) {
        inputs[i] = (const double *)&inputs_per_asset[i][0];
    }
    // OBV has no options (OBV_OPTIONS=0); mirroring scalar call which passes NULL.
    struct CSimdResult r = obv_simd_by_assets((const double *const *const *)inputs, 4, ctx->data_len, NULL, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] obv_simd_by_assets failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) obv_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

static void run_obv(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][OBV_OPTIONS] = {{}};
    printf("\n--- OBV ---\n");
    for (int s = 0; s < num_stocks; s++) {
        ObvCtx ctx = {.stock = &stocks[s]};

        TimingResult t = time_fn(bench_obv, &ctx, number, repeat, warmup);
        log_and_print("obv", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[0], 0, t, (int) stocks[s].len);

        TimingResult t_c = time_fn(bench_tulipc_obv, &ctx, number, repeat, warmup);
        log_and_print("obv", "C_tulip", stocks[s].symbol, option_sets[0], 0, t_c, (int) stocks[s].len);

        TimingResult t_talib = time_fn(bench_talib_obv, &ctx, number, repeat, warmup);
        log_and_print("obv", "talib", stocks[s].symbol, option_sets[0], 0, t_talib, (int) stocks[s].len);
    }

    // ---- SIMD runs (optional outputs off: NULL, 0) ----
    if (num_stocks >= 4) {
        size_t dlen = stocks[0].len;
        for (int i = 1; i < 4; i++) if (stocks[i].len < dlen) dlen = stocks[i].len;

        ObvSimdCtx ctx = { .stocks = stocks, .data_len = dlen };
        TimingResult t = time_fn(bench_obv_simd_assets, &ctx, number, repeat, warmup);
        log_and_print("obv", "tulip_rs_ffi_c_simd_by_assets", "All", NULL, 0, t, (int) dlen);
    }
}
