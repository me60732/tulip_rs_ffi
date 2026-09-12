// linreg (Linear Regression) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating an indicator with a single input, options,
// and optional_outputs requested.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    double period;
} LinregCtx;

static void bench_linreg(void *ctx_) {
    LinregCtx *ctx = ctx_;
    const double *inputs[LINREG_INPUTS] = {ctx->stock->close};
    double opts[LINREG_OPTIONS] = {ctx->period};
    struct CIndicatorResult r = linreg_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] linreg_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    linreg_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_linreg.rs's bench_c_linreg exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_linreg(void *ctx_) {
    LinregCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    double options[LINREG_OPTIONS] = {ctx->period};
    int start_index = ti_linreg_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_linreg_start returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[LINREG_INPUTS] = {ctx->stock->close};
    double *outputs[1] = {output};
    int ret = ti_linreg((int) len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_linreg returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib". Mirrors
// tulip_test/benches/benchmark_linreg.rs's bench_talib_linreg exactly.
// ---------------------------------------------------------------------------

static void bench_talib_linreg(void *ctx_) {
    LinregCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    int start_index = TA_LINEARREG_Lookback((int) ctx->period);
    if (start_index < 0) { fprintf(stderr, "[error] TA_LINEARREG_Lookback returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    int out_begin = 0, out_nb_element = 0;
    TA_RetCode ret = TA_LINEARREG(0, (int) len - 1,
                                  ctx->stock->close,
                                  (int) ctx->period,
                                  &out_begin, &out_nb_element, output);
    if (ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_LINEARREG returned %d\n", (int) ret); exit(1); }
    free(output);
}

static void run_linreg(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][LINREG_OPTIONS] = {{5.0}, {14.0}, {20.0}, {50.0}};
    printf("\n--- LINREG ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            LinregCtx ctx = {
                .stock = &stocks[s],
                .period = option_sets[o][0],
            };

            TimingResult t = time_fn(bench_linreg, &ctx, number, repeat, warmup);
            log_and_print("linreg", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], LINREG_OPTIONS, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_linreg, &ctx, number, repeat, warmup);
            log_and_print("linreg", "C_tulip", stocks[s].symbol, option_sets[o], LINREG_OPTIONS, t_c, (int) stocks[s].len);

            TimingResult t_talib = time_fn(bench_talib_linreg, &ctx, number, repeat, warmup);
            log_and_print("linreg", "talib", stocks[s].symbol, option_sets[o], LINREG_OPTIONS, t_talib, (int) stocks[s].len);
        }
    }
}
