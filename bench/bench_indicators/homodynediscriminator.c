// homodynediscriminator (Ehlers Dominant Cycle Period) -- tulip_rs_ffi extern "C" API.
//
// This is one of the three "template" indicators (alongside macd.c and
// stoch.c) that establish the pattern every other file under bench_indicators/
// follows. See ../README.md "Adding a new indicator to this harness".

#include "tulip_rs_ffi.h"
#include "../bench_common.h"

typedef struct {
    const Stock *stock;
} HomodyneDiscriminatorCtx;

static void bench_homodynediscriminator(void *ctx_) {
    HomodyneDiscriminatorCtx *ctx = ctx_;
    const double *inputs[HOMODYNEDISCRIMINATOR_INPUTS] = {ctx->stock->close};
    struct CIndicatorResult r = homodynediscriminator_indicator(inputs, ctx->stock->len, NULL, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] homodynediscriminator_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    homodynediscriminator_state_free(r.state);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib". Mirrors
// tulip_test/benches/benchmark_homodynediscriminator.rs's bench_talib_ht_dcperiod.
// TA-HT_DCPERIOD outputs dc_period, which matches our homodynediscriminator
// output [dc_period]. Lookback is 32 bars.
// ---------------------------------------------------------------------------

static void bench_talib_homodynediscriminator(void *ctx_) {
    HomodyneDiscriminatorCtx *ctx = ctx_;
    int size = (int) ctx->stock->len;
    int start_index = TA_HT_DCPERIOD_Lookback();
    if (start_index < 0) { fprintf(stderr, "[error] TA_HT_DCPERIOD_Lookback returned negative index\n"); exit(1); }
    int output_len = size - start_index;
    
    double *out_dc_period = malloc(sizeof(double) * (size_t) output_len);
    
    int out_begin = 0, out_nb_element = 0;
    TA_RetCode ret = TA_HT_DCPERIOD(
        0,
        size - 1,
        ctx->stock->close,
        &out_begin,
        &out_nb_element,
        out_dc_period
    );
    
    if (ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_HT_DCPERIOD returned %d\n", (int) ret); exit(1); }
    
    free(out_dc_period);
}

// ---------------------------------------------------------------------------
// SIMD comparisons -- optional outputs always NULL (off).
// homodynediscriminator_simd_by_assets() runs one option set across 4 assets.
// Note: HOMODYNEDISCRIMINATOR has no options (HOMODYNEDISCRIMINATOR_OPTIONS=0),
// so there is NO simd_by_options variant.
// ---------------------------------------------------------------------------

typedef struct {
    const Stock *stocks;   // array of 4 stocks
    size_t data_len;       // bars per asset (shared across the 4)
} HomodyneDiscriminatorSimdCtx;

static void bench_homodynediscriminator_simd_assets(void *ctx_) {
    HomodyneDiscriminatorSimdCtx *ctx = ctx_;
    // Each asset has HOMODYNEDISCRIMINATOR_INPUTS=1 input pointer (close price)
    const double *inputs_per_asset[4][HOMODYNEDISCRIMINATOR_INPUTS] = {
        {ctx->stocks[0].close}, {ctx->stocks[1].close},
        {ctx->stocks[2].close}, {ctx->stocks[3].close},
    };
    // inputs is an array of num_assets pointers to input arrays
    const double *inputs[4];
    for (int i = 0; i < 4; i++) {
        inputs[i] = (const double *)&inputs_per_asset[i][0];
    }
    // HOMODYNEDISCRIMINATOR has no options (HOMODYNEDISCRIMINATOR_OPTIONS=0);
    // mirroring scalar call which passes NULL.
    struct CSimdResult r = homodynediscriminator_simd_by_assets((const double *const *const *)inputs, 4, ctx->data_len, NULL, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] homodynediscriminator_simd_by_assets failed: %d\n", (int) r.error);
        exit(1);
    }
    for (uintptr_t i = 0; i < r.num_results; i++) homodynediscriminator_state_free(r.states[i]);
    tulip_ffi_simd_result_free(r);
}

static void run_homodynediscriminator(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][HOMODYNEDISCRIMINATOR_OPTIONS] = {{}};  // No options for this indicator
    printf("\n--- HOMODYNEDISCRIMINATOR ---\n");
    for (int s = 0; s < num_stocks; s++) {
        HomodyneDiscriminatorCtx ctx = {.stock = &stocks[s]};

        TimingResult t = time_fn(bench_homodynediscriminator, &ctx, number, repeat, warmup);
        log_and_print("homodynediscriminator", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[0], 0, t, (int) stocks[s].len);

        // No C_tulip comparison - no ti_homodynediscriminator implementation

        TimingResult t_talib = time_fn(bench_talib_homodynediscriminator, &ctx, number, repeat, warmup);
        log_and_print("homodynediscriminator", "talib", stocks[s].symbol, option_sets[0], 0, t_talib, (int) stocks[s].len);
    }

    // ---- SIMD runs (optional outputs off: NULL, 0) ----
    if (num_stocks >= 4) {
        size_t dlen = stocks[0].len;
        for (int i = 1; i < 4; i++) if (stocks[i].len < dlen) dlen = stocks[i].len;

        HomodyneDiscriminatorSimdCtx ctx = { .stocks = stocks, .data_len = dlen };
        TimingResult t = time_fn(bench_homodynediscriminator_simd_assets, &ctx, number, repeat, warmup);
        log_and_print("homodynediscriminator", "tulip_rs_ffi_c_simd_by_assets", "All", NULL, 0, t, (int) dlen);
    }
}
