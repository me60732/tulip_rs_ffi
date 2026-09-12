// mama (MESA Adaptive Moving Average) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating an indicator with a single input and options,
// with optional_outputs requested.

#include "tulip_rs_ffi.h"

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
}
