// volatility (Annualised Historical Volatility) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a single-input indicator with one option.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    double period;
} VolatilityCtx;

static void bench_volatility(void *ctx_) {
    VolatilityCtx *ctx = ctx_;
    const double *inputs[VOLATILITY_INPUTS] = {ctx->stock->close};
    double opts[VOLATILITY_OPTIONS] = {ctx->period};
    struct CIndicatorResult r = volatility_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] volatility_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    volatility_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_volatility.rs's bench_c_volatility exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_volatility(void *ctx_) {
    VolatilityCtx *ctx = ctx_;
    double options[VOLATILITY_OPTIONS] = {ctx->period};
    int start_index = ti_volatility_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_volatility_start returned negative index\n"); exit(1); }
    int output_len = (int) ctx->stock->len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[VOLATILITY_INPUTS] = {ctx->stock->close};
    double *outputs[1] = {output};
    int ret = ti_volatility((int) ctx->stock->len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_volatility returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- not available. The vendored Tulip Indicators C library
// has no TA_Lib equivalent for volatility.
// ---------------------------------------------------------------------------

static void run_volatility(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][VOLATILITY_OPTIONS] = {{5.0}, {14.0}, {20.0}, {50.0}};
    printf("\n--- VOLATILITY ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            VolatilityCtx ctx = {.stock = &stocks[s], .period = option_sets[o][0]};

            TimingResult t = time_fn(bench_volatility, &ctx, number, repeat, warmup);
            log_and_print("volatility", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], VOLATILITY_OPTIONS, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_volatility, &ctx, number, repeat, warmup);
            log_and_print("volatility", "C_tulip", stocks[s].symbol, option_sets[o], VOLATILITY_OPTIONS, t_c, (int) stocks[s].len);

            // No TA-Lib comparison available
        }
    }
}
