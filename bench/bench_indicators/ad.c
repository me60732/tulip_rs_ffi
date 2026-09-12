// ad (Accumulation/Distribution) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating an indicator with multiple input series but
// no options and no requested optional outputs. The inputs array is built
// inside the timed region since constructing it is what a real C caller's
// hot loop looks like.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
} AdCtx;

static void bench_ad(void *ctx_) {
    AdCtx *ctx = ctx_;
    const double *inputs[AD_INPUTS] = {ctx->stock->high, ctx->stock->low, ctx->stock->close, ctx->stock->volume};
    struct CIndicatorResult r = ad_indicator(inputs, ctx->stock->len, NULL, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] ad_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    ad_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_ad.rs's bench_c_ad exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_ad(void *ctx_) {
    AdCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    int start_index = ti_ad_start(NULL);
    if (start_index < 0) { fprintf(stderr, "[error] ti_ad_start returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[AD_INPUTS] = {ctx->stock->high, ctx->stock->low, ctx->stock->close, ctx->stock->volume};
    double *outputs[1] = {output};
    int ret = ti_ad((int) len, inputs, NULL, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_ad returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib". Mirrors
// tulip_test/benches/benchmark_ad.rs's bench_talib_ad exactly.
// ---------------------------------------------------------------------------

static void bench_talib_ad(void *ctx_) {
    AdCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    int start_index = TA_AD_Lookback();
    if (start_index < 0) { fprintf(stderr, "[error] TA_AD_Lookback returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    int out_begin = 0, out_nb_element = 0;
    TA_RetCode ret = TA_AD(0, (int) len - 1,
                           ctx->stock->high, ctx->stock->low, ctx->stock->close, ctx->stock->volume,
                           &out_begin, &out_nb_element, output);
    if (ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_AD returned %d\n", (int) ret); exit(1); }
    free(output);
}

static void run_ad(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][AD_OPTIONS] = {{}};
    printf("\n--- AD ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            AdCtx ctx = {.stock = &stocks[s]};

            TimingResult t = time_fn(bench_ad, &ctx, number, repeat, warmup);
            log_and_print("ad", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 0, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_ad, &ctx, number, repeat, warmup);
            log_and_print("ad", "C_tulip", stocks[s].symbol, option_sets[o], 0, t_c, (int) stocks[s].len);

            TimingResult t_talib = time_fn(bench_talib_ad, &ctx, number, repeat, warmup);
            log_and_print("ad", "talib", stocks[s].symbol, option_sets[o], 0, t_talib, (int) stocks[s].len);
        }
    }
}
