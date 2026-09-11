// bbands (Bollinger Bands) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating an indicator with a single input, options.

typedef struct {
    const Stock *stock;
    double period, std_dev;
} BBandsCtx;

static void bench_bbands(void *ctx_) {
    BBandsCtx *ctx = ctx_;
    const double *inputs[1] = {ctx->stock->close};
    double opts[2] = {ctx->period, ctx->std_dev};
    struct CIndicatorResult r = bbands_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] bbands_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    bbands_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_bbands.rs's bench_c_bbands exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_bbands(void *ctx_) {
    BBandsCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    double options[2] = {ctx->period, ctx->std_dev};
    int start_index = ti_bbands_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_bbands_start returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    double *lower = malloc(sizeof(double) * (size_t) output_len);
    double *middle = malloc(sizeof(double) * (size_t) output_len);
    double *upper = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[1] = {ctx->stock->close};
    double *outputs[3] = {lower, middle, upper};
    int ret = ti_bbands((int) len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_bbands returned %d\n", ret); exit(1); }
    free(lower);
    free(middle);
    free(upper);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib". Mirrors
// tulip_test/benches/benchmark_bbands.rs's bench_talib_bbands exactly.
// ---------------------------------------------------------------------------

static void bench_talib_bbands(void *ctx_) {
    BBandsCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    int start_index = TA_BBANDS_Lookback((int) ctx->period, ctx->std_dev, ctx->std_dev, TA_MAType_SMA);
    if (start_index < 0) { fprintf(stderr, "[error] TA_BBANDS_Lookback returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    double *upper = malloc(sizeof(double) * (size_t) output_len);
    double *middle = malloc(sizeof(double) * (size_t) output_len);
    double *lower = malloc(sizeof(double) * (size_t) output_len);
    int out_begin = 0, out_nb_element = 0;
    TA_RetCode ret = TA_BBANDS(0, (int) len - 1,
                               ctx->stock->close,
                               (int) ctx->period, ctx->std_dev, ctx->std_dev, TA_MAType_SMA,
                               &out_begin, &out_nb_element, upper, middle, lower);
    if (ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_BBANDS returned %d\n", (int) ret); exit(1); }
    free(lower);
    free(middle);
    free(upper);
}

static void run_bbands(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][2] = {{5.0, 2.0}, {14.0, 2.0}, {20.0, 2.0}, {50.0, 2.0}};
    printf("\n--- BBANDS ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            BBandsCtx ctx = {
                .stock = &stocks[s],
                .period = option_sets[o][0],
                .std_dev = option_sets[o][1],
            };

            TimingResult t = time_fn(bench_bbands, &ctx, number, repeat, warmup);
            log_and_print("bbands", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 2, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_bbands, &ctx, number, repeat, warmup);
            log_and_print("bbands", "C_tulip", stocks[s].symbol, option_sets[o], 2, t_c, (int) stocks[s].len);

            TimingResult t_talib = time_fn(bench_talib_bbands, &ctx, number, repeat, warmup);
            log_and_print("bbands", "talib", stocks[s].symbol, option_sets[o], 2, t_talib, (int) stocks[s].len);
        }
    }
}
