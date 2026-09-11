// kvo (Klinger Volume Oscillator) -- tulip_rs_ffi extern "C" API.
//
// This is one of the three "template" indicators (alongside macd.c and
// stoch.c) that establish the pattern every other file under bench_indicators/
// follows. See ../README.md "Adding a new indicator to this harness".

typedef struct {
    const Stock *stock;
    double short_period, long_period;
} KvoCtx;

static void bench_kvo(void *ctx_) {
    KvoCtx *ctx = ctx_;
    const double *inputs[4] = {ctx->stock->high, ctx->stock->low, ctx->stock->close, ctx->stock->volume};
    double opts[2] = {ctx->short_period, ctx->long_period};
    struct CIndicatorResult r = kvo_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] kvo_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    kvo_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_kvo.rs's bench_c_kvo exactly (lookback
// computation + output allocation inside the timed region).
// ---------------------------------------------------------------------------

static void bench_tulipc_kvo(void *ctx_) {
    KvoCtx *ctx = ctx_;
    double options[2] = {ctx->short_period, ctx->long_period};
    int start_index = ti_kvo_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_kvo_start returned negative index\n"); exit(1); }
    int output_len = (int) ctx->stock->len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[4] = {ctx->stock->high, ctx->stock->low, ctx->stock->close, ctx->stock->volume};
    double *outputs[1] = {output};
    int ret = ti_kvo((int) ctx->stock->len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_kvo returned %d\n", ret); exit(1); }
    free(output);
}

static void run_kvo(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][2] = {{34.0, 55.0}, {20.0, 40.0}, {10.0, 30.0}, {5.0, 20.0}};
    printf("\n--- KVO ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            KvoCtx ctx = {.stock = &stocks[s], .short_period = option_sets[o][0], .long_period = option_sets[o][1]};

            TimingResult t = time_fn(bench_kvo, &ctx, number, repeat, warmup);
            log_and_print("kvo", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 2, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_kvo, &ctx, number, repeat, warmup);
            log_and_print("kvo", "C_tulip", stocks[s].symbol, option_sets[o], 2, t_c, (int) stocks[s].len);
        }
    }
}
