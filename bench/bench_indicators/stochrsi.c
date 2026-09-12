// stochrsi (Stochastic RSI) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a single-input indicator with options and
// optional_outputs requested.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    double period;
} StochRsiCtx;

static void bench_stochrsi(void *ctx_) {
    StochRsiCtx *ctx = ctx_;
    const double *inputs[STOCHRSI_INPUTS] = {ctx->stock->close};
    double opts[STOCHRSI_OPTIONS] = {ctx->period};
    struct CIndicatorResult r = stochrsi_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] stochrsi_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    stochrsi_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_stochrsi.rs's bench_c_stochrsi exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_stochrsi(void *ctx_) {
    StochRsiCtx *ctx = ctx_;
    double options[STOCHRSI_OPTIONS] = {ctx->period};
    int start_index = ti_stochrsi_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_stochrsi_start returned negative index\n"); exit(1); }
    int output_len = (int) ctx->stock->len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[STOCHRSI_INPUTS] = {ctx->stock->close};
    double *outputs[1] = {output};
    int ret = ti_stochrsi((int) ctx->stock->len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_stochrsi returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib". Mirrors
// tulip_test/benches/benchmark_stochrsi.rs's bench_talib_stochrsi exactly.
// Note: TA_STOCHRSI uses time_period, fastk_period, fastd_period, fastd_matype.
// ---------------------------------------------------------------------------

static void bench_talib_stochrsi(void *ctx_) {
    StochRsiCtx *ctx = ctx_;
    int size = (int) ctx->stock->len;
    double options[STOCHRSI_OPTIONS] = {ctx->period};
    // TA_STOCHRSI uses time_period, fastk_period, fastd_period, fastd_matype
    // Default values: time_period=14, fastk=5, fastd=3, SMA(0)
    int start_index = TA_STOCHRSI_Lookback((int) options[0], 5, 3, TA_MAType_SMA);
    if (start_index < 0) { fprintf(stderr, "[error] TA_STOCHRSI_Lookback returned negative index\n"); exit(1); }
    int output_len = size - start_index;
    double *fastk = malloc(sizeof(double) * (size_t) output_len);
    double *fastd = malloc(sizeof(double) * (size_t) output_len);
    int out_begin = 0, out_nb_element = 0;
    TA_RetCode ret =
        TA_STOCHRSI(0, size - 1, ctx->stock->close,
                    (int) options[0], 5, 3, TA_MAType_SMA,
                    &out_begin, &out_nb_element, fastk, fastd);
    if (ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_STOCHRSI returned %d\n", (int) ret); exit(1); }
    free(fastk);
    free(fastd);
}

static void run_stochrsi(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][STOCHRSI_OPTIONS] = {{14.0}, {20.0}, {25.0}, {30.0}};
    printf("\n--- STOCHRSI ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            StochRsiCtx ctx = {.stock = &stocks[s], .period = option_sets[o][0]};

            TimingResult t = time_fn(bench_stochrsi, &ctx, number, repeat, warmup);
            log_and_print("stochrsi", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], STOCHRSI_OPTIONS, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_stochrsi, &ctx, number, repeat, warmup);
            log_and_print("stochrsi", "C_tulip", stocks[s].symbol, option_sets[o], STOCHRSI_OPTIONS, t_c, (int) stocks[s].len);

            TimingResult t_talib = time_fn(bench_talib_stochrsi, &ctx, number, repeat, warmup);
            log_and_print("stochrsi", "talib", stocks[s].symbol, option_sets[o], STOCHRSI_OPTIONS, t_talib, (int) stocks[s].len);
        }
    }
}
