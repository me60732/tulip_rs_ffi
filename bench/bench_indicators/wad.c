// wad (Williams Accumulation/Distribution) -- tulip_rs_ffi extern "C" API.
//
// A multi-input indicator with no options and no optional_outputs.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
} WadCtx;

static void bench_wad(void *ctx_) {
    WadCtx *ctx = ctx_;
    const double *inputs[WAD_INPUTS] = {ctx->stock->high, ctx->stock->low, ctx->stock->close};
    double opts[WAD_OPTIONS] = {};
    struct CIndicatorResult r = wad_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] wad_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    wad_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_wad.rs's bench_c_wad exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_wad(void *ctx_) {
    WadCtx *ctx = ctx_;
    double options[0] = {};
    int start_index = ti_wad_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_wad_start returned negative index\n"); exit(1); }
    int output_len = (int) ctx->stock->len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[3] = {ctx->stock->high, ctx->stock->low, ctx->stock->close};
    double *outputs[1] = {output};
    int ret = ti_wad((int) ctx->stock->len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_wad returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// No TA-Lib comparison: TA-Lib has no Williams A/D equivalent.
// ---------------------------------------------------------------------------

static void run_wad(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    printf("\n--- WAD ---\n");
    for (int s = 0; s < num_stocks; s++) {
        WadCtx ctx = {
            .stock = &stocks[s],
        };

        TimingResult t = time_fn(bench_wad, &ctx, number, repeat, warmup);
        log_and_print("wad", "tulip_rs_ffi_c", stocks[s].symbol, (double[]){}, 0, t, (int) stocks[s].len);

        TimingResult t_c = time_fn(bench_tulipc_wad, &ctx, number, repeat, warmup);
        log_and_print("wad", "C_tulip", stocks[s].symbol, (double[]){}, 0, t_c, (int) stocks[s].len);
    }
}
