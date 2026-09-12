// stddev (Standard Deviation) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a single-input indicator with options and
// optional_outputs requested.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    double period;
} StdDevCtx;

static void bench_stddev(void *ctx_) {
    StdDevCtx *ctx = ctx_;
    const double *inputs[STDDEV_INPUTS] = {ctx->stock->close};
    double opts[STDDEV_OPTIONS] = {ctx->period};
    struct CIndicatorResult r = stddev_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] stddev_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    stddev_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_stddev.rs's bench_c_stddev exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_stddev(void *ctx_) {
    StdDevCtx *ctx = ctx_;
    double options[STDDEV_OPTIONS] = {ctx->period};
    int start_index = ti_stddev_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_stddev_start returned negative index\n"); exit(1); }
    int output_len = (int) ctx->stock->len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[STDDEV_INPUTS] = {ctx->stock->close};
    double *outputs[1] = {output};
    int ret = ti_stddev((int) ctx->stock->len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_stddev returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib". Mirrors
// tulip_test/benches/benchmark_stddev.rs's bench_talib_stddev exactly.
// Note: TA_STDDEV requires optInNbDev parameter; using 1.0 (default for std dev).
// ---------------------------------------------------------------------------

static void bench_talib_stddev(void *ctx_) {
    StdDevCtx *ctx = ctx_;
    int size = (int) ctx->stock->len;
    double options[STDDEV_OPTIONS] = {ctx->period};
    int start_index = TA_STDDEV_Lookback((int) options[0], 1.0);
    if (start_index < 0) { fprintf(stderr, "[error] TA_STDDEV_Lookback returned negative index\n"); exit(1); }
    int output_len = size - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    int out_begin = 0, out_nb_element = 0;
    TA_RetCode ret =
        TA_STDDEV(0, size - 1, ctx->stock->close,
                  (int) options[0], 1.0,
                  &out_begin, &out_nb_element, output);
    if (ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_STDDEV returned %d\n", (int) ret); exit(1); }
    free(output);
}

static void run_stddev(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][STDDEV_OPTIONS] = {{5.0}, {14.0}, {20.0}, {50.0}};
    printf("\n--- STDDEV ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            StdDevCtx ctx = {.stock = &stocks[s], .period = option_sets[o][0]};

            TimingResult t = time_fn(bench_stddev, &ctx, number, repeat, warmup);
            log_and_print("stddev", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], STDDEV_OPTIONS, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_stddev, &ctx, number, repeat, warmup);
            log_and_print("stddev", "C_tulip", stocks[s].symbol, option_sets[o], STDDEV_OPTIONS, t_c, (int) stocks[s].len);

            TimingResult t_talib = time_fn(bench_talib_stddev, &ctx, number, repeat, warmup);
            log_and_print("stddev", "talib", stocks[s].symbol, option_sets[o], STDDEV_OPTIONS, t_talib, (int) stocks[s].len);
        }
    }
}
