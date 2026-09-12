// fosc (Force Oscillator) -- tulip_rs_ffi extern "C" API.
//
// This is one of the three "template" indicators (alongside macd.c and
// stoch.c) that establish the pattern every other file under bench_indicators/
// follows. See ../README.md "Adding a new indicator to this harness".

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    double period;
} FoscCtx;

static void bench_fosc(void *ctx_) {
    FoscCtx *ctx = ctx_;
    const double *inputs[FOSC_INPUTS] = {ctx->stock->close};
    double opts[FOSC_OPTIONS] = {ctx->period};
    struct CIndicatorResult r = fosc_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] fosc_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    fosc_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_fosc.rs's bench_c_fosc exactly (lookback
// computation + output allocation inside the timed region).
// ---------------------------------------------------------------------------

static void bench_tulipc_fosc(void *ctx_) {
    FoscCtx *ctx = ctx_;
    double options[FOSC_OPTIONS] = {ctx->period};
    int start_index = ti_fosc_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_fosc_start returned negative index\n"); exit(1); }
    int output_len = (int) ctx->stock->len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[FOSC_INPUTS] = {ctx->stock->close};
    double *outputs[1] = {output};
    int ret = ti_fosc((int) ctx->stock->len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_fosc returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- not available. TA-Lib has no FOSC equivalent.
// ---------------------------------------------------------------------------

static void run_fosc(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][FOSC_OPTIONS] = {{5.0}, {14.0}, {20.0}, {30.0}};
    printf("\n--- FOSC ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            FoscCtx ctx = {.stock = &stocks[s], .period = option_sets[o][0]};

            TimingResult t = time_fn(bench_fosc, &ctx, number, repeat, warmup);
            log_and_print("fosc", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 1, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_fosc, &ctx, number, repeat, warmup);
            log_and_print("fosc", "C_tulip", stocks[s].symbol, option_sets[o], 1, t_c, (int) stocks[s].len);

            // No talib comparison - Fosc is unique to Tulip
        }
    }
}
