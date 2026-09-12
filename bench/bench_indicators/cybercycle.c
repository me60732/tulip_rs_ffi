// cybercycle (Cyber Cycle) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a single-input indicator with one option,
// with optional outputs requested.

#include "tulip_rs_ffi.h"
#include "../bench_common.h"

typedef struct {
    const Stock *stock;
    double alpha;
} CybercycleCtx;

static void bench_cybercycle(void *ctx_) {
    CybercycleCtx *ctx = ctx_;
    const double *inputs[CYBERCYCLE_INPUTS] = {ctx->stock->close};
    double opts[CYBERCYCLE_OPTIONS] = {ctx->alpha};
    struct CIndicatorResult r = cybercycle_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] cybercycle_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    cybercycle_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip".
// NOTE: No ti_cybercycle_start/ti_cybercycle pair exists in tulip_test/src/c_bindings.rs.
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
    const double (*option_sets)[CYBERCYCLE_OPTIONS]; // by_options: 4 option sets
} CybercycleSimdCtx;

static void bench_cybercycle_simd_assets(void *ctx_) {
    CybercycleSimdCtx *ctx = ctx_;
    // Each asset has CYBERCYCLE_INPUTS=1 input pointer (close price)
    const double *inputs_per_asset[4][CYBERCYCLE_INPUTS] = {
        {ctx->stocks[0].close}, {ctx->stocks[1].close},
        {ctx->stocks[2].close}, {ctx->stocks[3].close},
    };
    // inputs is an array of num_assets pointers to input arrays
    const double *inputs[4];
    for (int i = 0; i < 4; i++) {
        inputs[i] = (const double *)&inputs_per_asset[i][0];
    }
    struct CSimdResult r = cybercycle_simd_by_assets((const double *const *const *)inputs, 4, ctx->data_len, ctx->opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] cybercycle_simd_by_assets failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) cybercycle_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

static void bench_cybercycle_simd_options(void *ctx_) {
    CybercycleSimdCtx *ctx = ctx_;
    const double *inputs[CYBERCYCLE_INPUTS] = {ctx->stocks->close};
    const double *opts[4] = {ctx->option_sets[0], ctx->option_sets[1],
                             ctx->option_sets[2], ctx->option_sets[3]};
    struct CSimdResult r = cybercycle_simd_by_options(inputs, ctx->data_len, opts, 4, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] cybercycle_simd_by_options failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) cybercycle_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib".
// NOTE: TA-Lib does not provide an equivalent for cybercycle.
// ---------------------------------------------------------------------------

static void run_cybercycle(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][CYBERCYCLE_OPTIONS] = {{0.05}, {0.07}, {0.10}, {0.15}};
    printf("\n--- CYBERCYCLE ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            CybercycleCtx ctx = {
                .stock = &stocks[s],
                .alpha = option_sets[o][0],
            };

            TimingResult t = time_fn(bench_cybercycle, &ctx, number, repeat, warmup);
            log_and_print("cybercycle", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], CYBERCYCLE_OPTIONS, t, (int) stocks[s].len);

            // No C_tulip comparison available
            // No talib comparison available
        }
    }

    // ---- SIMD runs (optional outputs off: NULL, 0) ----
    if (num_stocks >= 4) {
        size_t dlen = stocks[0].len;
        for (int i = 1; i < 4; i++) if (stocks[i].len < dlen) dlen = stocks[i].len;

        CybercycleSimdCtx sctx = { .stocks = stocks, .data_len = dlen };
        for (int o = 0; o < 4; o++) {
            sctx.opts = option_sets[o];
            TimingResult t_sa = time_fn(bench_cybercycle_simd_assets, &sctx, number, repeat, warmup);
            log_and_print("cybercycle", "tulip_rs_ffi_c_simd_by_assets", "All", option_sets[o], CYBERCYCLE_OPTIONS, t_sa, (int) dlen);
        }

        for (int s = 0; s < num_stocks; s++) {
            CybercycleSimdCtx octx = { .stocks = &stocks[s], .data_len = stocks[s].len, .option_sets = option_sets };
            // One call times 4 option sets; first set logged as the representative key.
            TimingResult t_so = time_fn(bench_cybercycle_simd_options, &octx, number, repeat, warmup);
            log_and_print("cybercycle", "tulip_rs_ffi_c_simd_by_options", stocks[s].symbol, option_sets[0], CYBERCYCLE_OPTIONS, t_so, (int) stocks[s].len);
        }
    }
}
