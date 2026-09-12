// di (Directional Indicator) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a multi-input indicator with options and
// multiple mandatory outputs (+DI, -DI) plus optional outputs (ATR, TR).
// The pointer array is built inside the timed region.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    double period;
} DiCtx;

static void bench_di(void *ctx_) {
    DiCtx *ctx = ctx_;
    const double *inputs[DI_INPUTS] = {ctx->stock->high, ctx->stock->low, ctx->stock->close};
    double opts[DI_OPTIONS] = {ctx->period};
    struct CIndicatorResult r = di_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] di_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    di_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_di.rs's bench_c_di exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_di(void *ctx_) {
    DiCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    double options[1] = {ctx->period};
    int start_index = ti_di_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_di_start returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    double *output_plus_di = malloc(sizeof(double) * (size_t) output_len);
    double *output_minus_di = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[3] = {ctx->stock->high, ctx->stock->low, ctx->stock->close};
    double *outputs[2] = {output_plus_di, output_minus_di};
    int ret = ti_di((int) len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_di returned %d\n", ret); exit(1); }
    free(output_plus_di);
    free(output_minus_di);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib".
//
// TA-Lib exposes +DI/-DI as two separate functions (TA_PLUS_DI/TA_MINUS_DI),
// but our di() computes both in a single call, so for an apples-to-apples
// comparison this closure calls both TA-Lib functions back-to-back and times
// them together as one "talib" sample.
// ---------------------------------------------------------------------------

static void bench_talib_di(void *ctx_) {
    DiCtx *ctx = ctx_;
    size_t len = ctx->stock->len;

    int plus_start = TA_PLUS_DI_Lookback((int) ctx->period);
    if (plus_start < 0) { fprintf(stderr, "[error] TA_PLUS_DI_Lookback returned negative index\n"); exit(1); }
    int plus_len = (int) len - plus_start;
    double *plus_output = malloc(sizeof(double) * (size_t) plus_len);
    int plus_begin = 0, plus_nb_element = 0;
    TA_RetCode plus_ret = TA_PLUS_DI(0, (int) len - 1,
                                      ctx->stock->high, ctx->stock->low, ctx->stock->close,
                                      (int) ctx->period,
                                      &plus_begin, &plus_nb_element, plus_output);
    if (plus_ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_PLUS_DI returned %d\n", (int) plus_ret); exit(1); }

    int minus_start = TA_MINUS_DI_Lookback((int) ctx->period);
    if (minus_start < 0) { fprintf(stderr, "[error] TA_MINUS_DI_Lookback returned negative index\n"); exit(1); }
    int minus_len = (int) len - minus_start;
    double *minus_output = malloc(sizeof(double) * (size_t) minus_len);
    int minus_begin = 0, minus_nb_element = 0;
    TA_RetCode minus_ret = TA_MINUS_DI(0, (int) len - 1,
                                        ctx->stock->high, ctx->stock->low, ctx->stock->close,
                                        (int) ctx->period,
                                        &minus_begin, &minus_nb_element, minus_output);
    if (minus_ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_MINUS_DI returned %d\n", (int) minus_ret); exit(1); }

    free(plus_output);
    free(minus_output);
}

static void run_di(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][DI_OPTIONS] = {{5.0}, {14.0}, {20.0}, {30.0}};
    printf("\n--- DI ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            DiCtx ctx = {
                .stock = &stocks[s],
                .period = option_sets[o][0],
            };

            TimingResult t = time_fn(bench_di, &ctx, number, repeat, warmup);
            log_and_print("di", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], DI_OPTIONS, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_di, &ctx, number, repeat, warmup);
            log_and_print("di", "C_tulip", stocks[s].symbol, option_sets[o], DI_OPTIONS, t_c, (int) stocks[s].len);

            // TA-Lib has no combined +DI/-DI function, so bench_talib_di calls
            // both TA_PLUS_DI and TA_MINUS_DI and times them together, matching
            // the fact that our di() computes both outputs in a single call.
            TimingResult t_talib = time_fn(bench_talib_di, &ctx, number, repeat, warmup);
            log_and_print("di", "talib", stocks[s].symbol, option_sets[o], DI_OPTIONS, t_talib, (int) stocks[s].len);
        }
    }
}
