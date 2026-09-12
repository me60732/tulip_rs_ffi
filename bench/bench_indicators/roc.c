// roc (Rate of Change) -- tulip_rs_ffi extern "C" API.
//
// This is one of the three "template" indicators (alongside macd.c and
// stoch.c) that establish the pattern every other file under bench_indicators/
// follows. See ../README.md "Adding a new indicator to this harness".

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    double period;
} RocCtx;

static void bench_roc(void *ctx_) {
    RocCtx *ctx = ctx_;
    const double *inputs[ROC_INPUTS] = {ctx->stock->close};
    double opts[ROC_OPTIONS] = {ctx->period};
    struct CIndicatorResult r = roc_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] roc_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    roc_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_roc.rs's bench_c_roc exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_roc(void *ctx_) {
    RocCtx *ctx = ctx_;
    double options[ROC_OPTIONS] = {ctx->period};
    int start_index = ti_roc_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_roc_start returned negative index\n"); exit(1); }
    int output_len = (int) ctx->stock->len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[ROC_INPUTS] = {ctx->stock->close};
    double *outputs[1] = {output};
    int ret = ti_roc((int) ctx->stock->len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_roc returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib". Mirrors
// tulip_test/benches/benchmark_roc.rs's bench_talib_roc exactly.
// ---------------------------------------------------------------------------

static void bench_talib_roc(void *ctx_) {
    RocCtx *ctx = ctx_;
    int size = (int) ctx->stock->len;
    int start_index = TA_ROC_Lookback((int) ctx->period);
    if (start_index < 0) { fprintf(stderr, "[error] TA_ROC_Lookback returned negative index\n"); exit(1); }
    int output_len = size - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    int out_begin = 0, out_nb_element = 0;
    TA_RetCode ret = TA_ROC(0, size - 1, ctx->stock->close, (int) ctx->period,
                            &out_begin, &out_nb_element, output);
    if (ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_ROC returned %d\n", (int) ret); exit(1); }
    free(output);
}

static void run_roc(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][ROC_OPTIONS] = {{25.0}, {30.0}, {50.0}, {100.0}};
    printf("\n--- ROC ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            RocCtx ctx = {.stock = &stocks[s], .period = option_sets[o][0]};

            TimingResult t = time_fn(bench_roc, &ctx, number, repeat, warmup);
            log_and_print("roc", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], ROC_OPTIONS, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_roc, &ctx, number, repeat, warmup);
            log_and_print("roc", "C_tulip", stocks[s].symbol, option_sets[o], ROC_OPTIONS, t_c, (int) stocks[s].len);

            TimingResult t_talib = time_fn(bench_talib_roc, &ctx, number, repeat, warmup);
            log_and_print("roc", "talib", stocks[s].symbol, option_sets[o], ROC_OPTIONS, t_talib, (int) stocks[s].len);
        }
    }
}
