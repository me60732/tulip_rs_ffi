// macd (Moving Average Convergence/Divergence) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating an indicator with a single input but
// multiple options and no requested optional outputs (optional_outputs is
// passed as NULL + 0).

#include "tulip_rs_ffi.h"
#include "../bench_common.h"

typedef struct {
    const Stock *stock;
    double short_period, long_period, signal_period;
} MacdCtx;

static void bench_macd(void *ctx_) {
    MacdCtx *ctx = ctx_;
    const double *inputs[MACD_INPUTS] = {ctx->stock->close};
    double opts[MACD_OPTIONS] = {ctx->short_period, ctx->long_period, ctx->signal_period};
    struct CIndicatorResult r = macd_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] macd_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    macd_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_macd.rs's bench_c_macd exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_macd(void *ctx_) {
    MacdCtx *ctx = ctx_;
    double options[MACD_OPTIONS] = {ctx->short_period, ctx->long_period, ctx->signal_period};
    int start_index = ti_macd_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_macd_start returned negative index\n"); exit(1); }
    int output_len = (int) ctx->stock->len - start_index;
    double *macd = malloc(sizeof(double) * (size_t) output_len);
    double *signal = malloc(sizeof(double) * (size_t) output_len);
    double *hist = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[MACD_INPUTS] = {ctx->stock->close};
    double *outputs[3] = {macd, signal, hist};
    int ret = ti_macd((int) ctx->stock->len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_macd returned %d\n", ret); exit(1); }
    free(macd);
    free(signal);
    free(hist);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib". Mirrors
// tulip_test/benches/benchmark_macd.rs's bench_talib_macd exactly.
// ---------------------------------------------------------------------------

static void bench_talib_macd(void *ctx_) {
    MacdCtx *ctx = ctx_;
    int size = (int) ctx->stock->len;
    int start_index =
        TA_MACD_Lookback((int) ctx->short_period, (int) ctx->long_period, (int) ctx->signal_period);
    if (start_index < 0) { fprintf(stderr, "[error] TA_MACD_Lookback returned negative index\n"); exit(1); }
    int output_len = size - start_index;
    double *macd = malloc(sizeof(double) * (size_t) output_len);
    double *signal = malloc(sizeof(double) * (size_t) output_len);
    double *hist = malloc(sizeof(double) * (size_t) output_len);
    int out_begin = 0, out_nb_element = 0;
    TA_RetCode ret = TA_MACD(0, size - 1, ctx->stock->close, (int) ctx->short_period, (int) ctx->long_period,
                             (int) ctx->signal_period, &out_begin, &out_nb_element, macd, signal, hist);
    if (ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_MACD returned %d\n", (int) ret); exit(1); }
    free(macd);
    free(signal);
    free(hist);
}

// ---------------------------------------------------------------------------
// SIMD comparisons -- optional outputs always NULL (off).
// *_simd_by_assets() runs one option set across 4 assets in a single call;
// *_simd_by_options() runs 4 option sets on one asset in a single call.
// ---------------------------------------------------------------------------

typedef struct {
    const Stock *stocks;   // by_assets: array of 4 stocks; by_options: single stock
    size_t data_len;       // bars per asset (shared across the 4 for by_assets)
    const double *opts;    // by_assets: the single shared option set (points to row in option_sets)
    const double (*option_sets)[MACD_OPTIONS]; // by_options: 4 option sets
} MacdSimdCtx;

static void bench_macd_simd_assets(void *ctx_) {
    MacdSimdCtx *ctx = ctx_;
    // Each asset has MACD_INPUTS=1 input pointer (close price)
    const double *inputs_per_asset[4][MACD_INPUTS] = {
        {ctx->stocks[0].close}, {ctx->stocks[1].close},
        {ctx->stocks[2].close}, {ctx->stocks[3].close},
    };
    // inputs is an array of num_assets pointers to input arrays
    const double *inputs[4];
    for (int i = 0; i < 4; i++) {
        inputs[i] = (const double *)&inputs_per_asset[i][0];
    }
    struct CSimdResult r = macd_simd_by_assets((const double *const *const *)inputs, 4, ctx->data_len, ctx->opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] macd_simd_by_assets failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) macd_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

static void bench_macd_simd_options(void *ctx_) {
    MacdSimdCtx *ctx = ctx_;
    const double *inputs[MACD_INPUTS] = {ctx->stocks->close};
    const double *opts[4] = {ctx->option_sets[0], ctx->option_sets[1],
                             ctx->option_sets[2], ctx->option_sets[3]};
    struct CSimdResult r = macd_simd_by_options(inputs, ctx->data_len, opts, 4, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] macd_simd_by_options failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) macd_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

static void run_macd(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][MACD_OPTIONS] = {
        {5.0, 13.0, 8.0}, {19.0, 39.0, 9.0}, {10.0, 30.0, 10.0}, {6.0, 20.0, 9.0}};
    printf("\n--- MACD ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            MacdCtx ctx = {
                .stock = &stocks[s],
                .short_period = option_sets[o][0],
                .long_period = option_sets[o][1],
                .signal_period = option_sets[o][2],
            };

            TimingResult t = time_fn(bench_macd, &ctx, number, repeat, warmup);
            log_and_print("macd", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 3, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_macd, &ctx, number, repeat, warmup);
            log_and_print("macd", "C_tulip", stocks[s].symbol, option_sets[o], 3, t_c, (int) stocks[s].len);

            TimingResult t_talib = time_fn(bench_talib_macd, &ctx, number, repeat, warmup);
            log_and_print("macd", "talib", stocks[s].symbol, option_sets[o], 3, t_talib, (int) stocks[s].len);
        }
    }

    // ---- SIMD runs (optional outputs off: NULL, 0) ----
    if (num_stocks >= 4) {
        size_t dlen = stocks[0].len;
        for (int i = 1; i < 4; i++) if (stocks[i].len < dlen) dlen = stocks[i].len;

        MacdSimdCtx sctx = { .stocks = stocks, .data_len = dlen };
        for (int o = 0; o < 4; o++) {
            sctx.opts = option_sets[o];
            TimingResult t_sa = time_fn(bench_macd_simd_assets, &sctx, number, repeat, warmup);
            log_and_print("macd", "tulip_rs_ffi_c_simd_by_assets", "All", option_sets[o], MACD_OPTIONS, t_sa, (int) dlen);
        }

        for (int s = 0; s < num_stocks; s++) {
            MacdSimdCtx octx = { .stocks = &stocks[s], .data_len = stocks[s].len, .option_sets = option_sets };
            // One call times 4 option sets; first set logged as the representative key.
            TimingResult t_so = time_fn(bench_macd_simd_options, &octx, number, repeat, warmup);
            log_and_print("macd", "tulip_rs_ffi_c_simd_by_options", stocks[s].symbol, option_sets[0], MACD_OPTIONS, t_so, (int) stocks[s].len);
        }
    }
}
