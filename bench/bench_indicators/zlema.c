// zlema (Zero-Lag Exponential Moving Average) -- tulip_rs_ffi extern "C" API.
//
// This is one of the three "template" indicators (alongside macd.c and
// stoch.c) that establish the pattern every other file under bench_indicators/
// follows. See ../README.md "Adding a new indicator to this harness".
//
// The timed call performs one full cycle of the wrapper: zlema_indicator()
// (which allocates its outputs inside the wrapper) followed by
// tulip_ffi_result_free() + zlema_state_free(), so allocation/teardown cost
// is included in the measurement.

#include "tulip_rs_ffi.h"
#include "../bench_common.h"

typedef struct {
    const Stock *stock;
    double period;
} ZlemaCtx;

// ---------------------------------------------------------------------------
// SIMD benchmarks -- optional outputs always NULL (off).
// *_simd_by_assets() runs one option set across 4 assets in a single call;
// *_simd_by_options() runs 4 option sets on one asset in a single call.
// ---------------------------------------------------------------------------

typedef struct {
    const Stock *stocks;   // by_assets: array of 4 stocks; by_options: single stock
    size_t data_len;       // bars per asset (shared across the 4 for by_assets)
    const double *opts;    // by_assets: the single shared option set
    const double (*option_sets)[ZLEMA_OPTIONS]; // by_options: 4 option sets
} ZlemaSimdCtx;

static void bench_zlema_simd_assets(void *ctx_) {
    ZlemaSimdCtx *ctx = ctx_;
    // Each asset has ZLEMA_INPUTS=1 input pointer (close price)
    const double *inputs_per_asset[4][ZLEMA_INPUTS] = {
        {ctx->stocks[0].close}, {ctx->stocks[1].close},
        {ctx->stocks[2].close}, {ctx->stocks[3].close},
    };
    // inputs is an array of num_assets pointers to input arrays
    const double *inputs[4];
    for (int i = 0; i < 4; i++) {
        inputs[i] = (const double *)&inputs_per_asset[i][0];
    }
    struct CSimdResult r = zlema_simd_by_assets((const double *const *const *)inputs, 4, ctx->data_len, ctx->opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] zlema_simd_by_assets failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) zlema_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

static void bench_zlema_simd_options(void *ctx_) {
    ZlemaSimdCtx *ctx = ctx_;
    const double *inputs[ZLEMA_INPUTS] = {ctx->stocks->close};
    const double *opts[4] = {ctx->option_sets[0], ctx->option_sets[1],
                             ctx->option_sets[2], ctx->option_sets[3]};
    struct CSimdResult r = zlema_simd_by_options(inputs, ctx->data_len, opts, 4, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] zlema_simd_by_options failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) zlema_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

static void bench_zlema(void *ctx_) {
    ZlemaCtx *ctx = ctx_;
    const double *inputs[ZLEMA_INPUTS] = {ctx->stock->close};
    double opts[ZLEMA_OPTIONS] = {ctx->period};
    struct CIndicatorResult r = zlema_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] zlema_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    zlema_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_zlema.rs's bench_c_zlema exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_zlema(void *ctx_) {
    ZlemaCtx *ctx = ctx_;
    double opts[ZLEMA_OPTIONS] = {ctx->period};
    int start_index = ti_zlema_start(opts);
    if (start_index < 0) { fprintf(stderr, "[error] ti_zlema_start returned negative index\n"); exit(1); }
    int output_len = (int) ctx->stock->len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[ZLEMA_INPUTS] = {ctx->stock->close};
    double *outputs[1] = {output};
    int ret = ti_zlema((int) ctx->stock->len, inputs, opts, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_zlema returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib". Mirrors
// tulip_test/benches/benchmark_zlema.rs's bench_talib_zlema exactly.
//
// NOTE: ZLEMA has no direct TA-Lib equivalent. We omit the TA-Lib benchmark for zlema.
// ---------------------------------------------------------------------------

static void run_zlema(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][ZLEMA_OPTIONS] = {{5.0}, {14.0}, {20.0}, {50.0}};
    printf("\n--- ZLEMA ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            ZlemaCtx ctx = {.stock = &stocks[s], .period = option_sets[o][0]};

            TimingResult t = time_fn(bench_zlema, &ctx, number, repeat, warmup);
            log_and_print("zlema", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 1, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_zlema, &ctx, number, repeat, warmup);
            log_and_print("zlema", "C_tulip", stocks[s].symbol, option_sets[o], 1, t_c, (int) stocks[s].len);
        }
    }

    // ---- SIMD runs (optional outputs off: NULL, 0) ----
    if (num_stocks >= 4) {
        size_t dlen = stocks[0].len;
        for (int i = 1; i < 4; i++) if (stocks[i].len < dlen) dlen = stocks[i].len;

        ZlemaSimdCtx sctx = { .stocks = stocks, .data_len = dlen };
        for (int o = 0; o < 4; o++) {
            sctx.opts = option_sets[o];
            TimingResult t_sa = time_fn(bench_zlema_simd_assets, &sctx, number, repeat, warmup);
            log_and_print("zlema", "tulip_rs_ffi_c_simd_by_assets", "All", option_sets[o], ZLEMA_OPTIONS, t_sa, (int) dlen);
        }

        for (int s = 0; s < num_stocks; s++) {
            ZlemaSimdCtx octx = { .stocks = &stocks[s], .data_len = stocks[s].len, .option_sets = option_sets };
            // One call times 4 option sets; first set logged as the representative key.
            TimingResult t_so = time_fn(bench_zlema_simd_options, &octx, number, repeat, warmup);
            log_and_print("zlema", "tulip_rs_ffi_c_simd_by_options", stocks[s].symbol, option_sets[0], ZLEMA_OPTIONS, t_so, (int) stocks[s].len);
        }
    }
}
