// pvi (Positive Volume Index) -- tulip_rs_ffi extern "C" API.
//
// This is one of the three "template" indicators (alongside macd.c and
// stoch.c) that establish the pattern every other file under bench_indicators/
// follows. See ../README.md "Adding a new indicator to this harness".

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    const double *inputs_buf; // close ++ volume, each stock->len long
} PviCtx;

static void bench_pvi(void *ctx_) {
    PviCtx *ctx = ctx_;
    const double *inputs[PVI_INPUTS] = {ctx->stock->close, ctx->stock->volume};
    struct CIndicatorResult r = pvi_indicator(inputs, ctx->stock->len, NULL, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] pvi_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    pvi_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_pvi.rs's bench_c_pvi exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_pvi(void *ctx_) {
    PviCtx *ctx = ctx_;
    double options[PVI_OPTIONS] = {};
    int start_index = ti_pvi_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_pvi_start returned negative index\n"); exit(1); }
    int output_len = (int) ctx->stock->len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[PVI_INPUTS] = {ctx->stock->close, ctx->stock->volume};
    double *outputs[1] = {output};
    int ret = ti_pvi((int) ctx->stock->len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_pvi returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib".
// Note: No TA-Lib equivalent for PVI.
// ---------------------------------------------------------------------------

static void run_pvi(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    printf("\n--- PVI ---\n");
    for (int s = 0; s < num_stocks; s++) {
        size_t len = stocks[s].len;
        double *inputs_buf = malloc(sizeof(double) * len * 2);
        memcpy(inputs_buf, stocks[s].close, sizeof(double) * len);
        memcpy(inputs_buf + len, stocks[s].volume, sizeof(double) * len);

        PviCtx ctx = {
            .stock = &stocks[s],
            .inputs_buf = inputs_buf,
        };

        TimingResult t = time_fn(bench_pvi, &ctx, number, repeat, warmup);
        log_and_print("pvi", "tulip_rs_ffi_c", stocks[s].symbol, NULL, 0, t, (int) stocks[s].len);

        TimingResult t_c = time_fn(bench_tulipc_pvi, &ctx, number, repeat, warmup);
        log_and_print("pvi", "C_tulip", stocks[s].symbol, NULL, 0, t_c, (int) stocks[s].len);

        // No TA-Lib comparison available

        free(inputs_buf);
    }
}
