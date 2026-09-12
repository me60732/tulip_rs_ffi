// willr (Williams %R) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a multi-input indicator with options and
// optional_outputs requested. The flattened inputs buffer is built once per
// stock and reused across all option sets for that stock.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    const double *inputs_buf; // high ++ low ++ close, each stock->len long
    double period;
} WillrCtx;

static void bench_willr(void *ctx_) {
    WillrCtx *ctx = ctx_;
    const double *inputs[WILLR_INPUTS] = {ctx->stock->high, ctx->stock->low, ctx->stock->close};
    double opts[WILLR_OPTIONS] = {ctx->period};
    struct CIndicatorResult r = willr_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] willr_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    willr_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_willr.rs's bench_c_willr exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_willr(void *ctx_) {
    WillrCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    double options[1] = {ctx->period};
    int start_index = ti_willr_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_willr_start returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[3] = {ctx->stock->high, ctx->stock->low, ctx->stock->close};
    double *outputs[1] = {output};
    int ret = ti_willr((int) len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_willr returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib". Mirrors
// tulip_test/benches/benchmark_willr.rs's bench_talib_willr exactly.
// ---------------------------------------------------------------------------

static void bench_talib_willr(void *ctx_) {
    WillrCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    int start_index = TA_WILLR_Lookback((int) ctx->period);
    if (start_index < 0) { fprintf(stderr, "[error] TA_WILLR_Lookback returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    int out_begin = 0, out_nb_element = 0;
    TA_RetCode ret = TA_WILLR(0, (int) len - 1,
                              ctx->stock->high, ctx->stock->low, ctx->stock->close,
                              (int) ctx->period,
                              &out_begin, &out_nb_element, output);
    if (ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_WILLR returned %d\n", (int) ret); exit(1); }
    free(output);
}

static void run_willr(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][WILLR_OPTIONS] = {{25.0}, {35.0}, {50.0}, {100.0}};
    printf("\n--- WILLR ---\n");
    for (int s = 0; s < num_stocks; s++) {
        size_t len = stocks[s].len;
        double *inputs_buf = malloc(sizeof(double) * len * 3);
        memcpy(inputs_buf, stocks[s].high, sizeof(double) * len);
        memcpy(inputs_buf + len, stocks[s].low, sizeof(double) * len);
        memcpy(inputs_buf + 2 * len, stocks[s].close, sizeof(double) * len);

        for (int o = 0; o < 4; o++) {
            WillrCtx ctx = {
                .stock = &stocks[s],
                .inputs_buf = inputs_buf,
                .period = option_sets[o][0],
            };

            TimingResult t = time_fn(bench_willr, &ctx, number, repeat, warmup);
            log_and_print("willr", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 1, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_willr, &ctx, number, repeat, warmup);
            log_and_print("willr", "C_tulip", stocks[s].symbol, option_sets[o], 1, t_c, (int) stocks[s].len);

            TimingResult t_talib = time_fn(bench_talib_willr, &ctx, number, repeat, warmup);
            log_and_print("willr", "talib", stocks[s].symbol, option_sets[o], 1, t_talib, (int) stocks[s].len);
        }

        free(inputs_buf);
    }
}
