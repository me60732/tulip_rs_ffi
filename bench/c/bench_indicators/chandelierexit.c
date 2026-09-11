// chandelierexit (Chandelier Exit) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a multi-input indicator with two options,
// returning two outputs: long and short.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    double period, step;
} ChandelierExitCtx;

static void bench_chandelierexit(void *ctx_) {
    ChandelierExitCtx *ctx = ctx_;
    const double *inputs[CHANDLERIEXIT_INPUTS] = {ctx->stock->high, ctx->stock->low, ctx->stock->close};
    double opts[CHANDLERIEXIT_OPTIONS] = {ctx->period, ctx->step};
    // chandelierexit has 2 optional outputs (long, short)
    bool optional_outputs[2] = {true, true};
    struct CIndicatorResult r = chandelierexit_indicator(inputs, ctx->stock->len, opts, optional_outputs, 2);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] chandelierexit_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    chandelierexit_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip".
// NOTE: No ti_chandelierexit_start/ti_chandelierexit pair exists in tulip_test/src/c_bindings.rs.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib".
// NOTE: TA-Lib does not provide an equivalent for chandelierexit.
// ---------------------------------------------------------------------------

static void run_chandelierexit(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][CHANDLERIEXIT_OPTIONS] = {{14.0, 3.0}, {20.0, 3.0}, {22.0, 3.0}, {22.0, 2.0}};
    printf("\n--- CHANDLERIEXIT ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            ChandelierExitCtx ctx = {
                .stock = &stocks[s],
                .period = option_sets[o][0],
                .step = option_sets[o][1],
            };

            TimingResult t = time_fn(bench_chandelierexit, &ctx, number, repeat, warmup);
            log_and_print("chandelierexit", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], CHANDLERIEXIT_OPTIONS, t, (int) stocks[s].len);

            // No C_tulip comparison available
            // No talib comparison available
        }
    }
}
