// instantaneoustrendline (Instantaneous Trendline) -- tulip_rs_ffi extern "C" API.
//
// This indicator has no C_tulip comparison because the Tulip Indicators C library
// does not implement Instantaneous Trendline. It has a talib comparison using
// TA-Lib's HT_TRENDLINE, which implements a different algorithm (variable-length SMA + 4-bar WMA)
// than Ehlers' 2-pole IIR used by tulip-rs; this benchmark measures throughput only.

#include "tulip_rs_ffi.h"
#include "../bench_common.h"

typedef struct {
    const Stock *stock;
} InstantaneousTrendlineCtx;

static void bench_instantaneoustrendline(void *ctx_) {
    InstantaneousTrendlineCtx *ctx = ctx_;
    const double *inputs[INSTANTANEOUSTRENDLINE_INPUTS] = {ctx->stock->close};
    struct CIndicatorResult r = instantaneoustrendline_indicator(inputs, ctx->stock->len, NULL, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] instantaneoustrendline_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    instantaneoustrendline_state_free(r.state);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib". Mirrors
// tulip_test/benches/benchmark_instantaneoustrendline.rs's bench_talib_ht_trendline.
// Note: HT_TRENDLINE uses a different algorithm than Ehlers' 2-pole IIR;
// this benchmark measures throughput only, not correctness comparison.
// ---------------------------------------------------------------------------

static void bench_talib_ht_trendline(void *ctx_) {
    InstantaneousTrendlineCtx *ctx = ctx_;
    int size = (int) ctx->stock->len;
    int start_index = TA_HT_TRENDLINE_Lookback();
    if (start_index < 0) { fprintf(stderr, "[error] TA_HT_TRENDLINE_Lookback returned negative index\n"); exit(1); }
    int output_len = size - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    int out_begin = 0, out_nb_element = 0;
    TA_RetCode ret = TA_HT_TRENDLINE(0, size - 1, ctx->stock->close, &out_begin, &out_nb_element, output);
    if (ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_HT_TRENDLINE returned %d\n", (int) ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// SIMD comparisons -- optional outputs always NULL (off).
// instantaneoustrendline_simd_by_assets() runs one option set across 4 assets
// in a single call. Note: INSTANTANEOUSTRENDLINE has no options, so there is NO
// simd_by_options variant.
// ---------------------------------------------------------------------------

typedef struct {
    const Stock *stocks;   // array of 4 stocks
    size_t data_len;       // bars per asset (shared across the 4)
} InstantaneousTrendlineSimdCtx;

static void bench_instantaneoustrendline_simd_assets(void *ctx_) {
    InstantaneousTrendlineSimdCtx *ctx = ctx_;
    // Each asset has INSTANTANEOUSTRENDLINE_INPUTS=1 input pointer (close price)
    const double *inputs_per_asset[4][INSTANTANEOUSTRENDLINE_INPUTS] = {
        {ctx->stocks[0].close}, {ctx->stocks[1].close},
        {ctx->stocks[2].close}, {ctx->stocks[3].close},
    };
    // inputs is an array of num_assets pointers to input arrays
    const double *inputs[4];
    for (int i = 0; i < 4; i++) {
        inputs[i] = (const double *)&inputs_per_asset[i][0];
    }
    // INSTANTANEOUSTRENDLINE has no options (INSTANTANEOUSTRENDLINE_OPTIONS=0); mirroring scalar call which passes NULL.
    struct CSimdResult r = instantaneoustrendline_simd_by_assets((const double *const *const *)inputs, 4, ctx->data_len, NULL, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] instantaneoustrendline_simd_by_assets failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) instantaneoustrendline_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

static void run_instantaneoustrendline(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][INSTANTANEOUSTRENDLINE_OPTIONS] = {{}};
    printf("\n--- INSTANTANEoustrendline ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 1; o++) {
            InstantaneousTrendlineCtx ctx = {.stock = &stocks[s]};

            TimingResult t = time_fn(bench_instantaneoustrendline, &ctx, number, repeat, warmup);
            log_and_print("instantaneoustrendline", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 0, t, (int) stocks[s].len);

            TimingResult t_talib = time_fn(bench_talib_ht_trendline, &ctx, number, repeat, warmup);
            log_and_print("instantaneoustrendline", "talib", stocks[s].symbol, option_sets[o], 0, t_talib, (int) stocks[s].len);
        }
    }

    // ---- SIMD runs (optional outputs off: NULL, 0) ----
    if (num_stocks >= 4) {
        size_t dlen = stocks[0].len;
        for (int i = 1; i < 4; i++) if (stocks[i].len < dlen) dlen = stocks[i].len;

        InstantaneousTrendlineSimdCtx ctx = { .stocks = stocks, .data_len = dlen };
        TimingResult t = time_fn(bench_instantaneoustrendline_simd_assets, &ctx, number, repeat, warmup);
        log_and_print("instantaneoustrendline", "tulip_rs_ffi_c_simd_by_assets", "All", NULL, 0, t, (int) dlen);
    }
}
