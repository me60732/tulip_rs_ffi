// typprice (Typical Price) -- tulip_rs_ffi extern "C" API.
//
// This is one of the three "template" indicators (alongside macd.c and
// stoch.c) that establish the pattern every other file under bench_indicators/
// follows. See ../README.md "Adding a new indicator to this harness".
//
// The timed call performs one full cycle of the wrapper: typprice_indicator()
// (which allocates its outputs inside the wrapper) followed by
// tulip_ffi_result_free() + typprice_state_free(), so allocation/teardown cost
// is included in the measurement.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
} TyppriceCtx;

static void bench_typprice(void *ctx_) {
    TyppriceCtx *ctx = ctx_;
    const double *inputs[TYPPRICE_INPUTS] = {ctx->stock->high, ctx->stock->low, ctx->stock->close};
    // TYPPRICE_OPTIONS is 0; omit opts array entirely and pass NULL
    struct CIndicatorResult r = typprice_indicator(inputs, ctx->stock->len, NULL, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] typprice_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    typprice_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_typprice.rs's bench_rust_typprice exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_typprice(void *ctx_) {
    TyppriceCtx *ctx = ctx_;
    double options[TYPPRICE_OPTIONS] = {};
    int start_index = ti_typprice_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_typprice_start returned negative index\n"); exit(1); }
    int output_len = (int) ctx->stock->len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[TYPPRICE_INPUTS] = {ctx->stock->high, ctx->stock->low, ctx->stock->close};
    double *outputs[1] = {output};
    int ret = ti_typprice((int) ctx->stock->len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_typprice returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib". Mirrors
// tulip_test/benches/benchmark_typprice.rs's bench_talib_typprice exactly.
// ---------------------------------------------------------------------------

static void bench_talib_typprice(void *ctx_) {
    TyppriceCtx *ctx = ctx_;
    int size = (int) ctx->stock->len;
    int start_index = TA_TYPPRICE_Lookback();
    if (start_index < 0) { fprintf(stderr, "[error] TA_TYPPRICE_Lookback returned negative index\n"); exit(1); }
    int output_len = size - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    int out_begin = 0, out_nb_element = 0;
    TA_RetCode ret = TA_TYPPRICE(0, size - 1,
                                 ctx->stock->high, ctx->stock->low, ctx->stock->close,
                                 &out_begin, &out_nb_element, output);
    if (ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_TYPPRICE returned %d\n", (int) ret); exit(1); }
    free(output);
}

static void run_typprice(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    printf("\n--- TYPPRICE ---\n");
    for (int s = 0; s < num_stocks; s++) {
        size_t len = stocks[s].len;
        double *inputs_buf = malloc(sizeof(double) * len * 3);
        memcpy(inputs_buf, stocks[s].high, sizeof(double) * len);
        memcpy(inputs_buf + len, stocks[s].low, sizeof(double) * len);
        memcpy(inputs_buf + 2 * len, stocks[s].close, sizeof(double) * len);

        TyppriceCtx ctx = {
            .stock = &stocks[s],
        };

        TimingResult t = time_fn(bench_typprice, &ctx, number, repeat, warmup);
        log_and_print("typprice", "tulip_rs_ffi_c", stocks[s].symbol, NULL, 0, t, (int) stocks[s].len);

        TimingResult t_c = time_fn(bench_tulipc_typprice, &ctx, number, repeat, warmup);
        log_and_print("typprice", "C_tulip", stocks[s].symbol, NULL, 0, t_c, (int) stocks[s].len);

        TimingResult t_talib = time_fn(bench_talib_typprice, &ctx, number, repeat, warmup);
        log_and_print("typprice", "talib", stocks[s].symbol, NULL, 0, t_talib, (int) stocks[s].len);

        free(inputs_buf);
    }
}
