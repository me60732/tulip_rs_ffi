// mom (Momentum) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a single-input indicator with one option.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    double period;
} MomCtx;

static void bench_mom(void *ctx_) {
    MomCtx *ctx = ctx_;
    const double *inputs[MOM_INPUTS] = {ctx->stock->close};
    double opts[MOM_OPTIONS] = {ctx->period};
    struct CIndicatorResult r = mom_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] mom_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    mom_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_mom.rs's bench_c_mom exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_mom(void *ctx_) {
    MomCtx *ctx = ctx_;
    double options[1] = {ctx->period};
    int start_index = ti_mom_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_mom_start returned negative index\n"); exit(1); }
    int output_len = (int) ctx->stock->len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[1] = {ctx->stock->close};
    double *outputs[1] = {output};
    int ret = ti_mom((int) ctx->stock->len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_mom returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib". Mirrors C_tulip pattern.
// TA_MOM_Lookback(optInTimePeriod) returns int, uses real (close) array.
// ---------------------------------------------------------------------------

static void bench_talib_mom(void *ctx_) {
    MomCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    int start_index = TA_MOM_Lookback((int) ctx->period);
    if (start_index < 0) { fprintf(stderr, "[error] TA_MOM_Lookback returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    int out_begin = 0, out_nb_element = 0;
    TA_RetCode ret = TA_MOM(0, (int) len - 1,
                            ctx->stock->close,
                            (int) ctx->period,
                            &out_begin, &out_nb_element, output);
    if (ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_MOM returned %d\n", (int) ret); exit(1); }
    free(output);
}

static void run_mom(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][MOM_OPTIONS] = {{25.0}, {30.0}, {50.0}, {100.0}};
    printf("\n--- MOM ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            MomCtx ctx = {
                .stock = &stocks[s],
                .period = option_sets[o][0],
            };

            TimingResult t = time_fn(bench_mom, &ctx, number, repeat, warmup);
            log_and_print("mom", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 1, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_mom, &ctx, number, repeat, warmup);
            log_and_print("mom", "C_tulip", stocks[s].symbol, option_sets[o], 1, t_c, (int) stocks[s].len);

            TimingResult t_talib = time_fn(bench_talib_mom, &ctx, number, repeat, warmup);
            log_and_print("mom", "talib", stocks[s].symbol, option_sets[o], 1, t_talib, (int) stocks[s].len);
        }
    }
}
