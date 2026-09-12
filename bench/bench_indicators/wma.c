// wma (Weighted Moving Average) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a single-input, single-option indicator with
// optional_outputs requested.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    double period;
} WmaCtx;

static void bench_wma(void *ctx_) {
    WmaCtx *ctx = ctx_;
    const double *inputs[WMA_INPUTS] = {ctx->stock->close};
    double opts[WMA_OPTIONS] = {ctx->period};
    struct CIndicatorResult r = wma_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] wma_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    wma_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_wma.rs's bench_c_wma exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_wma(void *ctx_) {
    WmaCtx *ctx = ctx_;
    double options[1] = {ctx->period};
    int start_index = ti_wma_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_wma_start returned negative index\n"); exit(1); }
    int output_len = (int) ctx->stock->len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[1] = {ctx->stock->close};
    double *outputs[1] = {output};
    int ret = ti_wma((int) ctx->stock->len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_wma returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib". Mirrors
// tulip_test/benches/benchmark_wma.rs's bench_talib_wma exactly.
// ---------------------------------------------------------------------------

static void bench_talib_wma(void *ctx_) {
    WmaCtx *ctx = ctx_;
    int size = (int) ctx->stock->len;
    int start_index = TA_WMA_Lookback((int) ctx->period);
    if (start_index < 0) { fprintf(stderr, "[error] TA_WMA_Lookback returned negative index\n"); exit(1); }
    int output_len = size - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    int out_begin = 0, out_nb_element = 0;
    TA_RetCode ret = TA_WMA(0, size - 1, ctx->stock->close, (int) ctx->period, &out_begin, &out_nb_element, output);
    if (ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_WMA returned %d\n", (int) ret); exit(1); }
    free(output);
}

static void run_wma(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][WMA_OPTIONS] = {{14.0}, {20.0}, {25.0}, {30.0}};
    printf("\n--- WMA ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            WmaCtx ctx = {.stock = &stocks[s], .period = option_sets[o][0]};

            TimingResult t = time_fn(bench_wma, &ctx, number, repeat, warmup);
            log_and_print("wma", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 1, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_wma, &ctx, number, repeat, warmup);
            log_and_print("wma", "C_tulip", stocks[s].symbol, option_sets[o], 1, t_c, (int) stocks[s].len);

            TimingResult t_talib = time_fn(bench_talib_wma, &ctx, number, repeat, warmup);
            log_and_print("wma", "talib", stocks[s].symbol, option_sets[o], 1, t_talib, (int) stocks[s].len);
        }
    }
}
