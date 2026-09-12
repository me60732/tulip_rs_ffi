// cybercycle (Cyber Cycle) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a single-input indicator with one option,
// with optional outputs requested.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    double alpha;
} CybercycleCtx;

static void bench_cybercycle(void *ctx_) {
    CybercycleCtx *ctx = ctx_;
    const double *inputs[CYBERCYCLE_INPUTS] = {ctx->stock->close};
    double opts[CYBERCYCLE_OPTIONS] = {ctx->alpha};
    struct CIndicatorResult r = cybercycle_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] cybercycle_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    cybercycle_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip".
// NOTE: No ti_cybercycle_start/ti_cybercycle pair exists in tulip_test/src/c_bindings.rs.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib".
// NOTE: TA-Lib does not provide an equivalent for cybercycle.
// ---------------------------------------------------------------------------

static void run_cybercycle(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][CYBERCYCLE_OPTIONS] = {{0.05}, {0.07}, {0.10}, {0.15}};
    printf("\n--- CYBERCYCLE ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            CybercycleCtx ctx = {
                .stock = &stocks[s],
                .alpha = option_sets[o][0],
            };

            TimingResult t = time_fn(bench_cybercycle, &ctx, number, repeat, warmup);
            log_and_print("cybercycle", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], CYBERCYCLE_OPTIONS, t, (int) stocks[s].len);

            // No C_tulip comparison available
            // No talib comparison available
        }
    }
}
