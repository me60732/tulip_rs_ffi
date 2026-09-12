// max (maximum) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating an indicator with a single input and options,
// no optional outputs requested.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    double period;
} MaxCtx;

static void bench_max(void *ctx_) {
    MaxCtx *ctx = ctx_;
    const double *inputs[MAX_INPUTS] = {ctx->stock->close};
    double opts[MAX_OPTIONS] = {ctx->period};
    // max has no optional outputs
    struct CIndicatorResult r = max_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] max_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    max_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_max.rs's bench_c_max exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_max(void *ctx_) {
    MaxCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    double options[MAX_OPTIONS] = {ctx->period};
    int start_index = ti_max_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_max_start returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[MAX_INPUTS] = {ctx->stock->close};
    double *outputs[1] = {output};
    int ret = ti_max((int) len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_max returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib". Mirrors
// tulip_test/benches/benchmark_max.rs's bench_talib_max exactly.
// ---------------------------------------------------------------------------

static void bench_talib_max(void *ctx_) {
    MaxCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    int start_index = TA_MAX_Lookback((int) ctx->period);
    if (start_index < 0) { fprintf(stderr, "[error] TA_MAX_Lookback returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    int out_begin = 0, out_nb_element = 0;
    TA_RetCode ret = TA_MAX(0, (int) len - 1,
                            ctx->stock->close,
                            (int) ctx->period,
                            &out_begin, &out_nb_element, output);
    if (ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_MAX returned %d\n", (int) ret); exit(1); }
    free(output);
}

static void run_max(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][MAX_OPTIONS] = {{5.0}, {14.0}, {20.0}, {50.0}};
    printf("\n--- MAX ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            MaxCtx ctx = {
                .stock = &stocks[s],
                .period = option_sets[o][0],
            };

            TimingResult t = time_fn(bench_max, &ctx, number, repeat, warmup);
            log_and_print("max", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], MAX_OPTIONS, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_max, &ctx, number, repeat, warmup);
            log_and_print("max", "C_tulip", stocks[s].symbol, option_sets[o], MAX_OPTIONS, t_c, (int) stocks[s].len);

            TimingResult t_talib = time_fn(bench_talib_max, &ctx, number, repeat, warmup);
            log_and_print("max", "talib", stocks[s].symbol, option_sets[o], MAX_OPTIONS, t_talib, (int) stocks[s].len);
        }
    }
}
