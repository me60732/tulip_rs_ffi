// ichimoku (Ichimoku Cloud) -- tulip_rs_ffi extern "C" API.
//
// This indicator has no C_tulip or talib comparison because neither the Tulip
// Indicators C library nor TA-Lib implement Ichimoku.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    double short_period, long_period;
} IchimokuCtx;

static void bench_ichimoku(void *ctx_) {
    IchimokuCtx *ctx = ctx_;
    const double *inputs[ICHIMOKU_INPUTS] = {ctx->stock->high, ctx->stock->low, ctx->stock->close};
    double opts[ICHIMOKU_OPTIONS] = {ctx->short_period, ctx->long_period};
    struct CIndicatorResult r = ichimoku_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] ichimoku_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    ichimoku_state_free(r.state);
}

static void run_ichimoku(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][ICHIMOKU_OPTIONS] = {{9.0, 26.0}, {5.0, 10.0}, {7.0, 14.0}, {9.0, 52.0}};
    printf("\n--- ICHIMOKU ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            IchimokuCtx ctx = {.stock = &stocks[s], .short_period = option_sets[o][0], .long_period = option_sets[o][1]};

            TimingResult t = time_fn(bench_ichimoku, &ctx, number, repeat, warmup);
            log_and_print("ichimoku", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 2, t, (int) stocks[s].len);
        }
    }
}
