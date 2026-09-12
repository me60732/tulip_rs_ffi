// tsf (Time Series Forecast) -- tulip_rs_ffi extern "C" API.
//
// This is one of the three "template" indicators (alongside macd.c and
// stoch.c) that establish the pattern every other file under bench_indicators/
// follows. See ../README.md "Adding a new indicator to this harness".
//
// The timed call performs one full cycle of the wrapper: tsf_indicator()
// (which allocates its outputs inside the wrapper) followed by
// tulip_ffi_result_free() + tsf_state_free(), so allocation/teardown cost
// is included in the measurement.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    double period;
} TsfCtx;

static void bench_tsf(void *ctx_) {
    TsfCtx *ctx = ctx_;
    const double *inputs[TSF_INPUTS] = {ctx->stock->close};
    double opts[TSF_OPTIONS] = {ctx->period};
    struct CIndicatorResult r = tsf_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] tsf_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    tsf_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_tsf.rs's bench_c_tsf exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_tsf(void *ctx_) {
    TsfCtx *ctx = ctx_;
    double options[TSF_OPTIONS] = {ctx->period};
    int start_index = ti_tsf_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_tsf_start returned negative index\n"); exit(1); }
    int output_len = (int) ctx->stock->len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[TSF_INPUTS] = {ctx->stock->close};
    double *outputs[1] = {output};
    int ret = ti_tsf((int) ctx->stock->len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_tsf returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib". Mirrors
// tulip_test/benches/benchmark_tsf.rs's bench_talib_tsf exactly.
// ---------------------------------------------------------------------------

static void bench_talib_tsf(void *ctx_) {
    TsfCtx *ctx = ctx_;
    int size = (int) ctx->stock->len;
    int start_index = TA_TSF_Lookback((int) ctx->period);
    if (start_index < 0) { fprintf(stderr, "[error] TA_TSF_Lookback returned negative index\n"); exit(1); }
    int output_len = size - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    int out_begin = 0, out_nb_element = 0;
    TA_RetCode ret = TA_TSF(0, size - 1, ctx->stock->close, (int) ctx->period,
                            &out_begin, &out_nb_element, output);
    if (ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_TSF returned %d\n", (int) ret); exit(1); }
    free(output);
}

static void run_tsf(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][TSF_OPTIONS] = {{5.0}, {14.0}, {20.0}, {50.0}};
    printf("\n--- TSF ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            TsfCtx ctx = {.stock = &stocks[s], .period = option_sets[o][0]};

            TimingResult t = time_fn(bench_tsf, &ctx, number, repeat, warmup);
            log_and_print("tsf", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 1, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_tsf, &ctx, number, repeat, warmup);
            log_and_print("tsf", "C_tulip", stocks[s].symbol, option_sets[o], 1, t_c, (int) stocks[s].len);

            TimingResult t_talib = time_fn(bench_talib_tsf, &ctx, number, repeat, warmup);
            log_and_print("tsf", "talib", stocks[s].symbol, option_sets[o], 1, t_talib, (int) stocks[s].len);
        }
    }
}
