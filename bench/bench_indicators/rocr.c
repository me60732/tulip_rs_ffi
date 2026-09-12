// rocr (Rate of Change Ratio) -- tulip_rs_ffi extern "C" API.
//
// This is one of the three "template" indicators (alongside macd.c and
// stoch.c) that establish the pattern every other file under bench_indicators/
// follows. See ../README.md "Adding a new indicator to this harness".
//
// The timed call performs one full cycle of the wrapper: rocr_indicator()
// (which allocates its outputs inside the wrapper) followed by
// tulip_ffi_result_free() + rocr_state_free(), so allocation/teardown cost
// is included in the measurement.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    double period;
} RocrCtx;

static void bench_rocr(void *ctx_) {
    RocrCtx *ctx = ctx_;
    const double *inputs[ROCR_INPUTS] = {ctx->stock->close};
    double opts[ROCR_OPTIONS] = {ctx->period};
    struct CIndicatorResult r = rocr_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] rocr_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    rocr_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_rocr.rs's bench_c_rocr exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_rocr(void *ctx_) {
    RocrCtx *ctx = ctx_;
    double options[ROCR_OPTIONS] = {ctx->period};
    int start_index = ti_rocr_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_rocr_start returned negative index\n"); exit(1); }
    int output_len = (int) ctx->stock->len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[ROCR_INPUTS] = {ctx->stock->close};
    double *outputs[1] = {output};
    int ret = ti_rocr((int) ctx->stock->len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_rocr returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib". Mirrors
// tulip_test/benches/benchmark_rocr.rs's bench_talib_rocr exactly.
// ---------------------------------------------------------------------------

static void bench_talib_rocr(void *ctx_) {
    RocrCtx *ctx = ctx_;
    int size = (int) ctx->stock->len;
    int start_index = TA_ROCR_Lookback((int) ctx->period);
    if (start_index < 0) { fprintf(stderr, "[error] TA_ROCR_Lookback returned negative index\n"); exit(1); }
    int output_len = size - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    int out_begin = 0, out_nb_element = 0;
    TA_RetCode ret = TA_ROCR(0, size - 1, ctx->stock->close, (int) ctx->period,
                             &out_begin, &out_nb_element, output);
    if (ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_ROCR returned %d\n", (int) ret); exit(1); }
    free(output);
}

static void run_rocr(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][ROCR_OPTIONS] = {{25.0}, {30.0}, {50.0}, {100.0}};
    printf("\n--- ROCR ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            RocrCtx ctx = {.stock = &stocks[s], .period = option_sets[o][0]};

            TimingResult t = time_fn(bench_rocr, &ctx, number, repeat, warmup);
            log_and_print("rocr", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 1, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_rocr, &ctx, number, repeat, warmup);
            log_and_print("rocr", "C_tulip", stocks[s].symbol, option_sets[o], 1, t_c, (int) stocks[s].len);

            TimingResult t_talib = time_fn(bench_talib_rocr, &ctx, number, repeat, warmup);
            log_and_print("rocr", "talib", stocks[s].symbol, option_sets[o], 1, t_talib, (int) stocks[s].len);
        }
    }
}
