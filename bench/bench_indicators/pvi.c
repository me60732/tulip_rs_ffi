// pvi (Positive Volume Index) -- tulip_rs_ffi extern "C" API.
//
// This is one of the three "template" indicators (alongside macd.c and
// stoch.c) that establish the pattern every other file under bench_indicators/
// follows. See ../README.md "Adding a new indicator to this harness".

#include "tulip_rs_ffi.h"
#include "../bench_common.h"

typedef struct {
    const Stock *stock;
    const double *inputs_buf; // close ++ volume, each stock->len long
} PviCtx;

static void bench_pvi(void *ctx_) {
    PviCtx *ctx = ctx_;
    const double *inputs[PVI_INPUTS] = {ctx->stock->close, ctx->stock->volume};
    struct CIndicatorResult r = pvi_indicator(inputs, ctx->stock->len, NULL, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] pvi_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    pvi_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_pvi.rs's bench_c_pvi exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_pvi(void *ctx_) {
    PviCtx *ctx = ctx_;
    double options[PVI_OPTIONS] = {};
    int start_index = ti_pvi_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_pvi_start returned negative index\n"); exit(1); }
    int output_len = (int) ctx->stock->len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[PVI_INPUTS] = {ctx->stock->close, ctx->stock->volume};
    double *outputs[1] = {output};
    int ret = ti_pvi((int) ctx->stock->len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_pvi returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib".
// Note: No TA-Lib equivalent for PVI.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// SIMD comparisons -- optional outputs always NULL (off).
// pvi_simd_by_assets() runs one option set across 4 assets in a single call.
// Note: PVI has no options (PVI_OPTIONS=0), so there is NO simd_by_options variant.
// ---------------------------------------------------------------------------

typedef struct {
    const Stock *stocks;   // array of 4 stocks
    size_t data_len;       // bars per asset (shared across the 4)
} PviSimdCtx;

static void bench_pvi_simd_assets(void *ctx_) {
    PviSimdCtx *ctx = ctx_;
    // Each asset has PVI_INPUTS=2 input pointers (close, volume)
    const double *inputs_per_asset[4][PVI_INPUTS] = {
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
    // PVI has no options (PVI_OPTIONS=0); mirroring scalar call which passes NULL.
    struct CSimdResult r = pvi_simd_by_assets((const double *const *const *)inputs, 4, ctx->data_len, NULL, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] pvi_simd_by_assets failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) pvi_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

static void run_pvi(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    printf("\n--- PVI ---\n");
    for (int s = 0; s < num_stocks; s++) {
        size_t len = stocks[s].len;
        double *inputs_buf = malloc(sizeof(double) * len * 2);
        memcpy(inputs_buf, stocks[s].close, sizeof(double) * len);
        memcpy(inputs_buf + len, stocks[s].volume, sizeof(double) * len);

        PviCtx ctx = {
            .stock = &stocks[s],
            .inputs_buf = inputs_buf,
        };

        TimingResult t = time_fn(bench_pvi, &ctx, number, repeat, warmup);
        log_and_print("pvi", "tulip_rs_ffi_c", stocks[s].symbol, NULL, 0, t, (int) stocks[s].len);

        TimingResult t_c = time_fn(bench_tulipc_pvi, &ctx, number, repeat, warmup);
        log_and_print("pvi", "C_tulip", stocks[s].symbol, NULL, 0, t_c, (int) stocks[s].len);

        // No TA-Lib comparison available

        free(inputs_buf);
    }

    // ---- SIMD runs (optional outputs off: NULL, 0) ----
    if (num_stocks >= 4) {
        size_t dlen = stocks[0].len;
        for (int i = 1; i < 4; i++) if (stocks[i].len < dlen) dlen = stocks[i].len;

        PviSimdCtx ctx = { .stocks = stocks, .data_len = dlen };
        TimingResult t = time_fn(bench_pvi_simd_assets, &ctx, number, repeat, warmup);
        log_and_print("pvi", "tulip_rs_ffi_c_simd_by_assets", "All", NULL, 0, t, (int) dlen);
    }
}
