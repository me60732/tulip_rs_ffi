// mfi (Money Flow Index) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a multi-input indicator with one option.

#include "tulip_rs_ffi.h"
#include "../bench_common.h"

// ---------------------------------------------------------------------------
// SIMD comparisons -- optional outputs always NULL (off).
// *_simd_by_assets() runs one option set across 4 assets in a single call;
// *_simd_by_options() runs 4 option sets on one asset in a single call.
// ---------------------------------------------------------------------------

typedef struct {
    const Stock *stocks;   // by_assets: array of 4 stocks; by_options: single stock
    size_t data_len;       // bars per asset (shared across the 4 for by_assets)
    const double *opts;    // by_assets: the single shared option set
    const double (*option_sets)[MFI_OPTIONS]; // by_options: 4 option sets
} MfiSimdCtx;

static void bench_mfi_simd_assets(void *ctx_) {
    MfiSimdCtx *ctx = ctx_;
    // Each asset has MFI_INPUTS=4 input pointers (high, low, close, volume)
    const double *inputs_per_asset[4][MFI_INPUTS] = {
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
    struct CSimdResult r = mfi_simd_by_assets((const double *const *const *)inputs, 4, ctx->data_len, ctx->opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] mfi_simd_by_assets failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) mfi_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

static void bench_mfi_simd_options(void *ctx_) {
    MfiSimdCtx *ctx = ctx_;
    const double *inputs[MFI_INPUTS] = {ctx->stocks->high, ctx->stocks->low, ctx->stocks->close, ctx->stocks->volume};
    const double *opts[4] = {ctx->option_sets[0], ctx->option_sets[1],
                             ctx->option_sets[2], ctx->option_sets[3]};
    struct CSimdResult r = mfi_simd_by_options(inputs, ctx->data_len, opts, 4, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] mfi_simd_by_options failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) mfi_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

typedef struct {
    const Stock *stock;
    const double *inputs_buf; // high ++ low ++ close ++ volume, each stock->len long
    double period;
} MfiCtx;

static void bench_mfi(void *ctx_) {
    MfiCtx *ctx = ctx_;
    const double *inputs[MFI_INPUTS] = {ctx->stock->high, ctx->stock->low, ctx->stock->close, ctx->stock->volume};
    double opts[MFI_OPTIONS] = {ctx->period};
    struct CIndicatorResult r = mfi_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] mfi_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    mfi_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_mfi.rs's bench_c_mfi exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_mfi(void *ctx_) {
    MfiCtx *ctx = ctx_;
    double options[1] = {ctx->period};
    int start_index = ti_mfi_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_mfi_start returned negative index\n"); exit(1); }
    int output_len = (int) ctx->stock->len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[4] = {ctx->stock->high, ctx->stock->low, ctx->stock->close, ctx->stock->volume};
    double *outputs[1] = {output};
    int ret = ti_mfi((int) ctx->stock->len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_mfi returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib". Mirrors C_tulip pattern.
// TA_MFI_Lookback(optInTimePeriod) returns int, uses high/low/close/volume arrays.
// ---------------------------------------------------------------------------

static void bench_talib_mfi(void *ctx_) {
    MfiCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    int start_index = TA_MFI_Lookback((int) ctx->period);
    if (start_index < 0) { fprintf(stderr, "[error] TA_MFI_Lookback returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    int out_begin = 0, out_nb_element = 0;
    TA_RetCode ret = TA_MFI(0, (int) len - 1,
                            ctx->stock->high, ctx->stock->low, ctx->stock->close, ctx->stock->volume,
                            (int) ctx->period,
                            &out_begin, &out_nb_element, output);
    if (ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_MFI returned %d\n", (int) ret); exit(1); }
    free(output);
}

static void run_mfi(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][MFI_OPTIONS] = {{14.0}, {20.0}, {25.0}, {30.0}};
    printf("\n--- MFI ---\n");
    for (int s = 0; s < num_stocks; s++) {
        size_t len = stocks[s].len;
        double *inputs_buf = malloc(sizeof(double) * len * 4);
        memcpy(inputs_buf, stocks[s].high, sizeof(double) * len);
        memcpy(inputs_buf + len, stocks[s].low, sizeof(double) * len);
        memcpy(inputs_buf + 2 * len, stocks[s].close, sizeof(double) * len);
        memcpy(inputs_buf + 3 * len, stocks[s].volume, sizeof(double) * len);

        for (int o = 0; o < 4; o++) {
            MfiCtx ctx = {
                .stock = &stocks[s],
                .inputs_buf = inputs_buf,
                .period = option_sets[o][0],
            };

            TimingResult t = time_fn(bench_mfi, &ctx, number, repeat, warmup);
            log_and_print("mfi", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 1, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_mfi, &ctx, number, repeat, warmup);
            log_and_print("mfi", "C_tulip", stocks[s].symbol, option_sets[o], 1, t_c, (int) stocks[s].len);

            TimingResult t_talib = time_fn(bench_talib_mfi, &ctx, number, repeat, warmup);
            log_and_print("mfi", "talib", stocks[s].symbol, option_sets[o], 1, t_talib, (int) stocks[s].len);
        }

        free(inputs_buf);
    }

    // ---- SIMD runs (optional outputs off: NULL, 0) ----
    if (num_stocks >= 4) {
        size_t dlen = stocks[0].len;
        for (int i = 1; i < 4; i++) if (stocks[i].len < dlen) dlen = stocks[i].len;

        MfiSimdCtx sctx = { .stocks = stocks, .data_len = dlen };
        for (int o = 0; o < 4; o++) {
            sctx.opts = option_sets[o];
            TimingResult t_sa = time_fn(bench_mfi_simd_assets, &sctx, number, repeat, warmup);
            log_and_print("mfi", "tulip_rs_ffi_c_simd_by_assets", "All", option_sets[o], MFI_OPTIONS, t_sa, (int) dlen);
        }

        for (int s = 0; s < num_stocks; s++) {
            MfiSimdCtx octx = { .stocks = &stocks[s], .data_len = stocks[s].len, .option_sets = option_sets };
            // One call times 4 option sets; first set logged as the representative key.
            TimingResult t_so = time_fn(bench_mfi_simd_options, &octx, number, repeat, warmup);
            log_and_print("mfi", "tulip_rs_ffi_c_simd_by_options", stocks[s].symbol, option_sets[0], MFI_OPTIONS, t_so, (int) stocks[s].len);
        }
    }
}
