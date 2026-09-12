// ccfisher (CC Fisher Transform) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating an indicator with a single input and options,
// with optional outputs requested.

#include "tulip_rs_ffi.h"
#include "../bench_common.h"

typedef struct {
    const Stock *stock;
    double alpha;
} CcFisherCtx;

static void bench_ccfisher(void *ctx_) {
    CcFisherCtx *ctx = ctx_;
    const double *inputs[CCFISHER_INPUTS] = {ctx->stock->close};
    double opts[CCFISHER_OPTIONS] = {ctx->alpha};
    struct CIndicatorResult r = ccfisher_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] ccfisher_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    ccfisher_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip".
// NOTE: No ti_ccfisher_start/ti_ccfisher pair exists in tulip_test/src/c_bindings.rs.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib".
// NOTE: TA-Lib does not provide an equivalent for ccfisher.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// SIMD comparisons -- optional outputs always NULL (off).
// *_simd_by_assets() runs one option set across 4 assets in a single call;
// *_simd_by_options() runs 4 option sets on one asset in a single call.
// ---------------------------------------------------------------------------

typedef struct {
    const Stock *stocks;   // by_assets: array of 4 stocks; by_options: single stock
    size_t data_len;       // bars per asset (shared across the 4 for by_assets)
    const double *opts;    // by_assets: the single shared option set
    const double (*option_sets)[CCFISHER_OPTIONS]; // by_options: 4 option sets
} CcFisherSimdCtx;

static void bench_ccfisher_simd_assets(void *ctx_) {
    CcFisherSimdCtx *ctx = ctx_;
    // Each asset has CCFISHER_INPUTS=1 input pointer (close price)
    const double *inputs_per_asset[4][CCFISHER_INPUTS] = {
        {ctx->stocks[0].close}, {ctx->stocks[1].close},
        {ctx->stocks[2].close}, {ctx->stocks[3].close},
    };
    // inputs is an array of num_assets pointers to input arrays
    const double *inputs[4];
    for (int i = 0; i < 4; i++) {
        inputs[i] = (const double *)&inputs_per_asset[i][0];
    }
    struct CSimdResult r = ccfisher_simd_by_assets((const double *const *const *)inputs, 4, ctx->data_len, ctx->opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] ccfisher_simd_by_assets failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) ccfisher_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

static void bench_ccfisher_simd_options(void *ctx_) {
    CcFisherSimdCtx *ctx = ctx_;
    const double *inputs[CCFISHER_INPUTS] = {ctx->stocks->close};
    const double *opts[4] = {ctx->option_sets[0], ctx->option_sets[1],
                             ctx->option_sets[2], ctx->option_sets[3]};
    struct CSimdResult r = ccfisher_simd_by_options(inputs, ctx->data_len, opts, 4, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] ccfisher_simd_by_options failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) ccfisher_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

static void run_ccfisher(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][CCFISHER_OPTIONS] = {{0.0}, {0.05}, {0.07}, {0.10}};
    printf("\n--- CCFISHER ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            CcFisherCtx ctx = {
                .stock = &stocks[s],
                .alpha = option_sets[o][0],
            };

            TimingResult t = time_fn(bench_ccfisher, &ctx, number, repeat, warmup);
            log_and_print("ccfisher", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], CCFISHER_OPTIONS, t, (int) stocks[s].len);

            // No C_tulip comparison available
            // No talib comparison available
        }
    }

    // ---- SIMD runs (optional outputs off: NULL, 0) ----
    if (num_stocks >= 4) {
        size_t dlen = stocks[0].len;
        for (int i = 1; i < 4; i++) if (stocks[i].len < dlen) dlen = stocks[i].len;

        CcFisherSimdCtx sctx = { .stocks = stocks, .data_len = dlen };
        for (int o = 0; o < 4; o++) {
            sctx.opts = option_sets[o];
            TimingResult t_sa = time_fn(bench_ccfisher_simd_assets, &sctx, number, repeat, warmup);
            log_and_print("ccfisher", "tulip_rs_ffi_c_simd_by_assets", "All", option_sets[o], CCFISHER_OPTIONS, t_sa, (int) dlen);
        }

        for (int s = 0; s < num_stocks; s++) {
            CcFisherSimdCtx octx = { .stocks = &stocks[s], .data_len = stocks[s].len, .option_sets = option_sets };
            // One call times 4 option sets; first set logged as the representative key.
            TimingResult t_so = time_fn(bench_ccfisher_simd_options, &octx, number, repeat, warmup);
            log_and_print("ccfisher", "tulip_rs_ffi_c_simd_by_options", stocks[s].symbol, option_sets[0], CCFISHER_OPTIONS, t_so, (int) stocks[s].len);
        }
    }
}
