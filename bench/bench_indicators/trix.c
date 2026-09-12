// trix (TRIX) -- tulip_rs_ffi extern "C" API.
//
// This is one of the three "template" indicators (alongside macd.c and
// stoch.c) that establish the pattern every other file under bench_indicators/
// follows. See ../README.md "Adding a new indicator to this harness".

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    double period;
} TrixCtx;

static void bench_trix(void *ctx_) {
    TrixCtx *ctx = ctx_;
    const double *inputs[TRIX_INPUTS] = {ctx->stock->close};
    double opts[TRIX_OPTIONS] = {ctx->period};
    struct CIndicatorResult r = trix_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] trix_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    trix_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_trix.rs's bench_c_trix exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_trix(void *ctx_) {
    TrixCtx *ctx = ctx_;
    double options[TRIX_OPTIONS] = {ctx->period};
    int start_index = ti_trix_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_trix_start returned negative index\n"); exit(1); }
    int output_len = (int) ctx->stock->len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[1] = {ctx->stock->close};
    double *outputs[1] = {output};
    int ret = ti_trix((int) ctx->stock->len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_trix returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib". Mirrors
// tulip_test/benches/benchmark_trix.rs's bench_talib_trix exactly.
// ---------------------------------------------------------------------------

static void bench_talib_trix(void *ctx_) {
    TrixCtx *ctx = ctx_;
    int size = (int) ctx->stock->len;
    int start_index = TA_TRIX_Lookback((int) ctx->period);
    if (start_index < 0) { fprintf(stderr, "[error] TA_TRIX_Lookback returned negative index\n"); exit(1); }
    int output_len = size - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    int out_begin = 0, out_nb_element = 0;
    TA_RetCode ret = TA_TRIX(0, size - 1, ctx->stock->close, (int) ctx->period,
                             &out_begin, &out_nb_element, output);
    if (ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_TRIX returned %d\n", (int) ret); exit(1); }
    free(output);
}

static void run_trix(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][TRIX_OPTIONS] = {{14.0}, {18.0}, {20.0}, {25.0}};
    printf("\n--- TRIX ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            TrixCtx ctx = {.stock = &stocks[s], .period = option_sets[o][0]};

            TimingResult t = time_fn(bench_trix, &ctx, number, repeat, warmup);
            log_and_print("trix", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], TRIX_OPTIONS, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_trix, &ctx, number, repeat, warmup);
            log_and_print("trix", "C_tulip", stocks[s].symbol, option_sets[o], TRIX_OPTIONS, t_c, (int) stocks[s].len);

            TimingResult t_talib = time_fn(bench_talib_trix, &ctx, number, repeat, warmup);
            log_and_print("trix", "talib", stocks[s].symbol, option_sets[o], TRIX_OPTIONS, t_talib, (int) stocks[s].len);
        }
    }
}
