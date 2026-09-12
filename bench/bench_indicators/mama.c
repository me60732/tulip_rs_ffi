// mama (MESA Adaptive Moving Average) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating an indicator with a single input and options,
// with optional_outputs requested.

#include "tulip_rs_ffi.h"
#include "../bench_common.h"

typedef struct {
    const Stock *stock;
    double fast_limit, slow_limit;
} MamaCtx;

static void bench_mama(void *ctx_) {
    MamaCtx *ctx = ctx_;
    const double *inputs[MAMA_INPUTS] = {ctx->stock->close};
    double opts[MAMA_OPTIONS] = {ctx->fast_limit, ctx->slow_limit};
    struct CIndicatorResult r = mama_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] mama_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    mama_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip".
// NOTE: No ti_mama_start/ti_mama pair exists in tulip_test/src/c_bindings.rs.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib". Mirrors
// tulip_test/benches/benchmark_mama.rs's bench_talib_mama exactly (using real
// uppercase TA_MAMA/TA_MAMA_Lookback C API).
// ---------------------------------------------------------------------------

static void bench_talib_mama(void *ctx_) {
    MamaCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    int start_index = TA_MAMA_Lookback(ctx->fast_limit, ctx->slow_limit);
    if (start_index < 0) { fprintf(stderr, "[error] TA_MAMA_Lookback returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    double *mama = malloc(sizeof(double) * (size_t) output_len);
    double *fama = malloc(sizeof(double) * (size_t) output_len);
    int out_begin = 0, out_nb_element = 0;
    TA_RetCode ret = TA_MAMA(0, (int) len - 1,
                             ctx->stock->close,
                             ctx->fast_limit, ctx->slow_limit,
                             &out_begin, &out_nb_element, mama, fama);
    if (ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_MAMA returned %d\n", (int) ret); exit(1); }
    free(mama);
    free(fama);
}

// ---------------------------------------------------------------------------
// SIMD comparisons -- optional outputs always NULL (off).
// *_simd_by_assets() runs one option set across 4 assets in a single call;
// *_simd_by_options() runs 4 option sets on one asset in a single call.
// ---------------------------------------------------------------------------

typedef struct {
    const Stock *stocks;   // by_assets: array of 4 stocks; by_options: single stock
    size_t data_len;       // bars per asset (shared across the 4 for by_assets)
    const double *opts;    // by_assets: the single shared option set
    const double (*option_sets)[MAMA_OPTIONS]; // by_options: 4 option sets
} MamaSimdCtx;

static void bench_mama_simd_assets(void *ctx_) {
    MamaSimdCtx *ctx = ctx_;
    // Each asset has MAMA_INPUTS=1 input pointer (close price)
    const double *inputs_per_asset[4][MAMA_INPUTS] = {
        {ctx->stocks[0].close}, {ctx->stocks[1].close},
        {ctx->stocks[2].close}, {ctx->stocks[3].close},
    };
    // inputs is an array of num_assets pointers to input arrays
    const double *inputs[4];
    for (int i = 0; i < 4; i++) {
        inputs[i] = (const double *)&inputs_per_asset[i][0];
    }
    struct CSimdResult r = mama_simd_by_assets((const double *const *const *)inputs, 4, ctx->data_len, ctx->opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] mama_simd_by_assets failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) mama_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

static void bench_mama_simd_options(void *ctx_) {
    MamaSimdCtx *ctx = ctx_;
    const double *inputs[MAMA_INPUTS] = {ctx->stocks->close};
    const double *opts[4] = {ctx->option_sets[0], ctx->option_sets[1],
                             ctx->option_sets[2], ctx->option_sets[3]};
    struct CSimdResult r = mama_simd_by_options(inputs, ctx->data_len, opts, 4, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] mama_simd_by_options failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) mama_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

static void run_mama(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][MAMA_OPTIONS] = {{0.5, 0.05}, {0.4, 0.04}, {0.6, 0.06}, {0.7, 0.07}};
    printf("\n--- MAMA ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            MamaCtx ctx = {
                .stock = &stocks[s],
                .fast_limit = option_sets[o][0],
                .slow_limit = option_sets[o][1],
            };

            TimingResult t = time_fn(bench_mama, &ctx, number, repeat, warmup);
            log_and_print("mama", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], MAMA_OPTIONS, t, (int) stocks[s].len);

            // No C_tulip comparison available
            TimingResult t_talib = time_fn(bench_talib_mama, &ctx, number, repeat, warmup);
            log_and_print("mama", "talib", stocks[s].symbol, option_sets[o], MAMA_OPTIONS, t_talib, (int) stocks[s].len);
        }
    }

    // ---- SIMD runs (optional outputs off: NULL, 0) ----
    if (num_stocks >= 4) {
        size_t dlen = stocks[0].len;
        for (int i = 1; i < 4; i++) if (stocks[i].len < dlen) dlen = stocks[i].len;

        MamaSimdCtx sctx = { .stocks = stocks, .data_len = dlen };
        for (int o = 0; o < 4; o++) {
            sctx.opts = option_sets[o];
            TimingResult t_sa = time_fn(bench_mama_simd_assets, &sctx, number, repeat, warmup);
            log_and_print("mama", "tulip_rs_ffi_c_simd_by_assets", "All", option_sets[o], MAMA_OPTIONS, t_sa, (int) dlen);
        }

        for (int s = 0; s < num_stocks; s++) {
            MamaSimdCtx octx = { .stocks = &stocks[s], .data_len = stocks[s].len, .option_sets = option_sets };
            // One call times 4 option sets; first set logged as the representative key.
            TimingResult t_so = time_fn(bench_mama_simd_options, &octx, number, repeat, warmup);
            log_and_print("mama", "tulip_rs_ffi_c_simd_by_options", stocks[s].symbol, option_sets[0], MAMA_OPTIONS, t_so, (int) stocks[s].len);
        }
    }
}
