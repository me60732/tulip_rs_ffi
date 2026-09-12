// trendmode (Trend Mode) -- tulip_rs_ffi extern "C" API.
//
// Single-input indicator with one option.

#include "tulip_rs_ffi.h"
#include "../bench_common.h"

typedef struct {
    const Stock *stock;
    double alpha;
} TrendModeCtx;

static void bench_trendmode(void *ctx_) {
    TrendModeCtx *ctx = ctx_;
    const double *inputs[TRENDMODE_INPUTS] = {ctx->stock->close};
    double opts[TRENDMODE_OPTIONS] = {ctx->alpha};
    struct CIndicatorResult r = trendmode_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] trendmode_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    trendmode_state_free(r.state);
}

// ---------------------------------------------------------------------------
// No C_tulip comparison available -- trendmode has no ti_ equivalent
// in the tulip-c library.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// No TA-Lib comparison available -- while TA_HT_TRENDMODE exists, it is
// the Hilbert Transform Trend Mode (a different algorithm), not the same
// as tulip-rs's TrendMode indicator. This is a custom Ehlers-based trend
// detection algorithm.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// SIMD comparisons -- optional outputs always NULL (off).
// trendmode_simd_by_assets() runs one option set across 4 assets in a single call;
// trendmode_simd_by_options() runs 4 option sets on one asset in a single call.
// Note: TRENDMODE has OPTIONS=2 (alpha and a second option).
// ---------------------------------------------------------------------------

typedef struct {
    const Stock *stocks;   // by_assets: array of 4 stocks; by_options: single stock
    size_t data_len;       // bars per asset (shared across the 4 for by_assets)
    const double *opts;    // by_assets: the single shared option set (points to opts[0])
    const double (*option_sets)[TRENDMODE_OPTIONS]; // by_options: 4 option sets
} TrendModeSimdCtx;

static void bench_trendmode_simd_assets(void *ctx_) {
    TrendModeSimdCtx *ctx = ctx_;
    // Each asset has TRENDMODE_INPUTS=1 input pointer (close price)
    const double *inputs_per_asset[4][TRENDMODE_INPUTS] = {
        {ctx->stocks[0].close}, {ctx->stocks[1].close},
        {ctx->stocks[2].close}, {ctx->stocks[3].close},
    };
    // inputs is an array of num_assets pointers to input arrays
    const double *inputs[4];
    for (int i = 0; i < 4; i++) {
        inputs[i] = (const double *)&inputs_per_asset[i][0];
    }
    struct CSimdResult r = trendmode_simd_by_assets((const double *const *const *)inputs, 4, ctx->data_len, ctx->opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] trendmode_simd_by_assets failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) trendmode_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

static void bench_trendmode_simd_options(void *ctx_) {
    TrendModeSimdCtx *ctx = ctx_;
    const double *inputs[TRENDMODE_INPUTS] = {ctx->stocks->close};
    // TRENDMODE_OPTIONS=2: opts[4] points at whole option_sets rows
    const double *opts[4] = {ctx->option_sets[0], ctx->option_sets[1],
                             ctx->option_sets[2], ctx->option_sets[3]};
    struct CSimdResult r = trendmode_simd_by_options(inputs, ctx->data_len, opts, 4, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] trendmode_simd_by_options failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) trendmode_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

static void run_trendmode(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][TRENDMODE_OPTIONS] = {{0.0}, {0.05}, {0.07}, {0.10}};
    printf("\n--- TRENDMODE ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            TrendModeCtx ctx = {.stock = &stocks[s], .alpha = option_sets[o][0]};

            TimingResult t = time_fn(bench_trendmode, &ctx, number, repeat, warmup);
            log_and_print("trendmode", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], TRENDMODE_OPTIONS, t, (int) stocks[s].len);
        }
    }

    // ---- SIMD runs (optional outputs off: NULL, 0) ----
    if (num_stocks >= 4) {
        size_t dlen = stocks[0].len;
        for (int i = 1; i < 4; i++) if (stocks[i].len < dlen) dlen = stocks[i].len;

        TrendModeSimdCtx sctx = { .stocks = stocks, .data_len = dlen };
        for (int o = 0; o < 4; o++) {
            sctx.opts = option_sets[o];
            TimingResult t_sa = time_fn(bench_trendmode_simd_assets, &sctx, number, repeat, warmup);
            log_and_print("trendmode", "tulip_rs_ffi_c_simd_by_assets", "All", option_sets[o], TRENDMODE_OPTIONS, t_sa, (int) dlen);
        }

        for (int s = 0; s < num_stocks; s++) {
            TrendModeSimdCtx octx = { .stocks = &stocks[s], .data_len = stocks[s].len, .option_sets = option_sets };
            // One call times 4 option sets; first set logged as the representative key.
            TimingResult t_so = time_fn(bench_trendmode_simd_options, &octx, number, repeat, warmup);
            log_and_print("trendmode", "tulip_rs_ffi_c_simd_by_options", stocks[s].symbol, option_sets[0], TRENDMODE_OPTIONS, t_so, (int) stocks[s].len);
        }
    }
}
