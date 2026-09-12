// rsi (Relative Strength Index) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a single-input indicator with options and
// no optional_outputs requested.
//
// The timed call performs one full cycle of the wrapper: rsi_indicator()
// (which allocates its outputs inside the wrapper) followed by
// tulip_ffi_result_free() + rsi_state_free(), so allocation/teardown cost
// is included in the measurement.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    double period;
} RsiCtx;

static void bench_rsi(void *ctx_) {
    RsiCtx *ctx = ctx_;
    const double *inputs[RSI_INPUTS] = {ctx->stock->close};
    double opts[RSI_OPTIONS] = {ctx->period};
    struct CIndicatorResult r = rsi_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] rsi_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    rsi_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_rsi.rs's bench_c_rsi exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_rsi(void *ctx_) {
    RsiCtx *ctx = ctx_;
    double options[RSI_OPTIONS] = {ctx->period};
    int start_index = ti_rsi_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_rsi_start returned negative index\n"); exit(1); }
    int output_len = (int) ctx->stock->len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[RSI_INPUTS] = {ctx->stock->close};
    double *outputs[1] = {output};
    int ret = ti_rsi((int) ctx->stock->len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_rsi returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib". Mirrors
// tulip_test/benches/benchmark_rsi.rs's bench_talib_rsi exactly.
// ---------------------------------------------------------------------------

static void bench_talib_rsi(void *ctx_) {
    RsiCtx *ctx = ctx_;
    int size = (int) ctx->stock->len;
    int start_index = TA_RSI_Lookback((int) ctx->period);
    if (start_index < 0) { fprintf(stderr, "[error] TA_RSI_Lookback returned negative index\n"); exit(1); }
    int output_len = size - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    int out_begin = 0, out_nb_element = 0;
    TA_RetCode ret =
        TA_RSI(0, size - 1, ctx->stock->close,
               (int) ctx->period,
               &out_begin, &out_nb_element, output);
    if (ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_RSI returned %d\n", (int) ret); exit(1); }
    free(output);
}

static void run_rsi(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][RSI_OPTIONS] = {{14.0}, {20.0}, {25.0}, {30.0}};
    printf("\n--- RSI ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            RsiCtx ctx = {.stock = &stocks[s], .period = option_sets[o][0]};

            TimingResult t = time_fn(bench_rsi, &ctx, number, repeat, warmup);
            log_and_print("rsi", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 1, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_rsi, &ctx, number, repeat, warmup);
            log_and_print("rsi", "C_tulip", stocks[s].symbol, option_sets[o], 1, t_c, (int) stocks[s].len);

            TimingResult t_talib = time_fn(bench_talib_rsi, &ctx, number, repeat, warmup);
            log_and_print("rsi", "talib", stocks[s].symbol, option_sets[o], 1, t_talib, (int) stocks[s].len);
        }
    }
}
