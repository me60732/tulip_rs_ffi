// kvo (Klinger Volume Oscillator) -- tulip_rs_ffi extern "C" API.
//
// This is one of the three "template" indicators (alongside macd.c and
// stoch.c) that establish the pattern every other file under bench_indicators/
// follows. See ../README.md "Adding a new indicator to this harness".

#include "tulip_rs_ffi.h"
#include "../bench_common.h"

typedef struct {
    const Stock *stock;
    double short_period, long_period;
} KvoCtx;

static void bench_kvo(void *ctx_) {
    KvoCtx *ctx = ctx_;
    const double *inputs[KVO_INPUTS] = {ctx->stock->high, ctx->stock->low, ctx->stock->close, ctx->stock->volume};
    double opts[KVO_OPTIONS] = {ctx->short_period, ctx->long_period};
    struct CIndicatorResult r = kvo_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] kvo_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    kvo_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_kvo.rs's bench_c_kvo exactly (lookback
// computation + output allocation inside the timed region).
// ---------------------------------------------------------------------------

static void bench_tulipc_kvo(void *ctx_) {
    KvoCtx *ctx = ctx_;
    double options[KVO_OPTIONS] = {ctx->short_period, ctx->long_period};
    int start_index = ti_kvo_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_kvo_start returned negative index\n"); exit(1); }
    int output_len = (int) ctx->stock->len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[KVO_INPUTS] = {ctx->stock->high, ctx->stock->low, ctx->stock->close, ctx->stock->volume};
    double *outputs[1] = {output};
    int ret = ti_kvo((int) ctx->stock->len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_kvo returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// SIMD comparisons -- optional outputs always NULL (off).
// *_simd_by_assets() runs one option set across 4 assets in a single call;
// *_simd_by_options() runs 4 option sets on one asset in a single call.
// ---------------------------------------------------------------------------

typedef struct {
    const Stock *stocks;   // by_assets: array of 4 stocks; by_options: single stock
    size_t data_len;       // bars per asset (shared across the 4 for by_assets)
    const double *opts;    // by_assets: the single shared option set
    const double (*option_sets)[KVO_OPTIONS]; // by_options: 4 option sets
} KvoSimdCtx;

static void bench_kvo_simd_assets(void *ctx_) {
    KvoSimdCtx *ctx = ctx_;
    // Each asset has KVO_INPUTS=4 input pointers (high, low, close, volume)
    const double *inputs_per_asset[4][KVO_INPUTS] = {
        {ctx->stocks[0].high, ctx->stocks[0].low, ctx->stocks[0].close, ctx->stocks[0].volume},
        {ctx->stocks[1].high, ctx->stocks[1].low, ctx->stocks[1].close, ctx->stocks[1].volume},
        {ctx->stocks[2].high, ctx->stocks[2].low, ctx->stocks[2].close, ctx->stocks[2].volume},
        {ctx->stocks[3].high, ctx->stocks[3].low, ctx->stocks[3].close, ctx->stocks[3].volume},
    };
    // inputs is an array of num_assets pointers to input arrays
    const double *inputs[4];
    for (int i = 0; i < 4; i++) {
        inputs[i] = (const double *)&inputs_per_asset[i][0];
    }
    struct CSimdResult r = kvo_simd_by_assets((const double *const *const *)inputs, 4, ctx->data_len, ctx->opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] kvo_simd_by_assets failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) kvo_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

static void bench_kvo_simd_options(void *ctx_) {
    KvoSimdCtx *ctx = ctx_;
    const double *inputs[KVO_INPUTS] = {ctx->stocks->high, ctx->stocks->low, ctx->stocks->close, ctx->stocks->volume};
    const double *opts[4] = {ctx->option_sets[0], ctx->option_sets[1],
                             ctx->option_sets[2], ctx->option_sets[3]};
    struct CSimdResult r = kvo_simd_by_options(inputs, ctx->data_len, opts, 4, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] kvo_simd_by_options failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) kvo_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

static void run_kvo(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][KVO_OPTIONS] = {{34.0, 55.0}, {20.0, 40.0}, {10.0, 30.0}, {5.0, 20.0}};
    printf("\n--- KVO ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            KvoCtx ctx = {.stock = &stocks[s], .short_period = option_sets[o][0], .long_period = option_sets[o][1]};

            TimingResult t = time_fn(bench_kvo, &ctx, number, repeat, warmup);
            log_and_print("kvo", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 2, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_kvo, &ctx, number, repeat, warmup);
            log_and_print("kvo", "C_tulip", stocks[s].symbol, option_sets[o], 2, t_c, (int) stocks[s].len);
        }
    }

    // ---- SIMD runs (optional outputs off: NULL, 0) ----
    if (num_stocks >= 4) {
        size_t dlen = stocks[0].len;
        for (int i = 1; i < 4; i++) if (stocks[i].len < dlen) dlen = stocks[i].len;

        KvoSimdCtx sctx = { .stocks = stocks, .data_len = dlen };
        for (int o = 0; o < 4; o++) {
            sctx.opts = option_sets[o];
            TimingResult t_sa = time_fn(bench_kvo_simd_assets, &sctx, number, repeat, warmup);
            log_and_print("kvo", "tulip_rs_ffi_c_simd_by_assets", "All", option_sets[o], KVO_OPTIONS, t_sa, (int) dlen);
        }

        for (int s = 0; s < num_stocks; s++) {
            KvoSimdCtx octx = { .stocks = &stocks[s], .data_len = stocks[s].len, .option_sets = option_sets };
            // One call times 4 option sets; first set logged as the representative key.
            TimingResult t_so = time_fn(bench_kvo_simd_options, &octx, number, repeat, warmup);
            log_and_print("kvo", "tulip_rs_ffi_c_simd_by_options", stocks[s].symbol, option_sets[0], KVO_OPTIONS, t_so, (int) stocks[s].len);
        }
    }
}
