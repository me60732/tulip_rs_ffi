// dema (Double Exponential Moving Average) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a single-input indicator with options and
// optional outputs requested.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    double period;
} DemaCtx;

static void bench_dema(void *ctx_) {
    DemaCtx *ctx = ctx_;
    const double *inputs[DEMA_INPUTS] = {ctx->stock->close};
    double opts[DEMA_OPTIONS] = {ctx->period};
    struct CIndicatorResult r = dema_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] dema_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    dema_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_dema.rs's bench_c_dema exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_dema(void *ctx_) {
    DemaCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    double options[1] = {ctx->period};
    int start_index = ti_dema_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_dema_start returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[1] = {ctx->stock->close};
    double *outputs[1] = {output};
    int ret = ti_dema((int) len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_dema returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib". Mirrors
// tulip_test/benches/benchmark_dema.rs's bench_talib_dema exactly.
// ---------------------------------------------------------------------------

static void bench_talib_dema(void *ctx_) {
    DemaCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    int start_index = TA_DEMA_Lookback((int) ctx->period);
    if (start_index < 0) { fprintf(stderr, "[error] TA_DEMA_Lookback returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    int out_begin = 0, out_nb_element = 0;
    TA_RetCode ret = TA_DEMA(0, (int) len - 1,
                             ctx->stock->close,
                             (int) ctx->period,
                             &out_begin, &out_nb_element, output);
    if (ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_DEMA returned %d\n", (int) ret); exit(1); }
    free(output);
}

static void run_dema(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][DEMA_OPTIONS] = {{5.0}, {14.0}, {20.0}, {50.0}};
    printf("\n--- DEMA ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            DemaCtx ctx = {
                .stock = &stocks[s],
                .period = option_sets[o][0],
            };

            TimingResult t = time_fn(bench_dema, &ctx, number, repeat, warmup);
            log_and_print("dema", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], DEMA_OPTIONS, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_dema, &ctx, number, repeat, warmup);
            log_and_print("dema", "C_tulip", stocks[s].symbol, option_sets[o], DEMA_OPTIONS, t_c, (int) stocks[s].len);

            TimingResult t_talib = time_fn(bench_talib_dema, &ctx, number, repeat, warmup);
            log_and_print("dema", "talib", stocks[s].symbol, option_sets[o], DEMA_OPTIONS, t_talib, (int) stocks[s].len);
        }
    }
}
