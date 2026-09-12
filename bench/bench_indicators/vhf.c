// vhf (Vertical Horizontal Filter) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a single-input indicator with one option
// and no optional outputs.

#include "tulip_rs_ffi.h"
#include "../bench_common.h"

typedef struct {
    const Stock *stock;
    double period;
} VhfCtx;

static void bench_vhf(void *ctx_) {
    VhfCtx *ctx = ctx_;
    const double *inputs[VHF_INPUTS] = {ctx->stock->close};
    double opts[VHF_OPTIONS] = {ctx->period};
    struct CIndicatorResult r = vhf_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] vhf_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    vhf_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_vhf.rs's bench_c_vhf exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_vhf(void *ctx_) {
    VhfCtx *ctx = ctx_;
    double options[VHF_OPTIONS] = {ctx->period};
    int start_index = ti_vhf_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_vhf_start returned negative index\n"); exit(1); }
    int output_len = (int) ctx->stock->len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[VHF_INPUTS] = {ctx->stock->close};
    double *outputs[1] = {output};
    int ret = ti_vhf((int) ctx->stock->len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_vhf returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- not available. TA-Lib has no VHF (Vertical Horizontal
// Filter) function.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// SIMD comparisons -- optional outputs always NULL (off).
// vhf_simd_by_assets() runs one option set across 4 assets in a single call;
// vhf_simd_by_options() runs 4 option sets on one asset in a single call.
// ---------------------------------------------------------------------------

typedef struct {
    const Stock *stocks;   // by_assets: array of 4 stocks; by_options: single stock
    size_t data_len;       // bars per asset (shared across the 4 for by_assets)
    const double *opts;    // by_assets: the single shared option set
    const double (*option_sets)[VHF_OPTIONS]; // by_options: 4 option sets
} VhfSimdCtx;

static void bench_vhf_simd_assets(void *ctx_) {
    VhfSimdCtx *ctx = ctx_;
    // Each asset has VHF_INPUTS=1 input pointer (close price)
    const double *inputs_per_asset[4][VHF_INPUTS] = {
        {ctx->stocks[0].close}, {ctx->stocks[1].close},
        {ctx->stocks[2].close}, {ctx->stocks[3].close},
    };
    // inputs is an array of num_assets pointers to input arrays
    const double *inputs[4];
    for (int i = 0; i < 4; i++) {
        inputs[i] = (const double *)&inputs_per_asset[i][0];
    }
    struct CSimdResult r = vhf_simd_by_assets((const double *const *const *)inputs, 4, ctx->data_len, ctx->opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] vhf_simd_by_assets failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) vhf_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

static void bench_vhf_simd_options(void *ctx_) {
    VhfSimdCtx *ctx = ctx_;
    const double *inputs[VHF_INPUTS] = {ctx->stocks->close};
    const double *opts[4] = {ctx->option_sets[0], ctx->option_sets[1],
                             ctx->option_sets[2], ctx->option_sets[3]};
    struct CSimdResult r = vhf_simd_by_options(inputs, ctx->data_len, opts, 4, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] vhf_simd_by_options failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) vhf_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

static void run_vhf(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][VHF_OPTIONS] = {{14.0}, {20.0}, {28.0}, {50.0}};
    printf("\n--- VHF ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            VhfCtx ctx = {.stock = &stocks[s], .period = option_sets[o][0]};

            TimingResult t = time_fn(bench_vhf, &ctx, number, repeat, warmup);
            log_and_print("vhf", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 1, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_vhf, &ctx, number, repeat, warmup);
            log_and_print("vhf", "C_tulip", stocks[s].symbol, option_sets[o], 1, t_c, (int) stocks[s].len);

            // No TA-Lib comparison available
        }
    }

    // ---- SIMD runs (optional outputs off: NULL, 0) ----
    if (num_stocks >= 4) {
        size_t dlen = stocks[0].len;
        for (int i = 1; i < 4; i++) if (stocks[i].len < dlen) dlen = stocks[i].len;

        VhfSimdCtx sctx = { .stocks = stocks, .data_len = dlen };
        for (int o = 0; o < 4; o++) {
            sctx.opts = option_sets[o];
            TimingResult t_sa = time_fn(bench_vhf_simd_assets, &sctx, number, repeat, warmup);
            log_and_print("vhf", "tulip_rs_ffi_c_simd_by_assets", "All", option_sets[o], VHF_OPTIONS, t_sa, (int) dlen);
        }

        for (int s = 0; s < num_stocks; s++) {
            VhfSimdCtx octx = { .stocks = &stocks[s], .data_len = stocks[s].len, .option_sets = option_sets };
            // One call times 4 option sets; first set logged as the representative key.
            TimingResult t_so = time_fn(bench_vhf_simd_options, &octx, number, repeat, warmup);
            log_and_print("vhf", "tulip_rs_ffi_c_simd_by_options", stocks[s].symbol, option_sets[0], VHF_OPTIONS, t_so, (int) stocks[s].len);
        }
    }
}
