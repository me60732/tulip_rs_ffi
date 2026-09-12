// ao (Awesome Oscillator) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a multi-input indicator with no options,
// with optional_outputs requested.

#include "tulip_rs_ffi.h"
#include "../bench_common.h"

#define AO_INPUTS 2
#define AO_OPTIONS 0

typedef struct {
    const Stock *stock;
} AoCtx;

static void bench_ao(void *ctx_) {
    AoCtx *ctx = ctx_;
    const double *inputs[AO_INPUTS] = {ctx->stock->high, ctx->stock->low};
    struct CIndicatorResult r = ao_indicator(inputs, ctx->stock->len, NULL, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] ao_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    ao_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_ao.rs's bench_c_ao exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_ao(void *ctx_) {
    AoCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    int start_index = ti_ao_start(NULL);
    if (start_index < 0) { fprintf(stderr, "[error] ti_ao_start returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[AO_INPUTS] = {ctx->stock->high, ctx->stock->low};
    double *outputs[1] = {output};
    int ret = ti_ao((int) len, inputs, NULL, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_ao returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib".
// NOTE: TA-Lib does not provide an equivalent for ao.
// ---------------------------------------------------------------------------

// SIMD comparisons -- optional outputs always NULL (off).
// ao_simd_by_assets() runs one option set across 4 assets in a single call.
// Note: AO has no options (AO_OPTIONS=0), so there is NO simd_by_options variant.
// ---------------------------------------------------------------------------

typedef struct {
    const Stock *stocks;   // array of 4 stocks
    size_t data_len;       // bars per asset (shared across the 4)
} AoSimdCtx;

static void bench_ao_simd_assets(void *ctx_) {
    AoSimdCtx *ctx = ctx_;
    // Each asset has AO_INPUTS=2 input pointers (high, low)
    const double *inputs_per_asset[4][AO_INPUTS] = {
        {ctx->stocks[0].high, ctx->stocks[0].low},
        {ctx->stocks[1].high, ctx->stocks[1].low},
        {ctx->stocks[2].high, ctx->stocks[2].low},
        {ctx->stocks[3].high, ctx->stocks[3].low},
    };
    // inputs is an array of num_assets pointers to input arrays
    const double *inputs[4];
    for (int i = 0; i < 4; i++) {
        inputs[i] = (const double *)&inputs_per_asset[i][0];
    }
    // AO has no options (AO_OPTIONS=0); mirroring scalar call which passes NULL.
    struct CSimdResult r = ao_simd_by_assets((const double *const *const *)inputs, 4, ctx->data_len, NULL, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] ao_simd_by_assets failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) ao_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

static void run_ao(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][AO_OPTIONS] = {{}};
    printf("\n--- AO ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            AoCtx ctx = {.stock = &stocks[s]};

            TimingResult t = time_fn(bench_ao, &ctx, number, repeat, warmup);
            log_and_print("ao", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 0, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_ao, &ctx, number, repeat, warmup);
            log_and_print("ao", "C_tulip", stocks[s].symbol, option_sets[o], 0, t_c, (int) stocks[s].len);

            // No talib comparison available
        }
    }

    // ---- SIMD runs (optional outputs off: NULL, 0) ----
    if (num_stocks >= 4) {
        size_t dlen = stocks[0].len;
        for (int i = 1; i < 4; i++) if (stocks[i].len < dlen) dlen = stocks[i].len;

        AoSimdCtx ctx = { .stocks = stocks, .data_len = dlen };
        TimingResult t = time_fn(bench_ao_simd_assets, &ctx, number, repeat, warmup);
        log_and_print("ao", "tulip_rs_ffi_c_simd_by_assets", "All", NULL, 0, t, (int) dlen);
    }
}
