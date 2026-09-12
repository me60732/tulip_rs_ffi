// msw (Mesa Sine Wave) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a single-input indicator with options and
// no optional_outputs requested.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    double period;
} MswCtx;

static void bench_msw(void *ctx_) {
    MswCtx *ctx = ctx_;
    const double *inputs[MSW_INPUTS] = {ctx->stock->close};
    double opts[MSW_OPTIONS] = {ctx->period};
    struct CIndicatorResult r = msw_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] msw_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    msw_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_msw.rs's bench_c_msw exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_msw(void *ctx_) {
    MswCtx *ctx = ctx_;
    double options[1] = {ctx->period};
    int start_index = ti_msw_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_msw_start returned negative index\n"); exit(1); }
    int output_len = (int) ctx->stock->len - start_index;
    double *sine = malloc(sizeof(double) * (size_t) output_len);
    double *lead = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[MSW_INPUTS] = {ctx->stock->close};
    double *outputs[2] = {sine, lead};
    int ret = ti_msw((int) ctx->stock->len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_msw returned %d\n", ret); exit(1); }
    free(sine);
    free(lead);
}

static void run_msw(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][MSW_OPTIONS] = {{5.0}, {8.0}, {14.0}, {20.0}};
    printf("\n--- MSW ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            MswCtx ctx = {.stock = &stocks[s], .period = option_sets[o][0]};

            TimingResult t = time_fn(bench_msw, &ctx, number, repeat, warmup);
            log_and_print("msw", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], MSW_OPTIONS, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_msw, &ctx, number, repeat, warmup);
            log_and_print("msw", "C_tulip", stocks[s].symbol, option_sets[o], MSW_OPTIONS, t_c, (int) stocks[s].len);
        }
    }
}
