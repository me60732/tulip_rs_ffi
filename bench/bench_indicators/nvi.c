// nvi (Negative Volume Index) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a two-input indicator with no options and
// no optional_outputs requested.

#include "tulip_rs_ffi.h"
#include "../bench_common.h"

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

// ---------------------------------------------------------------------------
// SIMD comparisons -- optional outputs always NULL (off).
// nvi_simd_by_assets() runs one option set across 4 assets in a single call.
// Note: NVI has no options (NVI_OPTIONS=0), so there is NO simd_by_options variant.
// ---------------------------------------------------------------------------

typedef struct {
    const Stock *stocks;   // array of 4 stocks
    size_t data_len;       // bars per asset (shared across the 4)
} NviSimdCtx;

static void bench_nvi_simd_assets(void *ctx_) {
    NviSimdCtx *ctx = ctx_;
    // Each asset has NVI_INPUTS=2 input pointers (close, volume)
    const double *inputs_per_asset[4][NVI_INPUTS] = {
        {ctx->stocks[0].close, ctx->stocks[0].volume},
        {ctx->stocks[1].close, ctx->stocks[1].volume},
        {ctx->stocks[2].close, ctx->stocks[2].volume},
        {ctx->stocks[3].close, ctx->stocks[3].volume},
    };
    // inputs is an array of num_assets pointers to input arrays
    const double *inputs[4];
    for (int i = 0; i < 4; i++) {
        inputs[i] = (const double *)&inputs_per_asset[i][0];
    }
    // NVI has no options (NVI_OPTIONS=0); mirroring scalar call which passes NULL.
    struct CSimdResult r = nvi_simd_by_assets((const double *const *const *)inputs, 4, ctx->data_len, NULL, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] nvi_simd_by_assets failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) nvi_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

static void run_nvi(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][NVI_OPTIONS] = {{}};
    printf("\n--- NVI ---\n");
    for (int s = 0; s < num_stocks; s++) {
        NviCtx ctx = {.stock = &stocks[s]};

        TimingResult t = time_fn(bench_nvi, &ctx, number, repeat, warmup);
        log_and_print("nvi", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[0], 0, t, (int) stocks[s].len);

        TimingResult t_c = time_fn(bench_tulipc_nvi, &ctx, number, repeat, warmup);
        log_and_print("nvi", "C_tulip", stocks[s].symbol, option_sets[0], 0, t_c, (int) stocks[s].len);
    }

    // ---- SIMD runs (optional outputs off: NULL, 0) ----
    if (num_stocks >= 4) {
        size_t dlen = stocks[0].len;
        for (int i = 1; i < 4; i++) if (stocks[i].len < dlen) dlen = stocks[i].len;

        NviSimdCtx ctx = { .stocks = stocks, .data_len = dlen };
        TimingResult t = time_fn(bench_nvi_simd_assets, &ctx, number, repeat, warmup);
        log_and_print("nvi", "tulip_rs_ffi_c_simd_by_assets", "All", NULL, 0, t, (int) dlen);
    }
}
