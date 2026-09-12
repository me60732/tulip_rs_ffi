// tr (True Range) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a multi-input indicator with no options.

#include "tulip_rs_ffi.h"
#include "../bench_common.h"

typedef struct {
    const Stock *stock;
} TrCtx;

static void bench_tr(void *ctx_) {
    TrCtx *ctx = ctx_;
    const double *inputs[TR_INPUTS] = {ctx->stock->high, ctx->stock->low, ctx->stock->close};
    // TR_OPTIONS == 0, omit opts array
    struct CIndicatorResult r = tr_indicator(inputs, ctx->stock->len, NULL, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] tr_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    tr_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_tr.rs's bench_c_tr exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_tr(void *ctx_) {
    TrCtx *ctx = ctx_;
    double options[0] = {};
    int start_index = ti_tr_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_tr_start returned negative index\n"); exit(1); }
    int output_len = (int) ctx->stock->len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[3] = {ctx->stock->high, ctx->stock->low, ctx->stock->close};
    double *outputs[1] = {output};
    int ret = ti_tr((int) ctx->stock->len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_tr returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib". Mirrors
// tulip_test/benches/benchmark_tr.rs's bench_talib_tr exactly.
// ---------------------------------------------------------------------------

static void bench_talib_tr(void *ctx_) {
    TrCtx *ctx = ctx_;
    int size = (int) ctx->stock->len;
    int start_index = TA_TRANGE_Lookback();
    if (start_index < 0) { fprintf(stderr, "[error] TA_TRANGE_Lookback returned negative index\n"); exit(1); }
    int output_len = size - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    int out_begin = 0, out_nb_element = 0;
    TA_RetCode ret =
        TA_TRANGE(0, size - 1,
                  ctx->stock->high, ctx->stock->low, ctx->stock->close,
                  &out_begin, &out_nb_element, output);
    if (ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_TRANGE returned %d\n", (int) ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// SIMD comparisons -- optional outputs always NULL (off).
// tr_simd_by_assets() runs one option set across 4 assets in a single call.
// Note: TR has no options (TR_OPTIONS=0), so there is NO simd_by_options variant.
// ---------------------------------------------------------------------------

typedef struct {
    const Stock *stocks;   // array of 4 stocks
    size_t data_len;       // bars per asset (shared across the 4)
} TrSimdCtx;

static void bench_tr_simd_assets(void *ctx_) {
    TrSimdCtx *ctx = ctx_;
    // Each asset has TR_INPUTS=3 input pointers (high, low, close)
    const double *inputs_per_asset[4][TR_INPUTS] = {
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
    // TR has no options (TR_OPTIONS=0); mirroring scalar call which passes NULL.
    struct CSimdResult r = tr_simd_by_assets((const double *const *const *)inputs, 4, ctx->data_len, NULL, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] tr_simd_by_assets failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) tr_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

static void run_tr(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][TR_OPTIONS] = {{}};
    printf("\n--- TR ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 1; o++) {
            TrCtx ctx = {.stock = &stocks[s]};

            TimingResult t = time_fn(bench_tr, &ctx, number, repeat, warmup);
            log_and_print("tr", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], TR_OPTIONS, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_tr, &ctx, number, repeat, warmup);
            log_and_print("tr", "C_tulip", stocks[s].symbol, option_sets[o], TR_OPTIONS, t_c, (int) stocks[s].len);

            TimingResult t_talib = time_fn(bench_talib_tr, &ctx, number, repeat, warmup);
            log_and_print("tr", "talib", stocks[s].symbol, option_sets[o], TR_OPTIONS, t_talib, (int) stocks[s].len);
        }
    }

    // ---- SIMD runs (optional outputs off: NULL, 0) ----
    if (num_stocks >= 4) {
        size_t dlen = stocks[0].len;
        for (int i = 1; i < 4; i++) if (stocks[i].len < dlen) dlen = stocks[i].len;

        TrSimdCtx ctx = { .stocks = stocks, .data_len = dlen };
        TimingResult t = time_fn(bench_tr_simd_assets, &ctx, number, repeat, warmup);
        log_and_print("tr", "tulip_rs_ffi_c_simd_by_assets", "All", NULL, 0, t, (int) dlen);
    }
}
