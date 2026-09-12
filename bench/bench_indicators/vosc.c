// vosc (Volume Oscillator) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a single-input indicator with multiple options
// and optional_outputs requested.

#include "tulip_rs_ffi.h"
#include "../bench_common.h"

typedef struct {
    const Stock *stock;
    double short_period, long_period;
} VoscCtx;

// ---------------------------------------------------------------------------
// SIMD comparisons -- optional outputs always NULL (off).
// *_simd_by_assets() runs one option set across 4 assets in a single call;
// *_simd_by_options() runs 4 option sets on one asset in a single call.
// ---------------------------------------------------------------------------

typedef struct {
    const Stock *stocks;   // by_assets: array of 4 stocks; by_options: single stock
    size_t data_len;       // bars per asset (shared across the 4 for by_assets)
    const double *opts;    // by_assets: the single shared option set
    const double (*option_sets)[VOSC_OPTIONS]; // by_options: 4 option sets
} VoscSimdCtx;

static void bench_vosc_simd_assets(void *ctx_) {
    VoscSimdCtx *ctx = ctx_;
    // Each asset has VOSC_INPUTS=1 input pointer (volume)
    const double *inputs_per_asset[4][VOSC_INPUTS] = {
        {ctx->stocks[0].volume}, {ctx->stocks[1].volume},
        {ctx->stocks[2].volume}, {ctx->stocks[3].volume},
    };
    // inputs is an array of num_assets pointers to input arrays
    const double *inputs[4];
    for (int i = 0; i < 4; i++) {
        inputs[i] = (const double *)&inputs_per_asset[i][0];
    }
    struct CSimdResult r = vosc_simd_by_assets((const double *const *const *)inputs, 4, ctx->data_len, ctx->opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] vosc_simd_by_assets failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) vosc_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

static void bench_vosc_simd_options(void *ctx_) {
    VoscSimdCtx *ctx = ctx_;
    const double *inputs[VOSC_INPUTS] = {ctx->stocks->volume};
    const double *opts[4] = {ctx->option_sets[0], ctx->option_sets[1],
                             ctx->option_sets[2], ctx->option_sets[3]};
    struct CSimdResult r = vosc_simd_by_options(inputs, ctx->data_len, opts, 4, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] vosc_simd_by_options failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) vosc_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

static void bench_vosc(void *ctx_) {
    VoscCtx *ctx = ctx_;
    const double *inputs[VOSC_INPUTS] = {ctx->stock->volume};
    double opts[VOSC_OPTIONS] = {ctx->short_period, ctx->long_period};
    struct CIndicatorResult r = vosc_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] vosc_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    vosc_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_vosc.rs's bench_c_vosc exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_vosc(void *ctx_) {
    VoscCtx *ctx = ctx_;
    double options[VOSC_OPTIONS] = {ctx->short_period, ctx->long_period};
    int start_index = ti_vosc_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_vosc_start returned negative index\n"); exit(1); }
    int output_len = (int) ctx->stock->len - start_index;
    double *vosc_line = malloc(sizeof(double) * (size_t) output_len);
    double *short_sma = malloc(sizeof(double) * (size_t) output_len);
    double *long_sma = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[VOSC_INPUTS] = {ctx->stock->volume};
    double *outputs[3] = {vosc_line, short_sma, long_sma};
    int ret = ti_vosc((int) ctx->stock->len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_vosc returned %d\n", ret); exit(1); }
    free(vosc_line);
    free(short_sma);
    free(long_sma);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- not available. The vendored Tulip Indicators C library
// has no TA_Lib equivalent for vosc.
// ---------------------------------------------------------------------------

static void run_vosc(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][VOSC_OPTIONS] = {{5.0, 20.0}, {9.0, 26.0}, {12.0, 26.0}, {3.0, 10.0}};
    printf("\n--- VOSC ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            VoscCtx ctx = {
                .stock = &stocks[s],
                .short_period = option_sets[o][0],
                .long_period = option_sets[o][1],
            };

            TimingResult t = time_fn(bench_vosc, &ctx, number, repeat, warmup);
            log_and_print("vosc", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], VOSC_OPTIONS, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_vosc, &ctx, number, repeat, warmup);
            log_and_print("vosc", "C_tulip", stocks[s].symbol, option_sets[o], VOSC_OPTIONS, t_c, (int) stocks[s].len);

            // No TA-Lib comparison available
        }
    }

    // ---- SIMD runs (optional outputs off: NULL, 0) ----
    if (num_stocks >= 4) {
        size_t dlen = stocks[0].len;
        for (int i = 1; i < 4; i++) if (stocks[i].len < dlen) dlen = stocks[i].len;

        VoscSimdCtx sctx = { .stocks = stocks, .data_len = dlen };
        for (int o = 0; o < 4; o++) {
            sctx.opts = option_sets[o];
            TimingResult t_sa = time_fn(bench_vosc_simd_assets, &sctx, number, repeat, warmup);
            log_and_print("vosc", "tulip_rs_ffi_c_simd_by_assets", "All", option_sets[o], VOSC_OPTIONS, t_sa, (int) dlen);
        }

        for (int s = 0; s < num_stocks; s++) {
            VoscSimdCtx octx = { .stocks = &stocks[s], .data_len = stocks[s].len, .option_sets = option_sets };
            // One call times 4 option sets; first set logged as the representative key.
            TimingResult t_so = time_fn(bench_vosc_simd_options, &octx, number, repeat, warmup);
            log_and_print("vosc", "tulip_rs_ffi_c_simd_by_options", stocks[s].symbol, option_sets[0], VOSC_OPTIONS, t_so, (int) stocks[s].len);
        }
    }
}
