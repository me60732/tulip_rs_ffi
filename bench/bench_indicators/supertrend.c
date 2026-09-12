// supertrend (Super Trend) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a multi-input indicator with options and
// optional outputs requested.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    double period, step;
} SuperTrendCtx;

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
}
