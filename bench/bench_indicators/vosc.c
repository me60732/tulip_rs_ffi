// vosc (Volume Oscillator) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a single-input indicator with multiple options
// and optional_outputs requested.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    double short_period, long_period;
} VoscCtx;

static void bench_vosc(void *ctx_) {
    VoscCtx *ctx = ctx_;
    const double *inputs[VOSC_INPUTS] = {ctx->stock->volume};
    double opts[VOSC_OPTIONS] = {ctx->short_period, ctx->long_period};
    struct CIndicatorResult r = vosc_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] vosc_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    vosc_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_vosc.rs's bench_c_vosc exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_vosc(void *ctx_) {
    VoscCtx *ctx = ctx_;
    double options[VOSC_OPTIONS] = {ctx->short_period, ctx->long_period};
    int start_index = ti_vosc_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_vosc_start returned negative index\n"); exit(1); }
    int output_len = (int) ctx->stock->len - start_index;
    double *vosc_line = malloc(sizeof(double) * (size_t) output_len);
    double *short_sma = malloc(sizeof(double) * (size_t) output_len);
    double *long_sma = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[VOSC_INPUTS] = {ctx->stock->volume};
    double *outputs[3] = {vosc_line, short_sma, long_sma};
    int ret = ti_vosc((int) ctx->stock->len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_vosc returned %d\n", ret); exit(1); }
    free(vosc_line);
    free(short_sma);
    free(long_sma);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- not available. The vendored Tulip Indicators C library
// has no TA_Lib equivalent for vosc.
// ---------------------------------------------------------------------------

static void run_vosc(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][VOSC_OPTIONS] = {{5.0, 20.0}, {9.0, 26.0}, {12.0, 26.0}, {3.0, 10.0}};
    printf("\n--- VOSC ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            VoscCtx ctx = {
                .stock = &stocks[s],
                .short_period = option_sets[o][0],
                .long_period = option_sets[o][1],
            };

            TimingResult t = time_fn(bench_vosc, &ctx, number, repeat, warmup);
            log_and_print("vosc", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], VOSC_OPTIONS, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_vosc, &ctx, number, repeat, warmup);
            log_and_print("vosc", "C_tulip", stocks[s].symbol, option_sets[o], VOSC_OPTIONS, t_c, (int) stocks[s].len);

            // No TA-Lib comparison available
        }
    }
}
