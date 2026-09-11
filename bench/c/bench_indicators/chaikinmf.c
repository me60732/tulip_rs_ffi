// chaikinmf (Chaikin Money Flow) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a multi-input indicator with one option.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    double period;
} ChaikinMfCtx;

static void bench_chaikinmf(void *ctx_) {
    ChaikinMfCtx *ctx = ctx_;
    const double *inputs[CHAIKINMF_INPUTS] = {ctx->stock->high, ctx->stock->low, ctx->stock->close, ctx->stock->volume};
    double opts[CHAIKINMF_OPTIONS] = {ctx->period};
    struct CIndicatorResult r = chaikinmf_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] chaikinmf_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    chaikinmf_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip".
// NOTE: No ti_chaikinmf_start/ti_chaikinmf pair exists in tulip_test/src/c_bindings.rs.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib".
// NOTE: TA-Lib does not provide an equivalent for chaikinmf.
// ---------------------------------------------------------------------------

static void run_chaikinmf(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][CHAIKINMF_OPTIONS] = {{14.0}, {20.0}, {25.0}, {30.0}};
    printf("\n--- CHAIKINMF ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            ChaikinMfCtx ctx = {
                .stock = &stocks[s],
                .period = option_sets[o][0],
            };

            TimingResult t = time_fn(bench_chaikinmf, &ctx, number, repeat, warmup);
            log_and_print("chaikinmf", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], CHAIKINMF_OPTIONS, t, (int) stocks[s].len);

            // No C_tulip comparison available
            // No talib comparison available
        }
    }
}
