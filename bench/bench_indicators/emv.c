// emv (Ease of Movement) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a multi-input indicator with no options
// and optional_outputs requested.
#include "tulip_rs_ffi.h"
#include "../bench_common.h"

typedef struct {
    const Stock *stock;
} EmvCtx;

static void bench_emv(void *ctx_) {
    EmvCtx *ctx = ctx_;
    const double *inputs[EMV_INPUTS] = {ctx->stock->high, ctx->stock->low, ctx->stock->volume};
    struct CIndicatorResult r = emv_indicator(inputs, ctx->stock->len, NULL, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] emv_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    emv_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_emv.rs's bench_c_emv exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_emv(void *ctx_) {
    EmvCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    int start_index = ti_emv_start(NULL);
    if (start_index < 0) { fprintf(stderr, "[error] ti_emv_start returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[EMV_INPUTS] = {ctx->stock->high, ctx->stock->low, ctx->stock->volume};
    double *outputs[1] = {output};
    int ret = ti_emv((int) len, inputs, NULL, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_emv returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- not available. TA-Lib has no EMV (Ease of Movement)
// function in ta_func.h, so this comparison is omitted.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// SIMD comparisons -- optional outputs always NULL (off).
// emv_simd_by_assets() runs one option set across 4 assets in a single call.
// Note: EMV has no options (EMV_OPTIONS=0), so there is NO simd_by_options variant.
// ---------------------------------------------------------------------------

typedef struct {
    const Stock *stocks;   // array of 4 stocks
    size_t data_len;       // bars per asset (shared across the 4)
} EmvSimdCtx;

static void bench_emv_simd_assets(void *ctx_) {
    EmvSimdCtx *ctx = ctx_;
    // Each asset has EMV_INPUTS=3 input pointers (high, low, volume)
    const double *inputs_per_asset[4][EMV_INPUTS] = {
        {ctx->stocks[0].high, ctx->stocks[0].low, ctx->stocks[0].volume},
        {ctx->stocks[1].high, ctx->stocks[1].low, ctx->stocks[1].volume},
        {ctx->stocks[2].high, ctx->stocks[2].low, ctx->stocks[2].volume},
        {ctx->stocks[3].high, ctx->stocks[3].low, ctx->stocks[3].volume},
    };
    // inputs is an array of num_assets pointers to input arrays
    const double *inputs[4];
    for (int i = 0; i < 4; i++) {
        inputs[i] = (const double *)&inputs_per_asset[i][0];
    }
    struct CSimdResult r = emv_simd_by_assets((const double *const *const *)inputs, 4, ctx->data_len, NULL, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] emv_simd_by_assets failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) emv_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

static void run_emv(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][EMV_OPTIONS] = {{}};
    printf("\n--- EMV ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 1; o++) {
            EmvCtx ctx = {.stock = &stocks[s]};

            TimingResult t = time_fn(bench_emv, &ctx, number, repeat, warmup);
            log_and_print("emv", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 0, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_emv, &ctx, number, repeat, warmup);
            log_and_print("emv", "C_tulip", stocks[s].symbol, option_sets[o], 0, t_c, (int) stocks[s].len);

            // No TA-Lib comparison available
        }
    }

    // ---- SIMD runs (optional outputs off: NULL, 0) ----
    if (num_stocks >= 4) {
        size_t dlen = stocks[0].len;
        for (int i = 1; i < 4; i++) if (stocks[i].len < dlen) dlen = stocks[i].len;

        EmvSimdCtx ctx = { .stocks = stocks, .data_len = dlen };
        TimingResult t = time_fn(bench_emv_simd_assets, &ctx, number, repeat, warmup);
        log_and_print("emv", "tulip_rs_ffi_c_simd_by_assets", "All", NULL, 0, t, (int) dlen);
    }
}
