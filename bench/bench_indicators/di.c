// di (Directional Indicator) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a multi-input indicator with options and
// multiple mandatory outputs (+DI, -DI) plus optional outputs (ATR, TR).
// The pointer array is built inside the timed region.

#include "tulip_rs_ffi.h"
#include "../bench_common.h"

typedef struct {
    const Stock *stock;
    double period;
} DiCtx;

// ---------------------------------------------------------------------------
// SIMD comparisons -- optional outputs always NULL (off).
// *_simd_by_assets() runs one option set across 4 assets in a single call;
// *_simd_by_options() runs 4 option sets on one asset in a single call.
// ---------------------------------------------------------------------------

typedef struct {
    const Stock *stocks;   // by_assets: array of 4 stocks; by_options: single stock
    size_t data_len;       // bars per asset (shared across the 4 for by_assets)
    const double *opts;    // by_assets: the single shared option set
    const double (*option_sets)[DI_OPTIONS]; // by_options: 4 option sets
} DiSimdCtx;

static void bench_di(void *ctx_) {
    DiCtx *ctx = ctx_;
    const double *inputs[DI_INPUTS] = {ctx->stock->high, ctx->stock->low, ctx->stock->close};
    double opts[DI_OPTIONS] = {ctx->period};
    struct CIndicatorResult r = di_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] di_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    di_state_free(r.state);
}

static void bench_di_simd_assets(void *ctx_) {
    DiSimdCtx *ctx = ctx_;
    const double *inputs_per_asset[4][DI_INPUTS] = {
        {ctx->stocks[0].high, ctx->stocks[0].low, ctx->stocks[0].close},
        {ctx->stocks[1].high, ctx->stocks[1].low, ctx->stocks[1].close},
        {ctx->stocks[2].high, ctx->stocks[2].low, ctx->stocks[2].close},
        {ctx->stocks[3].high, ctx->stocks[3].low, ctx->stocks[3].close},
    };
    const double *inputs[4];
    for (int i = 0; i < 4; i++) {
        inputs[i] = (const double *)&inputs_per_asset[i][0];
    }
    struct CSimdResult r = di_simd_by_assets((const double *const *const *)inputs, 4, ctx->data_len, ctx->opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] di_simd_by_assets failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) di_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

static void bench_di_simd_options(void *ctx_) {
    DiSimdCtx *ctx = ctx_;
    const double *inputs[DI_INPUTS] = {ctx->stocks->high, ctx->stocks->low, ctx->stocks->close};
    const double *opts[4] = {ctx->option_sets[0], ctx->option_sets[1],
                             ctx->option_sets[2], ctx->option_sets[3]};
    struct CSimdResult r = di_simd_by_options(inputs, ctx->data_len, opts, 4, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] di_simd_by_options failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) di_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_di.rs's bench_c_di exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_di(void *ctx_) {
    DiCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    double options[1] = {ctx->period};
    int start_index = ti_di_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_di_start returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    double *output_plus_di = malloc(sizeof(double) * (size_t) output_len);
    double *output_minus_di = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[3] = {ctx->stock->high, ctx->stock->low, ctx->stock->close};
    double *outputs[2] = {output_plus_di, output_minus_di};
    int ret = ti_di((int) len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_di returned %d\n", ret); exit(1); }
    free(output_plus_di);
    free(output_minus_di);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib".
//
// TA-Lib exposes +DI/-DI as two separate functions (TA_PLUS_DI/TA_MINUS_DI),
// but our di() computes both in a single call, so for an apples-to-apples
// comparison this closure calls both TA-Lib functions back-to-back and times
// them together as one "talib" sample.
// ---------------------------------------------------------------------------

static void bench_talib_di(void *ctx_) {
    DiCtx *ctx = ctx_;
    size_t len = ctx->stock->len;

    int plus_start = TA_PLUS_DI_Lookback((int) ctx->period);
    if (plus_start < 0) { fprintf(stderr, "[error] TA_PLUS_DI_Lookback returned negative index\n"); exit(1); }
    int plus_len = (int) len - plus_start;
    double *plus_output = malloc(sizeof(double) * (size_t) plus_len);
    int plus_begin = 0, plus_nb_element = 0;
    TA_RetCode plus_ret = TA_PLUS_DI(0, (int) len - 1,
                                      ctx->stock->high, ctx->stock->low, ctx->stock->close,
                                      (int) ctx->period,
                                      &plus_begin, &plus_nb_element, plus_output);
    if (plus_ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_PLUS_DI returned %d\n", (int) plus_ret); exit(1); }

    int minus_start = TA_MINUS_DI_Lookback((int) ctx->period);
    if (minus_start < 0) { fprintf(stderr, "[error] TA_MINUS_DI_Lookback returned negative index\n"); exit(1); }
    int minus_len = (int) len - minus_start;
    double *minus_output = malloc(sizeof(double) * (size_t) minus_len);
    int minus_begin = 0, minus_nb_element = 0;
    TA_RetCode minus_ret = TA_MINUS_DI(0, (int) len - 1,
                                        ctx->stock->high, ctx->stock->low, ctx->stock->close,
                                        (int) ctx->period,
                                        &minus_begin, &minus_nb_element, minus_output);
    if (minus_ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_MINUS_DI returned %d\n", (int) minus_ret); exit(1); }

    free(plus_output);
    free(minus_output);
}

static void run_di(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][DI_OPTIONS] = {{5.0}, {14.0}, {20.0}, {30.0}};
    printf("\n--- DI ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            DiCtx ctx = {
                .stock = &stocks[s],
                .period = option_sets[o][0],
            };

            TimingResult t = time_fn(bench_di, &ctx, number, repeat, warmup);
            log_and_print("di", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], DI_OPTIONS, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_di, &ctx, number, repeat, warmup);
            log_and_print("di", "C_tulip", stocks[s].symbol, option_sets[o], DI_OPTIONS, t_c, (int) stocks[s].len);

            // TA-Lib has no combined +DI/-DI function, so bench_talib_di calls
            // both TA_PLUS_DI and TA_MINUS_DI and times them together, matching
            // the fact that our di() computes both outputs in a single call.
            TimingResult t_talib = time_fn(bench_talib_di, &ctx, number, repeat, warmup);
            log_and_print("di", "talib", stocks[s].symbol, option_sets[o], DI_OPTIONS, t_talib, (int) stocks[s].len);
        }
    }

    // ---- SIMD runs (optional outputs off: NULL, 0) ----
    if (num_stocks >= 4) {
        size_t dlen = stocks[0].len;
        for (int i = 1; i < 4; i++) if (stocks[i].len < dlen) dlen = stocks[i].len;

        DiSimdCtx sctx = { .stocks = stocks, .data_len = dlen };
        for (int o = 0; o < 4; o++) {
            sctx.opts = option_sets[o];
            TimingResult t_sa = time_fn(bench_di_simd_assets, &sctx, number, repeat, warmup);
            log_and_print("di", "tulip_rs_ffi_c_simd_by_assets", "All", option_sets[o], DI_OPTIONS, t_sa, (int) dlen);
        }

        for (int s = 0; s < num_stocks; s++) {
            DiSimdCtx octx = { .stocks = &stocks[s], .data_len = stocks[s].len, .option_sets = option_sets };
            // One call times 4 option sets; first set logged as the representative key.
            TimingResult t_so = time_fn(bench_di_simd_options, &octx, number, repeat, warmup);
            log_and_print("di", "tulip_rs_ffi_c_simd_by_options", stocks[s].symbol, option_sets[0], DI_OPTIONS, t_so, (int) stocks[s].len);
        }
    }
}
