// vidya (Variable Index Dynamic Average) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a single-input indicator with 3 options
// and optional outputs supported.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    double short_period, long_period, alpha;
} VidyaCtx;

static void bench_vidya(void *ctx_) {
    VidyaCtx *ctx = ctx_;
    const double *inputs[VIDYA_INPUTS] = {ctx->stock->close};
    double opts[VIDYA_OPTIONS] = {ctx->short_period, ctx->long_period, ctx->alpha};
    struct CIndicatorResult r = vidya_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] vidya_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    vidya_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_vidya.rs's bench_c_vidya exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_vidya(void *ctx_) {
    VidyaCtx *ctx = ctx_;
    double options[VIDYA_OPTIONS] = {ctx->short_period, ctx->long_period, ctx->alpha};
    int start_index = ti_vidya_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_vidya_start returned negative index\n"); exit(1); }
    int output_len = (int) ctx->stock->len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[VIDYA_INPUTS] = {ctx->stock->close};
    double *outputs[1] = {output};
    int ret = ti_vidya((int) ctx->stock->len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_vidya returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- not available. TA-Lib has no VIDYA (Variable Index
// Dynamic Average) function. TA_VAR (Variance) is a different, unrelated
// calculation, so it is not used as a stand-in here.
// ---------------------------------------------------------------------------

static void run_vidya(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][VIDYA_OPTIONS] = {
        {2.0, 5.0, 0.2}, {5.0, 20.0, 0.2}, {9.0, 30.0, 0.2}, {12.0, 26.0, 0.1}};
    printf("\n--- VIDYA ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            VidyaCtx ctx = {
                .stock = &stocks[s],
                .short_period = option_sets[o][0],
                .long_period = option_sets[o][1],
                .alpha = option_sets[o][2],
            };

            TimingResult t = time_fn(bench_vidya, &ctx, number, repeat, warmup);
            log_and_print("vidya", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 3, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_vidya, &ctx, number, repeat, warmup);
            log_and_print("vidya", "C_tulip", stocks[s].symbol, option_sets[o], 3, t_c, (int) stocks[s].len);

            // No TA-Lib comparison available
        }
    }
}
