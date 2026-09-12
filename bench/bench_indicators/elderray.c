// elderray (Elder-ray Index) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a multi-input indicator with options and
// multiple outputs (bull, bear) plus optional output (ema).

#include "tulip_rs_ffi.h"
#include "../bench_common.h"

typedef struct {
    const Stock *stock;
    double period;
} ElderrayCtx;

static void bench_elderray(void *ctx_) {
    ElderrayCtx *ctx = ctx_;
    const double *inputs[ELDERRAY_INPUTS] = {ctx->stock->high, ctx->stock->low, ctx->stock->close};
    double opts[ELDERRAY_OPTIONS] = {ctx->period};
    struct CIndicatorResult r = elderray_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] elderray_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    elderray_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_elderray.rs's bench_c_elderray exactly.
//
// Note: Elder-ray is computed as bull = high - ema, bear = low - ema,
// where ema is calculated from close prices. The C benchmark uses ti_ema
// and computes the bull/bear values manually to match the Rust implementation.
// ---------------------------------------------------------------------------

static void bench_tulipc_elderray(void *ctx_) {
    ElderrayCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    double options[ELDERRAY_OPTIONS] = {ctx->period};
    
    // Get lookback from ti_ema
    int start_index = ti_ema_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_ema_start returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    
    double *ema_output = malloc(sizeof(double) * (size_t) output_len);
    double *bull_output = malloc(sizeof(double) * (size_t) output_len);
    double *bear_output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[1] = {ctx->stock->close};
    double *outputs[1] = {ema_output};
    
    // Calculate EMA from close
    int ret = ti_ema((int) len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_ema returned %d\n", ret); exit(1); }
    
    // Compute bull and bear: bull = high - ema, bear = low - ema
    for (int i = 0; i < output_len; i++) {
        bull_output[i] = ctx->stock->high[start_index + i] - ema_output[i];
        bear_output[i] = ctx->stock->low[start_index + i] - ema_output[i];
    }
    
    free(ema_output);
    free(bull_output);
    free(bear_output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- not available. TA-Lib has no Elder-ray function.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// SIMD comparisons -- optional outputs always NULL (off).
// *_simd_by_assets() runs one option set across 4 assets in a single call;
// *_simd_by_options() runs 4 option sets on one asset in a single call.
// ---------------------------------------------------------------------------

typedef struct {
    const Stock *stocks;   // by_assets: array of 4 stocks; by_options: single stock
    size_t data_len;       // bars per asset (shared across the 4 for by_assets)
    const double *opts;    // by_assets: the single shared option set
    const double (*option_sets)[ELDERRAY_OPTIONS]; // by_options: 4 option sets
} ElderraySimdCtx;

static void bench_elderray_simd_assets(void *ctx_) {
    ElderraySimdCtx *ctx = ctx_;
    // Each asset has ELDERRAY_INPUTS=3 input pointers (high, low, close)
    const double *inputs_per_asset[4][ELDERRAY_INPUTS] = {
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
    struct CSimdResult r = elderray_simd_by_assets((const double *const *const *)inputs, 4, ctx->data_len, ctx->opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] elderray_simd_by_assets failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) elderray_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

static void bench_elderray_simd_options(void *ctx_) {
    ElderraySimdCtx *ctx = ctx_;
    const double *inputs[ELDERRAY_INPUTS] = {ctx->stocks->high, ctx->stocks->low, ctx->stocks->close};
    const double *opts[4] = {ctx->option_sets[0], ctx->option_sets[1],
                             ctx->option_sets[2], ctx->option_sets[3]};
    struct CSimdResult r = elderray_simd_by_options(inputs, ctx->data_len, opts, 4, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] elderray_simd_by_options failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) elderray_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

static void run_elderray(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][ELDERRAY_OPTIONS] = {{5.0}, {13.0}, {26.0}, {30.0}};
    printf("\n--- ELDERRAY ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            ElderrayCtx ctx = {.stock = &stocks[s], .period = option_sets[o][0]};

            TimingResult t = time_fn(bench_elderray, &ctx, number, repeat, warmup);
            log_and_print("elderray", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 1, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_elderray, &ctx, number, repeat, warmup);
            log_and_print("elderray", "C_tulip", stocks[s].symbol, option_sets[o], 1, t_c, (int) stocks[s].len);

            // No talib comparison available
        }
    }

    // ---- SIMD runs (optional outputs off: NULL, 0) ----
    if (num_stocks >= 4) {
        size_t dlen = stocks[0].len;
        for (int i = 1; i < 4; i++) if (stocks[i].len < dlen) dlen = stocks[i].len;

        ElderraySimdCtx sctx = { .stocks = stocks, .data_len = dlen };
        for (int o = 0; o < 4; o++) {
            sctx.opts = option_sets[o];
            TimingResult t_sa = time_fn(bench_elderray_simd_assets, &sctx, number, repeat, warmup);
            log_and_print("elderray", "tulip_rs_ffi_c_simd_by_assets", "All", option_sets[o], ELDERRAY_OPTIONS, t_sa, (int) dlen);
        }

        for (int s = 0; s < num_stocks; s++) {
            ElderraySimdCtx octx = { .stocks = &stocks[s], .data_len = stocks[s].len, .option_sets = option_sets };
            // One call times 4 option sets; first set logged as the representative key.
            TimingResult t_so = time_fn(bench_elderray_simd_options, &octx, number, repeat, warmup);
            log_and_print("elderray", "tulip_rs_ffi_c_simd_by_options", stocks[s].symbol, option_sets[0], ELDERRAY_OPTIONS, t_so, (int) stocks[s].len);
        }
    }
}
