// hilberttransform (Ehlers Hilbert Transform) -- tulip_rs_ffi extern "C" API.
//
// This is one of the three "template" indicators (alongside macd.c and
// stoch.c) that establish the pattern every other file under bench_indicators/
// follows. See ../README.md "Adding a new indicator to this harness".

#include "tulip_rs_ffi.h"
#include "../bench_common.h"

typedef struct {
    const Stock *stock;
    double ss_period, hp_period;
} HilbertTransformCtx;

static void bench_hilberttransform(void *ctx_) {
    HilbertTransformCtx *ctx = ctx_;
    const double *inputs[HILBERTTRANSFORM_INPUTS] = {ctx->stock->close};
    double opts[HILBERTTRANSFORM_OPTIONS] = {ctx->ss_period, ctx->hp_period};
    struct CIndicatorResult r = hilberttransform_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] hilberttransform_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    hilberttransform_state_free(r.state);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib". Mirrors
// tulip_test/benches/benchmark_hilberttransform.rs's bench_talib_ht_phasor.
// TA-HT_PHASOR outputs in_phase and quadrature, which match our hilberttransform
// outputs [in_phase, quadrature].
// ---------------------------------------------------------------------------

static void bench_talib_hilberttransform(void *ctx_) {
    HilbertTransformCtx *ctx = ctx_;
    int size = (int) ctx->stock->len;
    int start_index = TA_HT_PHASOR_Lookback();
    if (start_index < 0) { fprintf(stderr, "[error] TA_HT_PHASOR_Lookback returned negative index\n"); exit(1); }
    int output_len = size - start_index;
    
    double *out_in_phase = malloc(sizeof(double) * (size_t) output_len);
    double *out_quadrature = malloc(sizeof(double) * (size_t) output_len);
    
    int out_begin = 0, out_nb_element = 0;
    TA_RetCode ret = TA_HT_PHASOR(
        0,
        size - 1,
        ctx->stock->close,
        &out_begin,
        &out_nb_element,
        out_in_phase,
        out_quadrature
    );
    
    if (ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_HT_PHASOR returned %d\n", (int) ret); exit(1); }
    
    free(out_in_phase);
    free(out_quadrature);
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
    const double (*option_sets)[HILBERTTRANSFORM_OPTIONS]; // by_options: 4 option sets
} HilbertTransformSimdCtx;

static void bench_hilberttransform_simd_assets(void *ctx_) {
    HilbertTransformSimdCtx *ctx = ctx_;
    // Each asset has HILBERTTRANSFORM_INPUTS=1 input pointer (close price)
    const double *inputs_per_asset[4][HILBERTTRANSFORM_INPUTS] = {
        {ctx->stocks[0].close}, {ctx->stocks[1].close},
        {ctx->stocks[2].close}, {ctx->stocks[3].close},
    };
    // inputs is an array of num_assets pointers to input arrays
    const double *inputs[4];
    for (int i = 0; i < 4; i++) {
        inputs[i] = (const double *)&inputs_per_asset[i][0];
    }
    struct CSimdResult r = hilberttransform_simd_by_assets((const double *const *const *)inputs, 4, ctx->data_len, ctx->opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] hilberttransform_simd_by_assets failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) hilberttransform_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

static void bench_hilberttransform_simd_options(void *ctx_) {
    HilbertTransformSimdCtx *ctx = ctx_;
    const double *inputs[HILBERTTRANSFORM_INPUTS] = {ctx->stocks->close};
    const double *opts[4] = {ctx->option_sets[0], ctx->option_sets[1],
                             ctx->option_sets[2], ctx->option_sets[3]};
    struct CSimdResult r = hilberttransform_simd_by_options(inputs, ctx->data_len, opts, 4, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] hilberttransform_simd_by_options failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) hilberttransform_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

static void run_hilberttransform(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][HILBERTTRANSFORM_OPTIONS] = {{10.0, 20.0}, {15.0, 30.0}, {20.0, 40.0}, {25.0, 50.0}};
    printf("\n--- HILBERTTRANSFORM ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            HilbertTransformCtx ctx = {.stock = &stocks[s], .ss_period = option_sets[o][0], .hp_period = option_sets[o][1]};

            TimingResult t = time_fn(bench_hilberttransform, &ctx, number, repeat, warmup);
            log_and_print("hilberttransform", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 2, t, (int) stocks[s].len);

            // No C_tulip comparison - no ti_hilberttransform implementation

            TimingResult t_talib = time_fn(bench_talib_hilberttransform, &ctx, number, repeat, warmup);
            log_and_print("hilberttransform", "talib", stocks[s].symbol, option_sets[o], 2, t_talib, (int) stocks[s].len);
        }
    }

    // ---- SIMD runs (optional outputs off: NULL, 0) ----
    if (num_stocks >= 4) {
        size_t dlen = stocks[0].len;
        for (int i = 1; i < 4; i++) if (stocks[i].len < dlen) dlen = stocks[i].len;

        HilbertTransformSimdCtx sctx = { .stocks = stocks, .data_len = dlen };
        for (int o = 0; o < 4; o++) {
            sctx.opts = option_sets[o];
            TimingResult t_sa = time_fn(bench_hilberttransform_simd_assets, &sctx, number, repeat, warmup);
            log_and_print("hilberttransform", "tulip_rs_ffi_c_simd_by_assets", "All", option_sets[o], HILBERTTRANSFORM_OPTIONS, t_sa, (int) dlen);
        }

        for (int s = 0; s < num_stocks; s++) {
            HilbertTransformSimdCtx octx = { .stocks = &stocks[s], .data_len = stocks[s].len, .option_sets = option_sets };
            // One call times 4 option sets; first set logged as the representative key.
            TimingResult t_so = time_fn(bench_hilberttransform_simd_options, &octx, number, repeat, warmup);
            log_and_print("hilberttransform", "tulip_rs_ffi_c_simd_by_options", stocks[s].symbol, option_sets[0], HILBERTTRANSFORM_OPTIONS, t_so, (int) stocks[s].len);
        }
    }
}
