// wad (Williams Accumulation/Distribution) -- tulip_rs_ffi extern "C" API.
//
// A multi-input indicator with no options and no optional_outputs.

#include "tulip_rs_ffi.h"
#include "../bench_common.h"

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
// SIMD comparisons -- optional outputs always NULL (off).
// wad_simd_by_assets() runs one option set across 4 assets in a single call.
// Note: WAD has no options (WAD_OPTIONS=0), so there is NO simd_by_options variant.
// ---------------------------------------------------------------------------

typedef struct {
    const Stock *stocks;   // array of 4 stocks
    size_t data_len;       // bars per asset (shared across the 4)
} WadSimdCtx;

static void bench_wad_simd_assets(void *ctx_) {
    WadSimdCtx *ctx = ctx_;
    // Each asset has WAD_INPUTS=3 input pointers (high, low, close)
    const double *inputs_per_asset[4][WAD_INPUTS] = {
        {ctx->stocks[0].high, ctx->stocks[0].low, ctx->stocks[0].close},
        {ctx->stocks[1].high, ctx->stocks[1].low, ctx->stocks[1].close},
        {ctx->stocks[2].high, ctx->stocks[2].low, ctx->stocks[2].close},
        {ctx->stocks[3].high, ctx->stocks[3].low, ctx->stocks[3].close},
    };
    // inputs is an array of num_assets pointers to input arrays
    const double *inputs[4];
    for (int i = 0; i < 4; i++) {
        inputs[i] = (const double *)&inputs_per_asset[i][0];
    }
    // WAD has no options (WAD_OPTIONS=0); mirroring scalar call which passes NULL.
    struct CSimdResult r = wad_simd_by_assets((const double *const *const *)inputs, 4, ctx->data_len, NULL, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] wad_simd_by_assets failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) wad_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

static void run_wad(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][WAD_OPTIONS] = {{}};
    printf("\n--- WAD ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            WadCtx ctx = {
                .stock = &stocks[s],
            };

            TimingResult t = time_fn(bench_wad, &ctx, number, repeat, warmup);
            log_and_print("wad", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 0, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_wad, &ctx, number, repeat, warmup);
            log_and_print("wad", "C_tulip", stocks[s].symbol, option_sets[o], 0, t_c, (int) stocks[s].len);
        }
    }

    // ---- SIMD runs (optional outputs off: NULL, 0) ----
    if (num_stocks >= 4) {
        size_t dlen = stocks[0].len;
        for (int i = 1; i < 4; i++) if (stocks[i].len < dlen) dlen = stocks[i].len;

        WadSimdCtx ctx = { .stocks = stocks, .data_len = dlen };
        TimingResult t = time_fn(bench_wad_simd_assets, &ctx, number, repeat, warmup);
        log_and_print("wad", "tulip_rs_ffi_c_simd_by_assets", "All", NULL, 0, t, (int) dlen);
    }
}
