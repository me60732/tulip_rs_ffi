// marketfi (Market Facilitation Index) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating an indicator with multiple input series but
// no options and no requested optional outputs. The inputs array is built
// inside the timed region since constructing it is what a real C caller's
// hot loop looks like.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
} MarketfiCtx;

static void bench_marketfi(void *ctx_) {
    MarketfiCtx *ctx = ctx_;
    const double *inputs[MARKETFI_INPUTS] = {ctx->stock->high, ctx->stock->low, ctx->stock->volume};
    struct CIndicatorResult r = marketfi_indicator(inputs, ctx->stock->len, NULL, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] marketfi_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    marketfi_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_marketfi.rs's bench_c_marketfi exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_marketfi(void *ctx_) {
    MarketfiCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    double options[0] = {};
    int start_index = ti_marketfi_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_marketfi_start returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[MARKETFI_INPUTS] = {ctx->stock->high, ctx->stock->low, ctx->stock->volume};
    double *outputs[1] = {output};
    int ret = ti_marketfi((int) len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_marketfi returned %d\n", ret); exit(1); }
    free(output);
}

static void run_marketfi(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][MARKETFI_OPTIONS] = {{}};
    printf("\n--- MARKETFI ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            MarketfiCtx ctx = {.stock = &stocks[s]};

            TimingResult t = time_fn(bench_marketfi, &ctx, number, repeat, warmup);
            log_and_print("marketfi", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], MARKETFI_OPTIONS, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_marketfi, &ctx, number, repeat, warmup);
            log_and_print("marketfi", "C_tulip", stocks[s].symbol, option_sets[o], MARKETFI_OPTIONS, t_c, (int) stocks[s].len);

            // No talib comparison available
        }
    }
}
