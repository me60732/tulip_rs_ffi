// aroon (Aroon) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a multi-input indicator with options,
// returning two outputs: aroon_down and aroon_up.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    double period;
} AroonCtx;

static void bench_aroon(void *ctx_) {
    AroonCtx *ctx = ctx_;
    const double *inputs[AROON_INPUTS] = {ctx->stock->high, ctx->stock->low};
    double opts[AROON_OPTIONS] = {ctx->period};
    struct CIndicatorResult r = aroon_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] aroon_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    aroon_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_aroon.rs's bench_c_aroon exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_aroon(void *ctx_) {
    AroonCtx *ctx = ctx_;
    double options[AROON_OPTIONS] = {ctx->period};
    int start_index = ti_aroon_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_aroon_start returned negative index\n"); exit(1); }
    int output_len = (int) ctx->stock->len - start_index;
    double *aroon_down = malloc(sizeof(double) * (size_t) output_len);
    double *aroon_up = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[AROON_INPUTS] = {ctx->stock->high, ctx->stock->low};
    double *outputs[2] = {aroon_down, aroon_up};
    int ret = ti_aroon((int) ctx->stock->len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_aroon returned %d\n", ret); exit(1); }
    free(aroon_down);
    free(aroon_up);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib". Mirrors
// tulip_test/benches/benchmark_aroon.rs's bench_talib_aroon exactly.
// ---------------------------------------------------------------------------

static void bench_talib_aroon(void *ctx_) {
    AroonCtx *ctx = ctx_;
    int size = (int) ctx->stock->len;
    int start_index = TA_AROON_Lookback((int) ctx->period);
    if (start_index < 0) { fprintf(stderr, "[error] TA_AROON_Lookback returned negative index\n"); exit(1); }
    int output_len = size - start_index;
    double *aroon_down = malloc(sizeof(double) * (size_t) output_len);
    double *aroon_up = malloc(sizeof(double) * (size_t) output_len);
    int out_begin = 0, out_nb_element = 0;
    TA_RetCode ret = TA_AROON(0, size - 1,
                              ctx->stock->high, ctx->stock->low,
                              (int) ctx->period,
                              &out_begin, &out_nb_element, aroon_down, aroon_up);
    if (ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_AROON returned %d\n", (int) ret); exit(1); }
    free(aroon_down);
    free(aroon_up);
}

static void run_aroon(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][AROON_OPTIONS] = {{25.0}, {35.0}, {50.0}, {100.0}};
    printf("\n--- AROON ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            AroonCtx ctx = {
                .stock = &stocks[s],
                .period = option_sets[o][0],
            };

            TimingResult t = time_fn(bench_aroon, &ctx, number, repeat, warmup);
            log_and_print("aroon", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 1, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_aroon, &ctx, number, repeat, warmup);
            log_and_print("aroon", "C_tulip", stocks[s].symbol, option_sets[o], 1, t_c, (int) stocks[s].len);

            TimingResult t_talib = time_fn(bench_talib_aroon, &ctx, number, repeat, warmup);
            log_and_print("aroon", "talib", stocks[s].symbol, option_sets[o], 1, t_talib, (int) stocks[s].len);
        }
    }
}
