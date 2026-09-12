// smaenvelope (SMA Envelope) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a single-input indicator with options and
// no optional_outputs requested.
//
// The timed call performs one full cycle of the wrapper: smaenvelope_indicator()
// (which allocates its outputs inside the wrapper) followed by
// tulip_ffi_result_free() + smaenvelope_state_free(), so allocation/teardown cost
// is included in the measurement.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    double period, percentage;
} SmaEnvelopeCtx;

static void bench_smaenvelope(void *ctx_) {
    SmaEnvelopeCtx *ctx = ctx_;
    const double *inputs[SMAENVELOPE_INPUTS] = {ctx->stock->close};
    double opts[SMAENVELOPE_OPTIONS] = {ctx->period, ctx->percentage};
    struct CIndicatorResult r = smaenvelope_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] smaenvelope_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    smaenvelope_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip".
// Note: SMA Envelope has no C_tulip (ti_*) bindings; only tulip_rs_ffi_c comparison is available.
// ---------------------------------------------------------------------------

static void run_smaenvelope(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][SMAENVELOPE_OPTIONS] = {{20.0, 2.5}, {20.0, 5.0}, {50.0, 2.5}, {50.0, 5.0}};
    printf("\n--- SMAENVELOPE ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            SmaEnvelopeCtx ctx = {
                .stock = &stocks[s],
                .period = option_sets[o][0],
                .percentage = option_sets[o][1],
            };

            TimingResult t = time_fn(bench_smaenvelope, &ctx, number, repeat, warmup);
            log_and_print("smaenvelope", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 2, t, (int) stocks[s].len);
        }
    }
}
