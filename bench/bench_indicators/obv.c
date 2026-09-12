// obv (On Balance Volume) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a two-input indicator with no options and
// no optional_outputs requested.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
} ObvCtx;

static void bench_obv(void *ctx_) {
    ObvCtx *ctx = ctx_;
    const double *inputs[OBV_INPUTS] = {ctx->stock->close, ctx->stock->volume};
    struct CIndicatorResult r = obv_indicator(inputs, ctx->stock->len, NULL, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] obv_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    obv_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_obv.rs's bench_c_obv exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_obv(void *ctx_) {
    ObvCtx *ctx = ctx_;
    double options[0] = {};
    int start_index = ti_obv_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_obv_start returned negative index\n"); exit(1); }
    int output_len = (int) ctx->stock->len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[OBV_INPUTS] = {ctx->stock->close, ctx->stock->volume};
    double *outputs[1] = {output};
    int ret = ti_obv((int) ctx->stock->len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_obv returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib". Mirrors
// tulip_test/benches/benchmark_obv.rs's bench_talib_obv exactly.
// ---------------------------------------------------------------------------

static void bench_talib_obv(void *ctx_) {
    ObvCtx *ctx = ctx_;
    int size = (int) ctx->stock->len;
    int start_index = TA_OBV_Lookback();
    if (start_index < 0) { fprintf(stderr, "[error] TA_OBV_Lookback returned negative index\n"); exit(1); }
    int output_len = size - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    int out_begin = 0, out_nb_element = 0;
    TA_RetCode ret =
        TA_OBV(0, size - 1, ctx->stock->close, ctx->stock->volume,
               &out_begin, &out_nb_element, output);
    if (ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_OBV returned %d\n", (int) ret); exit(1); }
    free(output);
}

static void run_obv(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    printf("\n--- OBV ---\n");
    for (int s = 0; s < num_stocks; s++) {
        ObvCtx ctx = {.stock = &stocks[s]};

        TimingResult t = time_fn(bench_obv, &ctx, number, repeat, warmup);
        log_and_print("obv", "tulip_rs_ffi_c", stocks[s].symbol, NULL, 0, t, (int) stocks[s].len);

        TimingResult t_c = time_fn(bench_tulipc_obv, &ctx, number, repeat, warmup);
        log_and_print("obv", "C_tulip", stocks[s].symbol, NULL, 0, t_c, (int) stocks[s].len);

        TimingResult t_talib = time_fn(bench_talib_obv, &ctx, number, repeat, warmup);
        log_and_print("obv", "talib", stocks[s].symbol, NULL, 0, t_talib, (int) stocks[s].len);
    }
}
