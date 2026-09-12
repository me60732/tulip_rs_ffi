// supersmoother (Super Smoother) -- tulip_rs_ffi extern "C" API.
//
// Single-input indicator with one option.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    double period;
} SuperSmootherCtx;

static void bench_supersmoother(void *ctx_) {
    SuperSmootherCtx *ctx = ctx_;
    const double *inputs[SUPERSMOOTHER_INPUTS] = {ctx->stock->close};
    double opts[SUPERSMOOTHER_OPTIONS] = {ctx->period};
    struct CIndicatorResult r = supersmoother_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] supersmoother_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    supersmoother_state_free(r.state);
}

// ---------------------------------------------------------------------------
// No C_tulip comparison available -- supersmoother has no ti_ equivalent
// in the tulip-c library.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// No TA-Lib comparison available -- supersmoother has no TA_ equivalent
// in the vendor/ta-lib library.
// ---------------------------------------------------------------------------

static void run_supersmoother(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][SUPERSMOOTHER_OPTIONS] = {{10.0}, {20.0}, {30.0}, {40.0}};
    printf("\n--- SUPERSMOOTHER ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            SuperSmootherCtx ctx = {.stock = &stocks[s], .period = option_sets[o][0]};

            TimingResult t = time_fn(bench_supersmoother, &ctx, number, repeat, warmup);
            log_and_print("supersmoother", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], SUPERSMOOTHER_OPTIONS, t, (int) stocks[s].len);
        }
    }
}
