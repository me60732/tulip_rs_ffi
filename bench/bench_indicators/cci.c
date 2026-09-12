// cci (Commodity Channel Index) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a multi-input indicator with options and
// optional outputs requested.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    double period;
} CciCtx;

static void bench_cci(void *ctx_) {
    CciCtx *ctx = ctx_;
    const double *inputs[CCI_INPUTS] = {ctx->stock->high, ctx->stock->low, ctx->stock->close};
    double opts[CCI_OPTIONS] = {ctx->period};
    struct CIndicatorResult r = cci_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] cci_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    cci_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_cci.rs's bench_c_cci exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_cci(void *ctx_) {
    CciCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    double options[1] = {ctx->period};
    int start_index = ti_cci_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_cci_start returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[3] = {ctx->stock->high, ctx->stock->low, ctx->stock->close};
    double *outputs[1] = {output};
    int ret = ti_cci((int) len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_cci returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib". Mirrors
// tulip_test/benches/benchmark_cci.rs's bench_talib_cci exactly.
// ---------------------------------------------------------------------------

static void bench_talib_cci(void *ctx_) {
    CciCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    int start_index = TA_CCI_Lookback((int) ctx->period);
    if (start_index < 0) { fprintf(stderr, "[error] TA_CCI_Lookback returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    int out_begin = 0, out_nb_element = 0;
    TA_RetCode ret = TA_CCI(0, (int) len - 1,
                            ctx->stock->high, ctx->stock->low, ctx->stock->close,
                            (int) ctx->period,
                            &out_begin, &out_nb_element, output);
    if (ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_CCI returned %d\n", (int) ret); exit(1); }
    free(output);
}

static void run_cci(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][CCI_OPTIONS] = {{20.0}, {25.0}, {30.0}, {50.0}};
    printf("\n--- CCI ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            CciCtx ctx = {
                .stock = &stocks[s],
                .period = option_sets[o][0],
            };

            TimingResult t = time_fn(bench_cci, &ctx, number, repeat, warmup);
            log_and_print("cci", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], CCI_OPTIONS, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_cci, &ctx, number, repeat, warmup);
            log_and_print("cci", "C_tulip", stocks[s].symbol, option_sets[o], CCI_OPTIONS, t_c, (int) stocks[s].len);

            TimingResult t_talib = time_fn(bench_talib_cci, &ctx, number, repeat, warmup);
            log_and_print("cci", "talib", stocks[s].symbol, option_sets[o], CCI_OPTIONS, t_talib, (int) stocks[s].len);
        }
    }
}
