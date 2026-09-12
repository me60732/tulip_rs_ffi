// atr (Average True Range) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a multi-input indicator with options,
// with optional_outputs requested.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    double period;
} AtrCtx;

static void bench_atr(void *ctx_) {
    AtrCtx *ctx = ctx_;
    const double *inputs[ATR_INPUTS] = {ctx->stock->high, ctx->stock->low, ctx->stock->close};
    double opts[ATR_OPTIONS] = {ctx->period};
    struct CIndicatorResult r = atr_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] atr_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    atr_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_atr.rs's bench_c_atr exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_atr(void *ctx_) {
    AtrCtx *ctx = ctx_;
    double options[ATR_OPTIONS] = {ctx->period};
    int start_index = ti_atr_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_atr_start returned negative index\n"); exit(1); }
    int output_len = (int) ctx->stock->len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[ATR_INPUTS] = {ctx->stock->high, ctx->stock->low, ctx->stock->close};
    double *outputs[1] = {output};
    int ret = ti_atr((int) ctx->stock->len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_atr returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib". Mirrors
// tulip_test/benches/benchmark_atr.rs's bench_talib_atr exactly.
// ---------------------------------------------------------------------------

static void bench_talib_atr(void *ctx_) {
    AtrCtx *ctx = ctx_;
    int size = (int) ctx->stock->len;
    int start_index = TA_ATR_Lookback((int) ctx->period);
    if (start_index < 0) { fprintf(stderr, "[error] TA_ATR_Lookback returned negative index\n"); exit(1); }
    int output_len = size - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    int out_begin = 0, out_nb_element = 0;
    TA_RetCode ret = TA_ATR(0, size - 1,
                            ctx->stock->high, ctx->stock->low, ctx->stock->close,
                            (int) ctx->period,
                            &out_begin, &out_nb_element, output);
    if (ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_ATR returned %d\n", (int) ret); exit(1); }
    free(output);
}

static void run_atr(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][ATR_OPTIONS] = {{5.0}, {14.0}, {25.0}, {30.0}};
    printf("\n--- ATR ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            AtrCtx ctx = {
                .stock = &stocks[s],
                .period = option_sets[o][0],
            };

            TimingResult t = time_fn(bench_atr, &ctx, number, repeat, warmup);
            log_and_print("atr", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 1, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_atr, &ctx, number, repeat, warmup);
            log_and_print("atr", "C_tulip", stocks[s].symbol, option_sets[o], 1, t_c, (int) stocks[s].len);

            TimingResult t_talib = time_fn(bench_talib_atr, &ctx, number, repeat, warmup);
            log_and_print("atr", "talib", stocks[s].symbol, option_sets[o], 1, t_talib, (int) stocks[s].len);
        }
    }
}
