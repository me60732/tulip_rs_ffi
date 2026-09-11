// fisher (Fisher Transform) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a multi-input indicator with options and
// multiple outputs (fisher, fisher_signal).

typedef struct {
    const Stock *stock;
    double period;
} FisherCtx;

static void bench_fisher(void *ctx_) {
    FisherCtx *ctx = ctx_;
    const double *inputs[2] = {ctx->stock->high, ctx->stock->low};
    double opts[1] = {ctx->period};
    struct CIndicatorResult r = fisher_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] fisher_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    fisher_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_fisher.rs's bench_c_fisher exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_fisher(void *ctx_) {
    FisherCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    double options[1] = {ctx->period};
    int start_index = ti_fisher_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_fisher_start returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    double *output_fisher = malloc(sizeof(double) * (size_t) output_len);
    double *output_signal = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[2] = {ctx->stock->high, ctx->stock->low};
    double *outputs[2] = {output_fisher, output_signal};
    int ret = ti_fisher((int) len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_fisher returned %d\n", ret); exit(1); }
    free(output_fisher);
    free(output_signal);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- not available. TA-Lib has no Fisher Transform function.
// ---------------------------------------------------------------------------

static void run_fisher(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][1] = {{5.0}, {10.0}, {14.0}, {20.0}};
    printf("\n--- FISHER ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            FisherCtx ctx = {.stock = &stocks[s], .period = option_sets[o][0]};

            TimingResult t = time_fn(bench_fisher, &ctx, number, repeat, warmup);
            log_and_print("fisher", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 1, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_fisher, &ctx, number, repeat, warmup);
            log_and_print("fisher", "C_tulip", stocks[s].symbol, option_sets[o], 1, t_c, (int) stocks[s].len);

            // fisher has no talib comparison
        }
    }
}
