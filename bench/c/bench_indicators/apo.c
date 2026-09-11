// apo (Absolute Price Oscillator) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a single-input indicator with multiple options,
// with optional_outputs requested.

typedef struct {
    const Stock *stock;
    double short_period, long_period;
} ApoCtx;

static void bench_apo(void *ctx_) {
    ApoCtx *ctx = ctx_;
    const double *inputs[1] = {ctx->stock->close};
    double opts[2] = {ctx->short_period, ctx->long_period};
    bool optionals[2] = {true, true}; // short_ema, long_ema
    struct CIndicatorResult r = apo_indicator(inputs, ctx->stock->len, opts, optionals, 2);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] apo_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    apo_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_apo.rs's bench_c_apo exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_apo(void *ctx_) {
    ApoCtx *ctx = ctx_;
    double options[2] = {ctx->short_period, ctx->long_period};
    int start_index = ti_apo_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_apo_start returned negative index\n"); exit(1); }
    int output_len = (int) ctx->stock->len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[1] = {ctx->stock->close};
    double *outputs[1] = {output};
    int ret = ti_apo((int) ctx->stock->len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_apo returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib". Mirrors
// tulip_test/benches/benchmark_apo.rs's bench_talib_apo exactly.
// ---------------------------------------------------------------------------

static void bench_talib_apo(void *ctx_) {
    ApoCtx *ctx = ctx_;
    int size = (int) ctx->stock->len;
    int start_index = TA_APO_Lookback((int) ctx->short_period, (int) ctx->long_period, TA_MAType_SMA);
    if (start_index < 0) { fprintf(stderr, "[error] TA_APO_Lookback returned negative index\n"); exit(1); }
    int output_len = size - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    int out_begin = 0, out_nb_element = 0;
    TA_RetCode ret = TA_APO(0, size - 1, ctx->stock->close,
                            (int) ctx->short_period, (int) ctx->long_period,
                            TA_MAType_SMA, &out_begin, &out_nb_element, output);
    if (ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_APO returned %d\n", (int) ret); exit(1); }
    free(output);
}

static void run_apo(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][2] = {{5.0, 13.0}, {8.0, 18.0}, {12.0, 26.0}, {3.0, 9.0}};
    printf("\n--- APO ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            ApoCtx ctx = {
                .stock = &stocks[s],
                .short_period = option_sets[o][0],
                .long_period = option_sets[o][1],
            };

            TimingResult t = time_fn(bench_apo, &ctx, number, repeat, warmup);
            log_and_print("apo", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 2, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_apo, &ctx, number, repeat, warmup);
            log_and_print("apo", "C_tulip", stocks[s].symbol, option_sets[o], 2, t_c, (int) stocks[s].len);

            TimingResult t_talib = time_fn(bench_talib_apo, &ctx, number, repeat, warmup);
            log_and_print("apo", "talib", stocks[s].symbol, option_sets[o], 2, t_talib, (int) stocks[s].len);
        }
    }
}
