// trvi (True Range Volatility Indicator) -- tulip_rs_ffi extern "C" API.
//
// This is one of the three "template" indicators (alongside macd.c and
// stoch.c) that establish the pattern every other file under bench_indicators/
// follows. See ../README.md "Adding a new indicator to this harness".

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    double period;
} TrviCtx;

static void bench_trvi(void *ctx_) {
    TrviCtx *ctx = ctx_;
    const double *inputs[TRVI_INPUTS] = {ctx->stock->high, ctx->stock->low, ctx->stock->close};
    double opts[TRVI_OPTIONS] = {ctx->period};
    struct CIndicatorResult r = trvi_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] trvi_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    trvi_state_free(r.state);
}

// ---------------------------------------------------------------------------
// No C_tulip or TA-Lib comparison: the vendored Tulip Indicators C library
// exports no ti_trvi, and TA-Lib has no TRVI equivalent.
// ---------------------------------------------------------------------------

static void run_trvi(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][TRVI_OPTIONS] = {{5.0}, {14.0}, {20.0}, {30.0}};
    printf("\n--- TRVI ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            TrviCtx ctx = {.stock = &stocks[s], .period = option_sets[o][0]};

            TimingResult t = time_fn(bench_trvi, &ctx, number, repeat, warmup);
            log_and_print("trvi", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], TRVI_OPTIONS, t, (int) stocks[s].len);
        }
    }
}
