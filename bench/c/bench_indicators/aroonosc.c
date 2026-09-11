// aroonosc (Aroon Oscillator) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a multi-input indicator with options.

typedef struct {
    const Stock *stock;
    double period;
} AroonOscCtx;

static void bench_aroonosc(void *ctx_) {
    AroonOscCtx *ctx = ctx_;
    const double *inputs[AROONOSC_INPUTS] = {ctx->stock->high, ctx->stock->low};
    double opts[AROONOSC_OPTIONS] = {ctx->period};
    bool optionals[2] = {true, true}; // aroon_down, aroon_up
    struct CIndicatorResult r = aroonosc_indicator(inputs, ctx->stock->len, opts, optionals, 2);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] aroonosc_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    aroonosc_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_aroonosc.rs's bench_c_aroonosc exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_aroonosc(void *ctx_) {
    AroonOscCtx *ctx = ctx_;
    double options[AROONOSC_OPTIONS] = {ctx->period};
    int start_index = ti_aroonosc_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_aroonosc_start returned negative index\n"); exit(1); }
    int output_len = (int) ctx->stock->len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[AROONOSC_INPUTS] = {ctx->stock->high, ctx->stock->low};
    double *outputs[1] = {output};
    int ret = ti_aroonosc((int) ctx->stock->len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_aroonosc returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib". Mirrors
// tulip_test/benches/benchmark_aroonosc.rs's bench_talib_aroonosc exactly.
// ---------------------------------------------------------------------------

static void bench_talib_aroonosc(void *ctx_) {
    AroonOscCtx *ctx = ctx_;
    int size = (int) ctx->stock->len;
    int start_index = TA_AROONOSC_Lookback((int) ctx->period);
    if (start_index < 0) { fprintf(stderr, "[error] TA_AROONOSC_Lookback returned negative index\n"); exit(1); }
    int output_len = size - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    int out_begin = 0, out_nb_element = 0;
    TA_RetCode ret = TA_AROONOSC(0, size - 1,
                                 ctx->stock->high, ctx->stock->low,
                                 (int) ctx->period,
                                 &out_begin, &out_nb_element, output);
    if (ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_AROONOSC returned %d\n", (int) ret); exit(1); }
    free(output);
}

static void run_aroonosc(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][1] = {{25.0}, {35.0}, {50.0}, {100.0}};
    printf("\n--- AROONOSC ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            AroonOscCtx ctx = {
                .stock = &stocks[s],
                .period = option_sets[o][0],
            };

            TimingResult t = time_fn(bench_aroonosc, &ctx, number, repeat, warmup);
            log_and_print("aroonosc", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 1, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_aroonosc, &ctx, number, repeat, warmup);
            log_and_print("aroonosc", "C_tulip", stocks[s].symbol, option_sets[o], 1, t_c, (int) stocks[s].len);

            TimingResult t_talib = time_fn(bench_talib_aroonosc, &ctx, number, repeat, warmup);
            log_and_print("aroonosc", "talib", stocks[s].symbol, option_sets[o], 1, t_talib, (int) stocks[s].len);
        }
    }
}
