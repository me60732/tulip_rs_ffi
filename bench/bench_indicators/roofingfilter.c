// roofingfilter (Roofing Filter) -- tulip_rs_ffi extern "C" API.
//
// This is one of the three "template" indicators (alongside macd.c and
// stoch.c) that establish the pattern every other file under bench_indicators/
// follows. See ../README.md "Adding a new indicator to this harness".
//
// The timed call performs one full cycle of the wrapper: roofingfilter_indicator()
// (which allocates its outputs inside the wrapper) followed by
// tulip_ffi_result_free() + roofingfilter_state_free(), so allocation/teardown cost
// is included in the measurement.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    double ss_period, hp_period;
} RoofingFilterCtx;

static void bench_roofingfilter(void *ctx_) {
    RoofingFilterCtx *ctx = ctx_;
    const double *inputs[ROOFINGFILTER_INPUTS] = {ctx->stock->close};
    double opts[ROOFINGFILTER_OPTIONS] = {ctx->ss_period, ctx->hp_period};
    struct CIndicatorResult r = roofingfilter_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] roofingfilter_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    roofingfilter_state_free(r.state);
}

// ---------------------------------------------------------------------------
// No C_tulip or TA-Lib comparison: neither library exports an equivalent
// (ti_roofingfilter does not exist; see the provenance note above).
// ---------------------------------------------------------------------------

static void run_roofingfilter(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][ROOFINGFILTER_OPTIONS] = {{10.0, 20.0}, {15.0, 30.0}, {20.0, 40.0}, {25.0, 50.0}};
    printf("\n--- ROOFINGFILTER ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            RoofingFilterCtx ctx = {
                .stock = &stocks[s],
                .ss_period = option_sets[o][0],
                .hp_period = option_sets[o][1],
            };

            TimingResult t = time_fn(bench_roofingfilter, &ctx, number, repeat, warmup);
            log_and_print("roofingfilter", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 2, t, (int) stocks[s].len);

            // Note: C_tulip and talib comparisons not available due to missing implementations
        }
    }
}
