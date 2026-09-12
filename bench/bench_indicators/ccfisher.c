// ccfisher (CC Fisher Transform) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating an indicator with a single input and options,
// with optional outputs requested.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    double alpha;
} CcFisherCtx;

static void bench_ccfisher(void *ctx_) {
    CcFisherCtx *ctx = ctx_;
    const double *inputs[CCFISHER_INPUTS] = {ctx->stock->close};
    double opts[CCFISHER_OPTIONS] = {ctx->alpha};
    struct CIndicatorResult r = ccfisher_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] ccfisher_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    ccfisher_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip".
// NOTE: No ti_ccfisher_start/ti_ccfisher pair exists in tulip_test/src/c_bindings.rs.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib".
// NOTE: TA-Lib does not provide an equivalent for ccfisher.
// ---------------------------------------------------------------------------

static void run_ccfisher(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][CCFISHER_OPTIONS] = {{0.0}, {0.05}, {0.07}, {0.10}};
    printf("\n--- CCFISHER ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            CcFisherCtx ctx = {
                .stock = &stocks[s],
                .alpha = option_sets[o][0],
            };

            TimingResult t = time_fn(bench_ccfisher, &ctx, number, repeat, warmup);
            log_and_print("ccfisher", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], CCFISHER_OPTIONS, t, (int) stocks[s].len);

            // No C_tulip comparison available
            // No talib comparison available
        }
    }
}
