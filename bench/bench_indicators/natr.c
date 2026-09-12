// natr (Normalized Average True Range) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a multi-input indicator with options and
// optional outputs requested.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    double period;
} NatrCtx;

static void bench_natr(void *ctx_) {
    NatrCtx *ctx = ctx_;
    const double *inputs[NATR_INPUTS] = {ctx->stock->high, ctx->stock->low, ctx->stock->close};
    double opts[NATR_OPTIONS] = {ctx->period};
    struct CIndicatorResult r = natr_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] natr_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    natr_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_natr.rs's bench_c_natr exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_natr(void *ctx_) {
    NatrCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    double options[1] = {ctx->period};
    int start_index = ti_natr_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_natr_start returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[NATR_INPUTS] = {ctx->stock->high, ctx->stock->low, ctx->stock->close};
    double *outputs[1] = {output};
    int ret = ti_natr((int) len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_natr returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib". Mirrors
// tulip_test/benches/benchmark_natr.rs's bench_talib_natr exactly.
// ---------------------------------------------------------------------------

static void bench_talib_natr(void *ctx_) {
    NatrCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    double options[1] = {ctx->period};
    int start_index = TA_NATR_Lookback((int) options[0]);
    if (start_index < 0) { fprintf(stderr, "[error] TA_NATR_Lookback returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    int out_begin = 0, out_nb_element = 0;
    TA_RetCode ret =
        TA_NATR(0, (int) len - 1, ctx->stock->high, ctx->stock->low, ctx->stock->close,
                (int) options[0], &out_begin, &out_nb_element, output);
    if (ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_NATR returned %d\n", (int) ret); exit(1); }
    free(output);
}

static void run_natr(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][NATR_OPTIONS] = {{5.0}, {14.0}, {20.0}, {30.0}};
    printf("\n--- NATR ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            NatrCtx ctx = {
                .stock = &stocks[s],
                .period = option_sets[o][0],
            };

            TimingResult t = time_fn(bench_natr, &ctx, number, repeat, warmup);
            log_and_print("natr", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], NATR_OPTIONS, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_natr, &ctx, number, repeat, warmup);
            log_and_print("natr", "C_tulip", stocks[s].symbol, option_sets[o], NATR_OPTIONS, t_c, (int) stocks[s].len);

            TimingResult t_talib = time_fn(bench_talib_natr, &ctx, number, repeat, warmup);
            log_and_print("natr", "talib", stocks[s].symbol, option_sets[o], NATR_OPTIONS, t_talib, (int) stocks[s].len);
        }
    }
}
