// wilders (Wilders Smoothing) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a single-input, single-option indicator with
// optional_outputs requested.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    double period;
} WildersCtx;

static void bench_wilders(void *ctx_) {
    WildersCtx *ctx = ctx_;
    const double *inputs[WILDERS_INPUTS] = {ctx->stock->close};
    double opts[WILDERS_OPTIONS] = {ctx->period};
    struct CIndicatorResult r = wilders_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] wilders_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    wilders_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_wilders.rs's bench_c_wilders exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_wilders(void *ctx_) {
    WildersCtx *ctx = ctx_;
    double options[1] = {ctx->period};
    int start_index = ti_wilders_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_wilders_start returned negative index\n"); exit(1); }
    int output_len = (int) ctx->stock->len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[1] = {ctx->stock->close};
    double *outputs[1] = {output};
    int ret = ti_wilders((int) ctx->stock->len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_wilders returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib". Mirrors
// tulip_test/benches/benchmark_wilders.rs's bench_talib_wilders exactly.
//
// NOTE: Wilders has no direct TA-Lib equivalent. The closest is RMA (Rogue
// Moving Average) in pandas_ta, but this is not part of the official TA-Lib
// library. We omit the TA-Lib benchmark for wilders.
// ---------------------------------------------------------------------------

static void run_wilders(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][WILDERS_OPTIONS] = {{5.0}, {14.0}, {20.0}, {50.0}};
    printf("\n--- WILDERS ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            WildersCtx ctx = {.stock = &stocks[s], .period = option_sets[o][0]};

            TimingResult t = time_fn(bench_wilders, &ctx, number, repeat, warmup);
            log_and_print("wilders", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 1, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_wilders, &ctx, number, repeat, warmup);
            log_and_print("wilders", "C_tulip", stocks[s].symbol, option_sets[o], 1, t_c, (int) stocks[s].len);
        }
    }
}
