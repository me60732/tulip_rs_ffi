// roofingfilter (Roofing Filter) -- tulip_rs_ffi extern "C" API.
//
// This is one of the three "template" indicators (alongside macd.c and
// stoch.c) that establish the pattern every other file under bench_indicators/
// follows. See ../README.md "Adding a new indicator to this harness".
//
// The timed call performs one full cycle of the wrapper: roofingfilter_indicator()
// (which allocates its outputs inside the wrapper) followed by
// tulip_ffi_result_free() + roofingfilter_state_free(), so allocation/teardown cost
// is included in the measurement.

#include "tulip_rs_ffi.h"
#include "../bench_common.h"

typedef struct {
    const Stock *stock;
    double ss_period, hp_period;
} RoofingFilterCtx;

static void bench_roofingfilter(void *ctx_) {
    RoofingFilterCtx *ctx = ctx_;
    const double *inputs[ROOFINGFILTER_INPUTS] = {ctx->stock->close};
    double opts[ROOFINGFILTER_OPTIONS] = {ctx->ss_period, ctx->hp_period};
    struct CIndicatorResult r = roofingfilter_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] roofingfilter_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    roofingfilter_state_free(r.state);
}

// ---------------------------------------------------------------------------
// No C_tulip or TA-Lib comparison: neither library exports an equivalent
// (ti_roofingfilter does not exist; see the provenance note above).
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// SIMD comparisons -- optional outputs always NULL (off).
// roofingfilter_simd_by_assets() runs one option set across 4 assets in a single call;
// roofingfilter_simd_by_options() runs 4 option sets on one asset in a single call.
// ---------------------------------------------------------------------------

typedef struct {
    const Stock *stocks;   // by_assets: array of 4 stocks; by_options: single stock
    size_t data_len;       // bars per asset (shared across the 4 for by_assets)
    const double *opts;    // by_assets: the single shared option set
    const double (*option_sets)[ROOFINGFILTER_OPTIONS]; // by_options: 4 option sets
} RoofingFilterSimdCtx;

static void bench_roofingfilter_simd_assets(void *ctx_) {
    RoofingFilterSimdCtx *ctx = ctx_;
    // Each asset has ROOFINGFILTER_INPUTS=1 input pointer (close price)
    const double *inputs_per_asset[4][ROOFINGFILTER_INPUTS] = {
        {ctx->stocks[0].close}, {ctx->stocks[1].close},
        {ctx->stocks[2].close}, {ctx->stocks[3].close},
    };
    // inputs is an array of num_assets pointers to input arrays
    const double *inputs[4];
    for (int i = 0; i < 4; i++) {
        inputs[i] = (const double *)&inputs_per_asset[i][0];
    }
    struct CSimdResult r = roofingfilter_simd_by_assets((const double *const *const *)inputs, 4, ctx->data_len, ctx->opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] roofingfilter_simd_by_assets failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) roofingfilter_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

static void bench_roofingfilter_simd_options(void *ctx_) {
    RoofingFilterSimdCtx *ctx = ctx_;
    const double *inputs[ROOFINGFILTER_INPUTS] = {ctx->stocks->close};
    const double *opts[4] = {ctx->option_sets[0], ctx->option_sets[1],
                             ctx->option_sets[2], ctx->option_sets[3]};
    struct CSimdResult r = roofingfilter_simd_by_options(inputs, ctx->data_len, opts, 4, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] roofingfilter_simd_by_options failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) roofingfilter_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

static void run_roofingfilter(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][ROOFINGFILTER_OPTIONS] = {{10.0, 20.0}, {15.0, 30.0}, {20.0, 40.0}, {25.0, 50.0}};
    printf("\n--- ROOFINGFILTER ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            RoofingFilterCtx ctx = {
                .stock = &stocks[s],
                .ss_period = option_sets[o][0],
                .hp_period = option_sets[o][1],
            };

            TimingResult t = time_fn(bench_roofingfilter, &ctx, number, repeat, warmup);
            log_and_print("roofingfilter", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 2, t, (int) stocks[s].len);

            // Note: C_tulip and talib comparisons not available due to missing implementations
        }
    }

    // ---- SIMD runs (optional outputs off: NULL, 0) ----
    if (num_stocks >= 4) {
        size_t dlen = stocks[0].len;
        for (int i = 1; i < 4; i++) if (stocks[i].len < dlen) dlen = stocks[i].len;

        RoofingFilterSimdCtx sctx = { .stocks = stocks, .data_len = dlen };
        for (int o = 0; o < 4; o++) {
            sctx.opts = option_sets[o];
            TimingResult t_sa = time_fn(bench_roofingfilter_simd_assets, &sctx, number, repeat, warmup);
            log_and_print("roofingfilter", "tulip_rs_ffi_c_simd_by_assets", "All", option_sets[o], 2, t_sa, (int) dlen);
        }

        for (int s = 0; s < num_stocks; s++) {
            RoofingFilterSimdCtx octx = { .stocks = &stocks[s], .data_len = stocks[s].len, .option_sets = option_sets };
            // One call times 4 option sets; first set logged as the representative key.
            TimingResult t_so = time_fn(bench_roofingfilter_simd_options, &octx, number, repeat, warmup);
            log_and_print("roofingfilter", "tulip_rs_ffi_c_simd_by_options", stocks[s].symbol, option_sets[0], 2, t_so, (int) stocks[s].len);
        }
    }
}
