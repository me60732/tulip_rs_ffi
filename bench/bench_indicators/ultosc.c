// ultosc (Ultimate Oscillator) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a multi-input indicator (high, low, close)
// with 3 options and no optional outputs.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    double short_period, medium_period, long_period;
} UltoscCtx;

static void bench_ultosc(void *ctx_) {
    UltoscCtx *ctx = ctx_;
    const double *inputs[ULTOSC_INPUTS] = {ctx->stock->high, ctx->stock->low, ctx->stock->close};
    double opts[ULTOSC_OPTIONS] = {ctx->short_period, ctx->medium_period, ctx->long_period};
    struct CIndicatorResult r = ultosc_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] ultosc_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    ultosc_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_ultosc.rs's bench_c_ultosc exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_ultosc(void *ctx_) {
    UltoscCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    double options[ULTOSC_OPTIONS] = {ctx->short_period, ctx->medium_period, ctx->long_period};
    int start_index = ti_ultosc_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_ultosc_start returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[ULTOSC_INPUTS] = {ctx->stock->high, ctx->stock->low, ctx->stock->close};
    double *outputs[1] = {output};
    int ret = ti_ultosc((int) len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_ultosc returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib". Mirrors
// tulip_test/benches/benchmark_ultosc.rs's bench_talib_ultosc exactly.
// ---------------------------------------------------------------------------

static void bench_talib_ultosc(void *ctx_) {
    UltoscCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    int start_index =
        TA_ULTOSC_Lookback((int) ctx->short_period, (int) ctx->medium_period, (int) ctx->long_period);
    if (start_index < 0) { fprintf(stderr, "[error] TA_ULTOSC_Lookback returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    int out_begin = 0, out_nb_element = 0;
    TA_RetCode ret =
        TA_ULTOSC(0, (int) len - 1,
                  ctx->stock->high, ctx->stock->low, ctx->stock->close,
                  (int) ctx->short_period, (int) ctx->medium_period, (int) ctx->long_period,
                  &out_begin, &out_nb_element, output);
    if (ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_ULTOSC returned %d\n", (int) ret); exit(1); }
    free(output);
}

static void run_ultosc(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][ULTOSC_OPTIONS] = {
        {7.0, 14.0, 28.0}, {4.0, 8.0, 16.0}, {5.0, 10.0, 20.0}, {6.0, 12.0, 24.0}};
    printf("\n--- ULTOSC ---\n");
    for (int s = 0; s < num_stocks; s++) {
        size_t len = stocks[s].len;
        double *inputs_buf = malloc(sizeof(double) * len * 3);
        memcpy(inputs_buf, stocks[s].high, sizeof(double) * len);
        memcpy(inputs_buf + len, stocks[s].low, sizeof(double) * len);
        memcpy(inputs_buf + 2 * len, stocks[s].close, sizeof(double) * len);

        for (int o = 0; o < 4; o++) {
            UltoscCtx ctx = {
                .stock = &stocks[s],
                .short_period = option_sets[o][0],
                .medium_period = option_sets[o][1],
                .long_period = option_sets[o][2],
            };

            TimingResult t = time_fn(bench_ultosc, &ctx, number, repeat, warmup);
            log_and_print("ultosc", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 3, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_ultosc, &ctx, number, repeat, warmup);
            log_and_print("ultosc", "C_tulip", stocks[s].symbol, option_sets[o], 3, t_c, (int) stocks[s].len);

            TimingResult t_talib = time_fn(bench_talib_ultosc, &ctx, number, repeat, warmup);
            log_and_print("ultosc", "talib", stocks[s].symbol, option_sets[o], 3, t_talib, (int) stocks[s].len);
        }

        free(inputs_buf);
    }
}
