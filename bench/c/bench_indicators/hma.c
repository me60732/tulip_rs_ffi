// hma (Hull Moving Average) -- tulip_rs_ffi extern "C" API.
//
// This is one of the three "template" indicators (alongside macd.c and
// stoch.c) that establish the pattern every other file under bench_indicators/
// follows. See ../README.md "Adding a new indicator to this harness".

typedef struct {
    const Stock *stock;
    double period;
} HmaCtx;

static void bench_hma(void *ctx_) {
    HmaCtx *ctx = ctx_;
    const double *inputs[1] = {ctx->stock->close};
    double opts[1] = {ctx->period};
    struct CIndicatorResult r = hma_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] hma_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    hma_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_hma.rs's bench_c_hma exactly (lookback
// computation + output allocation inside the timed region).
// ---------------------------------------------------------------------------

static void bench_tulipc_hma(void *ctx_) {
    HmaCtx *ctx = ctx_;
    double options[1] = {ctx->period};
    int start_index = ti_hma_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_hma_start returned negative index\n"); exit(1); }
    int output_len = (int) ctx->stock->len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[1] = {ctx->stock->close};
    double *outputs[1] = {output};
    int ret = ti_hma((int) ctx->stock->len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_hma returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- not available. TA-Lib has no HMA equivalent.
// ---------------------------------------------------------------------------

static void run_hma(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][1] = {{5.0}, {14.0}, {20.0}, {50.0}};
    printf("\n--- HMA ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            HmaCtx ctx = {.stock = &stocks[s], .period = option_sets[o][0]};

            TimingResult t = time_fn(bench_hma, &ctx, number, repeat, warmup);
            log_and_print("hma", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 1, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_hma, &ctx, number, repeat, warmup);
            log_and_print("hma", "C_tulip", stocks[s].symbol, option_sets[o], 1, t_c, (int) stocks[s].len);

            // No talib comparison - HMA is unique to Tulip
        }
    }
}
