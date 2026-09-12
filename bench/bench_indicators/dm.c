// dm (Directional Movement) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a multi-input indicator with options and
// multiple mandatory outputs (+DM, -DM).

#include "tulip_rs_ffi.h"
#include "../bench_common.h"

typedef struct {
    const Stock *stock;
    double period;
} DmCtx;

// ---------------------------------------------------------------------------
// SIMD comparisons -- optional outputs always NULL (off).
// *_simd_by_assets() runs one option set across 4 assets in a single call;
// *_simd_by_options() runs 4 option sets on one asset in a single call.
// ---------------------------------------------------------------------------

typedef struct {
    const Stock *stocks;   // by_assets: array of 4 stocks; by_options: single stock
    size_t data_len;       // bars per asset (shared across the 4 for by_assets)
    const double *opts;    // by_assets: the single shared option set
    const double (*option_sets)[DM_OPTIONS]; // by_options: 4 option sets
} DmSimdCtx;

static void bench_dm(void *ctx_) {
    DmCtx *ctx = ctx_;
    const double *inputs[DM_INPUTS] = {ctx->stock->high, ctx->stock->low};
    double opts[DM_OPTIONS] = {ctx->period};
    struct CIndicatorResult r = dm_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] dm_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    dm_state_free(r.state);
}

static void bench_dm_simd_assets(void *ctx_) {
    DmSimdCtx *ctx = ctx_;
    const double *inputs_per_asset[4][DM_INPUTS] = {
        {ctx->stocks[0].high, ctx->stocks[0].low},
        {ctx->stocks[1].high, ctx->stocks[1].low},
        {ctx->stocks[2].high, ctx->stocks[2].low},
        {ctx->stocks[3].high, ctx->stocks[3].low},
    };
    const double *inputs[4];
    for (int i = 0; i < 4; i++) {
        inputs[i] = (const double *)&inputs_per_asset[i][0];
    }
    struct CSimdResult r = dm_simd_by_assets((const double *const *const *)inputs, 4, ctx->data_len, ctx->opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] dm_simd_by_assets failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) dm_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

static void bench_dm_simd_options(void *ctx_) {
    DmSimdCtx *ctx = ctx_;
    const double *inputs[DM_INPUTS] = {ctx->stocks->high, ctx->stocks->low};
    const double *opts[4] = {ctx->option_sets[0], ctx->option_sets[1],
                             ctx->option_sets[2], ctx->option_sets[3]};
    struct CSimdResult r = dm_simd_by_options(inputs, ctx->data_len, opts, 4, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] dm_simd_by_options failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) dm_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_dm.rs's bench_c_dm exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_dm(void *ctx_) {
    DmCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    double options[1] = {ctx->period};
    int start_index = ti_dm_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_dm_start returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    double *output_plus_dm = malloc(sizeof(double) * (size_t) output_len);
    double *output_minus_dm = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[2] = {ctx->stock->high, ctx->stock->low};
    double *outputs[2] = {output_plus_dm, output_minus_dm};
    int ret = ti_dm((int) len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_dm returned %d\n", ret); exit(1); }
    free(output_plus_dm);
    free(output_minus_dm);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib".
// NOTE: TA-Lib does not provide an equivalent for dm.
// ---------------------------------------------------------------------------

static void run_dm(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][DM_OPTIONS] = {{24.0}, {14.0}, {5.0}, {30.0}};
    printf("\n--- DM ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            DmCtx ctx = {
                .stock = &stocks[s],
                .period = option_sets[o][0],
            };

            TimingResult t = time_fn(bench_dm, &ctx, number, repeat, warmup);
            log_and_print("dm", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], DM_OPTIONS, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_dm, &ctx, number, repeat, warmup);
            log_and_print("dm", "C_tulip", stocks[s].symbol, option_sets[o], DM_OPTIONS, t_c, (int) stocks[s].len);

            // No talib comparison available
        }
    }

    // ---- SIMD runs (optional outputs off: NULL, 0) ----
    if (num_stocks >= 4) {
        size_t dlen = stocks[0].len;
        for (int i = 1; i < 4; i++) if (stocks[i].len < dlen) dlen = stocks[i].len;

        DmSimdCtx sctx = { .stocks = stocks, .data_len = dlen };
        for (int o = 0; o < 4; o++) {
            sctx.opts = option_sets[o];
            TimingResult t_sa = time_fn(bench_dm_simd_assets, &sctx, number, repeat, warmup);
            log_and_print("dm", "tulip_rs_ffi_c_simd_by_assets", "All", option_sets[o], DM_OPTIONS, t_sa, (int) dlen);
        }

        for (int s = 0; s < num_stocks; s++) {
            DmSimdCtx octx = { .stocks = &stocks[s], .data_len = stocks[s].len, .option_sets = option_sets };
            // One call times 4 option sets; first set logged as the representative key.
            TimingResult t_so = time_fn(bench_dm_simd_options, &octx, number, repeat, warmup);
            log_and_print("dm", "tulip_rs_ffi_c_simd_by_options", stocks[s].symbol, option_sets[0], DM_OPTIONS, t_so, (int) stocks[s].len);
        }
    }
}
