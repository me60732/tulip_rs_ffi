// supertrend (Super Trend) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a multi-input indicator with options and
// optional outputs requested.

#include "tulip_rs_ffi.h"
#include "../bench_common.h"

typedef struct {
    const Stock *stock;
    double period, step;
} SuperTrendCtx;

// ---------------------------------------------------------------------------
// SIMD comparisons -- optional outputs always NULL (off).
// *_simd_by_assets() runs one option set across 4 assets in a single call;
// *_simd_by_options() runs 4 option sets on one asset in a single call.
// ---------------------------------------------------------------------------

typedef struct {
    const Stock *stocks;   // by_assets: array of 4 stocks; by_options: single stock
    size_t data_len;       // bars per asset (shared across the 4 for by_assets)
    const double *opts;    // by_assets: the single shared option set
    const double (*option_sets)[SUPERTREND_OPTIONS]; // by_options: 4 option sets
} SuperTrendSimdCtx;

static void bench_supertrend(void *ctx_) {
    SuperTrendCtx *ctx = ctx_;
    const double *inputs[SUPERTREND_INPUTS] = {ctx->stock->high, ctx->stock->low, ctx->stock->close};
    double opts[SUPERTREND_OPTIONS] = {ctx->period, ctx->step};
    struct CIndicatorResult r = supertrend_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] supertrend_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    supertrend_state_free(r.state);
}

// ---------------------------------------------------------------------------
// No C_tulip comparison available -- supertrend has no ti_ equivalent
// in the tulip-c library.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// No TA-Lib comparison available -- while some vendor libraries have
// "supertrend" implementations, the vendor/ta-lib distribution does not
// include this indicator. The tulip-rs implementation follows a specific
// custom algorithm based on ATR and price trend detection.
// ---------------------------------------------------------------------------

static void bench_supertrend_simd_assets(void *ctx_) {
    SuperTrendSimdCtx *ctx = ctx_;
    // Each asset has SUPERTREND_INPUTS=3 input pointers (high, low, close)
    const double *inputs_per_asset[4][SUPERTREND_INPUTS] = {
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
    struct CSimdResult r = supertrend_simd_by_assets((const double *const *const *)inputs, 4, ctx->data_len, ctx->opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] supertrend_simd_by_assets failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) supertrend_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

static void bench_supertrend_simd_options(void *ctx_) {
    SuperTrendSimdCtx *ctx = ctx_;
    const double *inputs[SUPERTREND_INPUTS] = {ctx->stocks->high, ctx->stocks->low, ctx->stocks->close};
    const double *opts[4] = {ctx->option_sets[0], ctx->option_sets[1],
                             ctx->option_sets[2], ctx->option_sets[3]};
    struct CSimdResult r = supertrend_simd_by_options(inputs, ctx->data_len, opts, 4, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] supertrend_simd_by_options failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) supertrend_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

static void run_supertrend(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][SUPERTREND_OPTIONS] = {{7.0, 3.0}, {5.0, 2.0}, {10.0, 2.5}, {14.0, 2.0}};
    printf("\n--- SUPERTREND ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            SuperTrendCtx ctx = {
                .stock = &stocks[s],
                .period = option_sets[o][0],
                .step = option_sets[o][1],
            };

            TimingResult t = time_fn(bench_supertrend, &ctx, number, repeat, warmup);
            log_and_print("supertrend", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], SUPERTREND_OPTIONS, t, (int) stocks[s].len);
        }
    }

    // ---- SIMD runs (optional outputs off: NULL, 0) ----
    if (num_stocks >= 4) {
        size_t dlen = stocks[0].len;
        for (int i = 1; i < 4; i++) if (stocks[i].len < dlen) dlen = stocks[i].len;

        SuperTrendSimdCtx sctx = { .stocks = stocks, .data_len = dlen };
        for (int o = 0; o < 4; o++) {
            sctx.opts = option_sets[o];
            TimingResult t_sa = time_fn(bench_supertrend_simd_assets, &sctx, number, repeat, warmup);
            log_and_print("supertrend", "tulip_rs_ffi_c_simd_by_assets", "All", option_sets[o], SUPERTREND_OPTIONS, t_sa, (int) dlen);
        }

        for (int s = 0; s < num_stocks; s++) {
            SuperTrendSimdCtx octx = { .stocks = &stocks[s], .data_len = stocks[s].len, .option_sets = option_sets };
            // One call times 4 option sets; first set logged as the representative key.
            TimingResult t_so = time_fn(bench_supertrend_simd_options, &octx, number, repeat, warmup);
            log_and_print("supertrend", "tulip_rs_ffi_c_simd_by_options", stocks[s].symbol, option_sets[0], SUPERTREND_OPTIONS, t_so, (int) stocks[s].len);
        }
    }
}
