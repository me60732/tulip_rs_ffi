// vidya (Variable Index Dynamic Average) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a single-input indicator with 3 options
// and optional outputs supported.

#include "tulip_rs_ffi.h"
#include "../bench_common.h"

typedef struct {
    const Stock *stock;
    double short_period, long_period, alpha;
} VidyaCtx;

// ---------------------------------------------------------------------------
// SIMD comparisons -- optional outputs always NULL (off).
// *_simd_by_assets() runs one option set across 4 assets in a single call;
// *_simd_by_options() runs 4 option sets on one asset in a single call.
// ---------------------------------------------------------------------------

typedef struct {
    const Stock *stocks;   // by_assets: array of 4 stocks; by_options: single stock
    size_t data_len;       // bars per asset (shared across the 4 for by_assets)
    const double *opts;    // by_assets: the single shared option set
    const double (*option_sets)[VIDYA_OPTIONS]; // by_options: 4 option sets
} VidyaSimdCtx;

static void bench_vidya_simd_assets(void *ctx_) {
    VidyaSimdCtx *ctx = ctx_;
    // Each asset has VIDYA_INPUTS=1 input pointer (close price)
    const double *inputs_per_asset[4][VIDYA_INPUTS] = {
        {ctx->stocks[0].close}, {ctx->stocks[1].close},
        {ctx->stocks[2].close}, {ctx->stocks[3].close},
    };
    // inputs is an array of num_assets pointers to input arrays
    const double *inputs[4];
    for (int i = 0; i < 4; i++) {
        inputs[i] = (const double *)&inputs_per_asset[i][0];
    }
    struct CSimdResult r = vidya_simd_by_assets((const double *const *const *)inputs, 4, ctx->data_len, ctx->opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] vidya_simd_by_assets failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) vidya_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

static void bench_vidya_simd_options(void *ctx_) {
    VidyaSimdCtx *ctx = ctx_;
    const double *inputs[VIDYA_INPUTS] = {ctx->stocks->close};
    const double *opts[4] = {ctx->option_sets[0], ctx->option_sets[1],
                             ctx->option_sets[2], ctx->option_sets[3]};
    struct CSimdResult r = vidya_simd_by_options(inputs, ctx->data_len, opts, 4, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] vidya_simd_by_options failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) vidya_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

static void bench_vidya(void *ctx_) {
    VidyaCtx *ctx = ctx_;
    const double *inputs[VIDYA_INPUTS] = {ctx->stock->close};
    double opts[VIDYA_OPTIONS] = {ctx->short_period, ctx->long_period, ctx->alpha};
    struct CIndicatorResult r = vidya_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] vidya_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    vidya_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_vidya.rs's bench_c_vidya exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_vidya(void *ctx_) {
    VidyaCtx *ctx = ctx_;
    double options[VIDYA_OPTIONS] = {ctx->short_period, ctx->long_period, ctx->alpha};
    int start_index = ti_vidya_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_vidya_start returned negative index\n"); exit(1); }
    int output_len = (int) ctx->stock->len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[VIDYA_INPUTS] = {ctx->stock->close};
    double *outputs[1] = {output};
    int ret = ti_vidya((int) ctx->stock->len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_vidya returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- not available. TA-Lib has no VIDYA (Variable Index
// Dynamic Average) function. TA_VAR (Variance) is a different, unrelated
// calculation, so it is not used as a stand-in here.
// ---------------------------------------------------------------------------

static void run_vidya(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][VIDYA_OPTIONS] = {
        {2.0, 5.0, 0.2}, {5.0, 20.0, 0.2}, {9.0, 30.0, 0.2}, {12.0, 26.0, 0.1}};
    printf("\n--- VIDYA ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            VidyaCtx ctx = {
                .stock = &stocks[s],
                .short_period = option_sets[o][0],
                .long_period = option_sets[o][1],
                .alpha = option_sets[o][2],
            };

            TimingResult t = time_fn(bench_vidya, &ctx, number, repeat, warmup);
            log_and_print("vidya", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], VIDYA_OPTIONS, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_vidya, &ctx, number, repeat, warmup);
            log_and_print("vidya", "C_tulip", stocks[s].symbol, option_sets[o], VIDYA_OPTIONS, t_c, (int) stocks[s].len);

            // No TA-Lib comparison available
        }
    }

    // ---- SIMD runs (optional outputs off: NULL, 0) ----
    if (num_stocks >= 4) {
        size_t dlen = stocks[0].len;
        for (int i = 1; i < 4; i++) if (stocks[i].len < dlen) dlen = stocks[i].len;

        VidyaSimdCtx sctx = { .stocks = stocks, .data_len = dlen };
        for (int o = 0; o < 4; o++) {
            sctx.opts = option_sets[o];
            TimingResult t_sa = time_fn(bench_vidya_simd_assets, &sctx, number, repeat, warmup);
            log_and_print("vidya", "tulip_rs_ffi_c_simd_by_assets", "All", option_sets[o], VIDYA_OPTIONS, t_sa, (int) dlen);
        }

        for (int s = 0; s < num_stocks; s++) {
            VidyaSimdCtx octx = { .stocks = &stocks[s], .data_len = stocks[s].len, .option_sets = option_sets };
            // One call times 4 option sets; first set logged as the representative key.
            TimingResult t_so = time_fn(bench_vidya_simd_options, &octx, number, repeat, warmup);
            log_and_print("vidya", "tulip_rs_ffi_c_simd_by_options", stocks[s].symbol, option_sets[0], VIDYA_OPTIONS, t_so, (int) stocks[s].len);
        }
    }
}
