// qstick (Qstick) -- tulip_rs_ffi extern "C" API.
//
// This is one of the three "template" indicators (alongside macd.c and
// stoch.c) that establish the pattern every other file under bench_indicators/
// follows. See ../README.md "Adding a new indicator to this harness".

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    const double *inputs_buf; // open ++ close, each stock->len long
    double period;
} QstickCtx;

static void bench_qstick(void *ctx_) {
    QstickCtx *ctx = ctx_;
    const double *inputs[QSTICK_INPUTS] = {ctx->stock->open, ctx->stock->close};
    double opts[QSTICK_OPTIONS] = {ctx->period};
    struct CIndicatorResult r = qstick_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] qstick_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    qstick_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_qstick.rs's bench_c_qstick exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_qstick(void *ctx_) {
    QstickCtx *ctx = ctx_;
    double options[QSTICK_OPTIONS] = {ctx->period};
    int start_index = ti_qstick_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_qstick_start returned negative index\n"); exit(1); }
    int output_len = (int) ctx->stock->len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[QSTICK_INPUTS] = {ctx->stock->open, ctx->stock->close};
    double *outputs[1] = {output};
    int ret = ti_qstick((int) ctx->stock->len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_qstick returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib".
// Note: No TA-Lib equivalent for Qstick.
// ---------------------------------------------------------------------------

static void run_qstick(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][QSTICK_OPTIONS] = {{5.0}, {8.0}, {14.0}, {20.0}};
    printf("\n--- QSTICK ---\n");
    for (int s = 0; s < num_stocks; s++) {
        size_t len = stocks[s].len;
        double *inputs_buf = malloc(sizeof(double) * len * 2);
        memcpy(inputs_buf, stocks[s].open, sizeof(double) * len);
        memcpy(inputs_buf + len, stocks[s].close, sizeof(double) * len);

        for (int o = 0; o < 4; o++) {
            QstickCtx ctx = {
                .stock = &stocks[s],
                .inputs_buf = inputs_buf,
                .period = option_sets[o][0],
            };

            TimingResult t = time_fn(bench_qstick, &ctx, number, repeat, warmup);
            log_and_print("qstick", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], QSTICK_OPTIONS, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_qstick, &ctx, number, repeat, warmup);
            log_and_print("qstick", "C_tulip", stocks[s].symbol, option_sets[o], QSTICK_OPTIONS, t_c, (int) stocks[s].len);

            // No TA-Lib comparison available
        }

        free(inputs_buf);
    }
}
