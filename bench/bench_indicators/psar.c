// psar (Parabolic SAR) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a two-input indicator with options and
// no optional_outputs requested. The flattened inputs buffer is built once per
// stock and reused across all option sets for that stock.

#include "tulip_rs_ffi.h"
#include "../bench_common.h"

typedef struct {
    const Stock *stock;
    const double *inputs_buf; // high ++ low, each stock->len long
    double acceleration_factor, max_acceleration_factor;
} PsarCtx;

static void bench_psar(void *ctx_) {
    PsarCtx *ctx = ctx_;
    const double *inputs[PSAR_INPUTS] = {ctx->stock->high, ctx->stock->low};
    double opts[PSAR_OPTIONS] = {ctx->acceleration_factor, ctx->max_acceleration_factor};
    struct CIndicatorResult r = psar_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] psar_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    psar_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_psar.rs's bench_c_psar exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_psar(void *ctx_) {
    PsarCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    double options[PSAR_OPTIONS] = {ctx->acceleration_factor, ctx->max_acceleration_factor};
    int start_index = ti_psar_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_psar_start returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[PSAR_INPUTS] = {ctx->stock->high, ctx->stock->low};
    double *outputs[1] = {output};
    int ret = ti_psar((int) len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_psar returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib". Mirrors
// tulip_test/benches/benchmark_psar.rs's bench_talib_psar exactly.
// Note: TA-Lib uses TA_SAR (not TA_PSAR) for Parabolic SAR.
// ---------------------------------------------------------------------------

static void bench_talib_psar(void *ctx_) {
    PsarCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    double options[PSAR_OPTIONS] = {ctx->acceleration_factor, ctx->max_acceleration_factor};
    int start_index = TA_SAR_Lookback(options[0], options[1]);
    if (start_index < 0) { fprintf(stderr, "[error] TA_SAR_Lookback returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    int out_begin = 0, out_nb_element = 0;
    TA_RetCode ret =
        TA_SAR(0, (int) len - 1,
               ctx->stock->high, ctx->stock->low,
               options[0], options[1],
               &out_begin, &out_nb_element, output);
    if (ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_SAR returned %d\n", (int) ret); exit(1); }
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
    const double (*option_sets)[PSAR_OPTIONS]; // by_options: 4 option sets
} PsarSimdCtx;

static void bench_psar_simd_assets(void *ctx_) {
    PsarSimdCtx *ctx = ctx_;
    // Each asset has PSAR_INPUTS=2 input pointers (high, low)
    const double *inputs_per_asset[4][PSAR_INPUTS] = {
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
    struct CSimdResult r = psar_simd_by_assets((const double *const *const *)inputs, 4, ctx->data_len, ctx->opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] psar_simd_by_assets failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) psar_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

static void bench_psar_simd_options(void *ctx_) {
    PsarSimdCtx *ctx = ctx_;
    const double *inputs[PSAR_INPUTS] = {ctx->stocks->high, ctx->stocks->low};
    const double *opts[4] = {ctx->option_sets[0], ctx->option_sets[1],
                             ctx->option_sets[2], ctx->option_sets[3]};
    struct CSimdResult r = psar_simd_by_options(inputs, ctx->data_len, opts, 4, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] psar_simd_by_options failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) psar_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

static void run_psar(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][PSAR_OPTIONS] = {{0.02, 0.2}, {0.01, 0.2}, {0.02, 0.1}, {0.04, 0.4}};
    printf("\n--- PSAR ---\n");
    for (int s = 0; s < num_stocks; s++) {
        size_t len = stocks[s].len;
        double *inputs_buf = malloc(sizeof(double) * len * 2);
        memcpy(inputs_buf, stocks[s].high, sizeof(double) * len);
        memcpy(inputs_buf + len, stocks[s].low, sizeof(double) * len);

        for (int o = 0; o < 4; o++) {
            PsarCtx ctx = {
                .stock = &stocks[s],
                .inputs_buf = inputs_buf,
                .acceleration_factor = option_sets[o][0],
                .max_acceleration_factor = option_sets[o][1],
            };

            TimingResult t = time_fn(bench_psar, &ctx, number, repeat, warmup);
            log_and_print("psar", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], PSAR_OPTIONS, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_psar, &ctx, number, repeat, warmup);
            log_and_print("psar", "C_tulip", stocks[s].symbol, option_sets[o], PSAR_OPTIONS, t_c, (int) stocks[s].len);

            TimingResult t_talib = time_fn(bench_talib_psar, &ctx, number, repeat, warmup);
            log_and_print("psar", "talib", stocks[s].symbol, option_sets[o], PSAR_OPTIONS, t_talib, (int) stocks[s].len);
        }

        free(inputs_buf);
    }

    // ---- SIMD runs (optional outputs off: NULL, 0) ----
    if (num_stocks >= 4) {
        size_t dlen = stocks[0].len;
        for (int i = 1; i < 4; i++) if (stocks[i].len < dlen) dlen = stocks[i].len;

        PsarSimdCtx sctx = { .stocks = stocks, .data_len = dlen };
        for (int o = 0; o < 4; o++) {
            sctx.opts = option_sets[o];
            TimingResult t_sa = time_fn(bench_psar_simd_assets, &sctx, number, repeat, warmup);
            log_and_print("psar", "tulip_rs_ffi_c_simd_by_assets", "All", option_sets[o], PSAR_OPTIONS, t_sa, (int) dlen);
        }

        for (int s = 0; s < num_stocks; s++) {
            PsarSimdCtx octx = { .stocks = &stocks[s], .data_len = stocks[s].len, .option_sets = option_sets };
            // One call times 4 option sets; first set logged as the representative key.
            TimingResult t_so = time_fn(bench_psar_simd_options, &octx, number, repeat, warmup);
            log_and_print("psar", "tulip_rs_ffi_c_simd_by_options", stocks[s].symbol, option_sets[0], PSAR_OPTIONS, t_so, (int) stocks[s].len);
        }
    }
}
