// ef (Efficiency Ratio) -- tulip_rs_ffi extern "C" API.
//
// This indicator has no C_tulip or talib comparison because neither the Tulip
// Indicators C library nor TA-Lib implement Efficiency Ratio.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    double period;
} EfCtx;

static void bench_ef(void *ctx_) {
    EfCtx *ctx = ctx_;
    const double *inputs[EF_INPUTS] = {ctx->stock->close};
    double opts[EF_OPTIONS] = {ctx->period};
    struct CIndicatorResult r = ef_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] ef_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    ef_state_free(r.state);
}

static void run_ef(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][EF_OPTIONS] = {{5.0}, {10.0}, {14.0}, {20.0}};
    printf("\n--- EF ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            EfCtx ctx = {.stock = &stocks[s], .period = option_sets[o][0]};

            TimingResult t = time_fn(bench_ef, &ctx, number, repeat, warmup);
            log_and_print("ef", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 1, t, (int) stocks[s].len);

            // ef has no C_tulip or talib comparison
        }
    }
}
