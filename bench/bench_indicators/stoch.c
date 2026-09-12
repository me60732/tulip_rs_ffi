// stoch (Stochastic Oscillator) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a *multi-input* indicator: the ffi
// API takes `inputs` as an array of INPUTS pointers (here: high, low,
// close -- matching tulip_rs::indicators::stoch::Stoch::INFO.inputs order),
// each stock->len f64s long. The pointer array is built inside the timed
// region since constructing it is what a real C caller's hot loop looks like.

#include "tulip_rs_ffi.h"
#include "../bench_common.h"

typedef struct {
    const Stock *stock;
    double k_period, k_slow, d_period;
} StochCtx;

// ---------------------------------------------------------------------------
// SIMD comparisons -- optional outputs always NULL (off).
// *_simd_by_assets() runs one option set across 4 assets in a single call;
// *_simd_by_options() runs 4 option sets on one asset in a single call.
// ---------------------------------------------------------------------------

typedef struct {
    const Stock *stocks;   // by_assets: array of 4 stocks; by_options: single stock
    size_t data_len;       // bars per asset (shared across the 4 for by_assets)
    const double *opts;    // by_assets: the single shared option set
    const double (*option_sets)[STOCH_OPTIONS]; // by_options: 4 option sets
} StochSimdCtx;

static void bench_stoch(void *ctx_) {
    StochCtx *ctx = ctx_;
    const double *inputs[STOCH_INPUTS] = {ctx->stock->high, ctx->stock->low, ctx->stock->close};
    double opts[STOCH_OPTIONS] = {ctx->k_period, ctx->k_slow, ctx->d_period};
    struct CIndicatorResult r = stoch_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] stoch_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    stoch_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_stoch.rs's bench_c_stoch exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_stoch(void *ctx_) {
    StochCtx *ctx = ctx_;
    double options[STOCH_OPTIONS] = {ctx->k_period, ctx->k_slow, ctx->d_period};
    int start_index = ti_stoch_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_stoch_start returned negative index\n"); exit(1); }
    int output_len = (int) ctx->stock->len - start_index;
    double *slowk = malloc(sizeof(double) * (size_t) output_len);
    double *slowd = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[STOCH_INPUTS] = {ctx->stock->high, ctx->stock->low, ctx->stock->close};
    double *outputs[2] = {slowk, slowd};
    int ret = ti_stoch((int) ctx->stock->len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_stoch returned %d\n", ret); exit(1); }
    free(slowk);
    free(slowd);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib". Mirrors
// tulip_test/benches/benchmark_stoch.rs's bench_talib_stoch exactly.
// ---------------------------------------------------------------------------

static void bench_talib_stoch(void *ctx_) {
    StochCtx *ctx = ctx_;
    int size = (int) ctx->stock->len;
    int start_index = TA_STOCH_Lookback((int) ctx->k_period, (int) ctx->k_slow, TA_MAType_SMA, (int) ctx->d_period,
                                        TA_MAType_SMA);
    if (start_index < 0) { fprintf(stderr, "[error] TA_STOCH_Lookback returned negative index\n"); exit(1); }
    int output_len = size - start_index;
    double *slowk = malloc(sizeof(double) * (size_t) output_len);
    double *slowd = malloc(sizeof(double) * (size_t) output_len);
    int out_begin = 0, out_nb_element = 0;
    TA_RetCode ret =
        TA_STOCH(0, size - 1, ctx->stock->high, ctx->stock->low, ctx->stock->close, (int) ctx->k_period,
                 (int) ctx->k_slow, TA_MAType_SMA, (int) ctx->d_period, TA_MAType_SMA, &out_begin, &out_nb_element,
                 slowk, slowd);
    if (ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_STOCH returned %d\n", (int) ret); exit(1); }
    free(slowk);
    free(slowd);
}

static void bench_stoch_simd_assets(void *ctx_) {
    StochSimdCtx *ctx = ctx_;
    // Each asset has STOCH_INPUTS=3 input pointers (high, low, close)
    const double *inputs_per_asset[4][STOCH_INPUTS] = {
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
    struct CSimdResult r = stoch_simd_by_assets((const double *const *const *)inputs, 4, ctx->data_len, ctx->opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] stoch_simd_by_assets failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) stoch_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

static void bench_stoch_simd_options(void *ctx_) {
    StochSimdCtx *ctx = ctx_;
    const double *inputs[STOCH_INPUTS] = {ctx->stocks->high, ctx->stocks->low, ctx->stocks->close};
    const double *opts[4] = {ctx->option_sets[0], ctx->option_sets[1],
                             ctx->option_sets[2], ctx->option_sets[3]};
    struct CSimdResult r = stoch_simd_by_options(inputs, ctx->data_len, opts, 4, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] stoch_simd_by_options failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) stoch_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

static void run_stoch(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][STOCH_OPTIONS] = {
        {28.0, 16.0, 12.0}, {35.0, 21.0, 14.0}, {50.0, 30.0, 21.0}, {100.0, 50.0, 30.0}};
    printf("\n--- STOCH ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            StochCtx ctx = {
                .stock = &stocks[s],
                .k_period = option_sets[o][0],
                .k_slow = option_sets[o][1],
                .d_period = option_sets[o][2],
            };

            TimingResult t = time_fn(bench_stoch, &ctx, number, repeat, warmup);
            log_and_print("stoch", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 3, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_stoch, &ctx, number, repeat, warmup);
            log_and_print("stoch", "C_tulip", stocks[s].symbol, option_sets[o], 3, t_c, (int) stocks[s].len);

            TimingResult t_talib = time_fn(bench_talib_stoch, &ctx, number, repeat, warmup);
            log_and_print("stoch", "talib", stocks[s].symbol, option_sets[o], 3, t_talib, (int) stocks[s].len);
        }
    }

    // ---- SIMD runs (optional outputs off: NULL, 0) ----
    if (num_stocks >= 4) {
        size_t dlen = stocks[0].len;
        for (int i = 1; i < 4; i++) if (stocks[i].len < dlen) dlen = stocks[i].len;

        StochSimdCtx sctx = { .stocks = stocks, .data_len = dlen };
        for (int o = 0; o < 4; o++) {
            sctx.opts = option_sets[o];
            TimingResult t_sa = time_fn(bench_stoch_simd_assets, &sctx, number, repeat, warmup);
            log_and_print("stoch", "tulip_rs_ffi_c_simd_by_assets", "All", option_sets[o], STOCH_OPTIONS, t_sa, (int) dlen);
        }

        for (int s = 0; s < num_stocks; s++) {
            StochSimdCtx octx = { .stocks = &stocks[s], .data_len = stocks[s].len, .option_sets = option_sets };
            // One call times 4 option sets; first set logged as the representative key.
            TimingResult t_so = time_fn(bench_stoch_simd_options, &octx, number, repeat, warmup);
            log_and_print("stoch", "tulip_rs_ffi_c_simd_by_options", stocks[s].symbol, option_sets[0], STOCH_OPTIONS, t_so, (int) stocks[s].len);
        }
    }
}
