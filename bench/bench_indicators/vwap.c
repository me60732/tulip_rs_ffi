// vwap (Volume-Weighted Average Price) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a multi-input indicator with no options and
// optional_outputs requested.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
} VwapCtx;

static void bench_vwap(void *ctx_) {
    VwapCtx *ctx = ctx_;
    const double *inputs[VWAP_INPUTS] = {ctx->stock->high, ctx->stock->low, ctx->stock->close, ctx->stock->volume};
    // VWAP has no options (VWAP_OPTIONS=0)
    struct CIndicatorResult r = vwap_indicator(inputs, ctx->stock->len, NULL, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] vwap_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    vwap_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- not available. The vendored Tulip
// Indicators C library has no ti_vwap function, so this comparison is omitted.
// ---------------------------------------------------------------------------

static void run_vwap(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    printf("\n--- VWAP ---\n");
    for (int s = 0; s < num_stocks; s++) {
        VwapCtx ctx = {.stock = &stocks[s]};

        TimingResult t = time_fn(bench_vwap, &ctx, number, repeat, warmup);
        log_and_print("vwap", "tulip_rs_ffi_c", stocks[s].symbol, (double[]){}, 0, t, (int) stocks[s].len);

        // No Tulip C comparison available
    }
}
