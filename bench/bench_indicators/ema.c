// ema (Exponential Moving Average) -- tulip_rs_ffi extern "C" API.
//
// This is one of the three "template" indicators (alongside macd.c and
// stoch.c) that establish the pattern every other file under bench_indicators/
// follows. See ../README.md "Adding a new indicator to this harness".

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    double period;
} EmaCtx;

static void bench_ema(void *ctx_) {
    EmaCtx *ctx = ctx_;
    const double *inputs[EMA_INPUTS] = {ctx->stock->close};
    double opts[EMA_OPTIONS] = {ctx->period};
    struct CIndicatorResult r = ema_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] ema_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    ema_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_ema.rs's bench_c_ema exactly (lookback
// computation + output allocation inside the timed region).
// ---------------------------------------------------------------------------

static void bench_tulipc_ema(void *ctx_) {
    EmaCtx *ctx = ctx_;
    double options[EMA_OPTIONS] = {ctx->period};
    int start_index = ti_ema_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_ema_start returned negative index\n"); exit(1); }
    int output_len = (int) ctx->stock->len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[EMA_INPUTS] = {ctx->stock->close};
    double *outputs[1] = {output};
    int ret = ti_ema((int) ctx->stock->len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_ema returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib". Mirrors
// tulip_test/benches/benchmark_ema.rs's bench_talib_ema exactly.
// ---------------------------------------------------------------------------

static void bench_talib_ema(void *ctx_) {
    EmaCtx *ctx = ctx_;
    int size = (int) ctx->stock->len;
    int start_index = TA_EMA_Lookback((int) ctx->period);
    if (start_index < 0) { fprintf(stderr, "[error] TA_EMA_Lookback returned negative index\n"); exit(1); }
    int output_len = size - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    int out_begin = 0, out_nb_element = 0;
    TA_RetCode ret = TA_EMA(0, size - 1, ctx->stock->close, (int) ctx->period, &out_begin, &out_nb_element, output);
    if (ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_EMA returned %d\n", (int) ret); exit(1); }
    free(output);
}

static void run_ema(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][EMA_OPTIONS] = {{14.0}, {20.0}, {26.0}, {50.0}};
    printf("\n--- EMA ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            EmaCtx ctx = {.stock = &stocks[s], .period = option_sets[o][0]};

            TimingResult t = time_fn(bench_ema, &ctx, number, repeat, warmup);
            log_and_print("ema", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 1, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_ema, &ctx, number, repeat, warmup);
            log_and_print("ema", "C_tulip", stocks[s].symbol, option_sets[o], 1, t_c, (int) stocks[s].len);

            TimingResult t_talib = time_fn(bench_talib_ema, &ctx, number, repeat, warmup);
            log_and_print("ema", "talib", stocks[s].symbol, option_sets[o], 1, t_talib, (int) stocks[s].len);
        }
    }
}
