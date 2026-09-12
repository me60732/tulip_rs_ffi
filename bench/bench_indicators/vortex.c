// vortex (Vortex Indicator) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a multi-input indicator (high, low, close)
// with one option and 2 outputs (vi_up, vi_down).

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    double period;
} VortexCtx;

static void bench_vortex(void *ctx_) {
    VortexCtx *ctx = ctx_;
    const double *inputs[VORTEX_INPUTS] = {ctx->stock->high, ctx->stock->low, ctx->stock->close};
    double opts[VORTEX_OPTIONS] = {ctx->period};
    struct CIndicatorResult r = vortex_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] vortex_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    vortex_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- not available. The vendored Tulip
// Indicators C library has no ti_vortex function, so this comparison is
// omitted.
// ---------------------------------------------------------------------------

static void run_vortex(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][VORTEX_OPTIONS] = {{14.0}, {20.0}, {25.0}, {30.0}};
    printf("\n--- VORTEX ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            VortexCtx ctx = {.stock = &stocks[s], .period = option_sets[o][0]};

            TimingResult t = time_fn(bench_vortex, &ctx, number, repeat, warmup);
            log_and_print("vortex", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], VORTEX_OPTIONS, t, (int) stocks[s].len);

            // No Tulip C comparison available
        }
    }
}
