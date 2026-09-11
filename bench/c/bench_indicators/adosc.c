// adosc (Chaikin A/D Oscillator) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating an indicator with multiple input series and
// options, with optional_outputs requested. The inputs array is built inside
// the timed region since constructing it is what a real C caller's hot loop looks like.

typedef struct {
    const Stock *stock;
    double short_period, long_period;
} AdoscCtx;

static void bench_adosc(void *ctx_) {
    AdoscCtx *ctx = ctx_;
    const double *inputs[ADOSC_INPUTS] = {ctx->stock->high, ctx->stock->low, ctx->stock->close, ctx->stock->volume};
    double opts[ADOSC_OPTIONS] = {ctx->short_period, ctx->long_period};
    bool optionals[3] = {true, true, true}; // short_ema, long_ema, ad
    struct CIndicatorResult r = adosc_indicator(inputs, ctx->stock->len, opts, optionals, 3);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] adosc_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    adosc_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_adosc.rs's bench_c_adosc exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_adosc(void *ctx_) {
    AdoscCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    double options[ADOSC_OPTIONS] = {ctx->short_period, ctx->long_period};
    int start_index = ti_adosc_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_adosc_start returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[ADOSC_INPUTS] = {ctx->stock->high, ctx->stock->low, ctx->stock->close, ctx->stock->volume};
    double *outputs[1] = {output};
    int ret = ti_adosc((int) len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_adosc returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib". Mirrors
// tulip_test/benches/benchmark_adosc.rs's bench_talib_adosc exactly.
// ---------------------------------------------------------------------------

static void bench_talib_adosc(void *ctx_) {
    AdoscCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    int start_index = TA_ADOSC_Lookback((int) ctx->short_period, (int) ctx->long_period);
    if (start_index < 0) { fprintf(stderr, "[error] TA_ADOSC_Lookback returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    int out_begin = 0, out_nb_element = 0;
    TA_RetCode ret = TA_ADOSC(0, (int) len - 1,
                              ctx->stock->high, ctx->stock->low, ctx->stock->close, ctx->stock->volume,
                              (int) ctx->short_period, (int) ctx->long_period,
                              &out_begin, &out_nb_element, output);
    if (ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_ADOSC returned %d\n", (int) ret); exit(1); }
    free(output);
}

static void run_adosc(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][2] = {{2.0, 5.0}, {6.0, 20.0}, {5.0, 15.0}, {10.0, 30.0}};
    printf("\n--- ADOSC ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            AdoscCtx ctx = {
                .stock = &stocks[s],
                .short_period = option_sets[o][0],
                .long_period = option_sets[o][1],
            };

            TimingResult t = time_fn(bench_adosc, &ctx, number, repeat, warmup);
            log_and_print("adosc", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 2, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_adosc, &ctx, number, repeat, warmup);
            log_and_print("adosc", "C_tulip", stocks[s].symbol, option_sets[o], 2, t_c, (int) stocks[s].len);

            TimingResult t_talib = time_fn(bench_talib_adosc, &ctx, number, repeat, warmup);
            log_and_print("adosc", "talib", stocks[s].symbol, option_sets[o], 2, t_talib, (int) stocks[s].len);
        }
    }
}
