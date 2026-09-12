// trendmode (Trend Mode) -- tulip_rs_ffi extern "C" API.
//
// Single-input indicator with one option.

#include "tulip_rs_ffi.h"

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
}
