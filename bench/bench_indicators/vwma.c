// vwma (Volume-Weighted Moving Average) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a multi-input indicator with one option and
// no optional_outputs.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    double period;
} VwmaCtx;

static void bench_vwma(void *ctx_) {
    VwmaCtx *ctx = ctx_;
    const double *inputs[VWMA_INPUTS] = {ctx->stock->close, ctx->stock->volume};
    double opts[VWMA_OPTIONS] = {ctx->period};
    struct CIndicatorResult r = vwma_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] vwma_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    vwma_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_vwma.rs's bench_c_vwma exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_vwma(void *ctx_) {
    VwmaCtx *ctx = ctx_;
    double options[VWMA_OPTIONS] = {ctx->period};
    int start_index = ti_vwma_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_vwma_start returned negative index\n"); exit(1); }
    int output_len = (int) ctx->stock->len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[VWMA_INPUTS] = {ctx->stock->close, ctx->stock->volume};
    double *outputs[1] = {output};
    int ret = ti_vwma((int) ctx->stock->len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_vwma returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- not available. The vendored Tulip Indicators C library
// has no TA_Lib equivalent for vwma.
// ---------------------------------------------------------------------------

static void run_vwma(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][VWMA_OPTIONS] = {{14.0}, {20.0}, {25.0}, {30.0}};
    printf("\n--- VWMA ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            VwmaCtx ctx = {.stock = &stocks[s], .period = option_sets[o][0]};

            TimingResult t = time_fn(bench_vwma, &ctx, number, repeat, warmup);
            log_and_print("vwma", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], VWMA_OPTIONS, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_vwma, &ctx, number, repeat, warmup);
            log_and_print("vwma", "C_tulip", stocks[s].symbol, option_sets[o], VWMA_OPTIONS, t_c, (int) stocks[s].len);

            // No TA-Lib comparison available
        }
    }
}
