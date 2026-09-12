// nvi (Negative Volume Index) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a two-input indicator with no options and
// no optional_outputs requested.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
} NviCtx;

static void bench_nvi(void *ctx_) {
    NviCtx *ctx = ctx_;
    const double *inputs[NVI_INPUTS] = {ctx->stock->close, ctx->stock->volume};
    struct CIndicatorResult r = nvi_indicator(inputs, ctx->stock->len, NULL, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] nvi_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    nvi_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_nvi.rs's bench_c_nvi exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_nvi(void *ctx_) {
    NviCtx *ctx = ctx_;
    double options[0] = {};
    int start_index = ti_nvi_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_nvi_start returned negative index\n"); exit(1); }
    int output_len = (int) ctx->stock->len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[NVI_INPUTS] = {ctx->stock->close, ctx->stock->volume};
    double *outputs[1] = {output};
    int ret = ti_nvi((int) ctx->stock->len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_nvi returned %d\n", ret); exit(1); }
    free(output);
}

static void run_nvi(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    printf("\n--- NVI ---\n");
    for (int s = 0; s < num_stocks; s++) {
        NviCtx ctx = {.stock = &stocks[s]};

        TimingResult t = time_fn(bench_nvi, &ctx, number, repeat, warmup);
        log_and_print("nvi", "tulip_rs_ffi_c", stocks[s].symbol, NULL, 0, t, (int) stocks[s].len);

        TimingResult t_c = time_fn(bench_tulipc_nvi, &ctx, number, repeat, warmup);
        log_and_print("nvi", "C_tulip", stocks[s].symbol, NULL, 0, t_c, (int) stocks[s].len);
    }
}
