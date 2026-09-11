// adxr (Average Directional Movement Rating) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a multi-input indicator with options and
// optional_outputs requested. The inputs array is built inside the timed region.

typedef struct {
    const Stock *stock;
    double period;
} AdxrCtx;

static void bench_adxr(void *ctx_) {
    AdxrCtx *ctx = ctx_;
    const double *inputs[3] = {ctx->stock->high, ctx->stock->low, ctx->stock->close};
    double opts[1] = {ctx->period};
    bool optionals[4] = {true, true, true, true}; // adx, dx, atr, tr
    struct CIndicatorResult r = adxr_indicator(inputs, ctx->stock->len, opts, optionals, 4);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] adxr_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    adxr_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_adxr.rs's bench_c_adxr exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_adxr(void *ctx_) {
    AdxrCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    double options[1] = {ctx->period};
    int start_index = ti_adxr_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_adxr_start returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[3] = {ctx->stock->high, ctx->stock->low, ctx->stock->close};
    double *outputs[1] = {output};
    int ret = ti_adxr((int) len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_adxr returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib". Mirrors
// tulip_test/benches/benchmark_adxr.rs's bench_talib_adxr exactly.
// ---------------------------------------------------------------------------

static void bench_talib_adxr(void *ctx_) {
    AdxrCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    int start_index = TA_ADXR_Lookback((int) ctx->period);
    if (start_index < 0) { fprintf(stderr, "[error] TA_ADXR_Lookback returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    int out_begin = 0, out_nb_element = 0;
    TA_RetCode ret = TA_ADXR(0, (int) len - 1,
                             ctx->stock->high, ctx->stock->low, ctx->stock->close,
                             (int) ctx->period,
                             &out_begin, &out_nb_element, output);
    if (ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_ADXR returned %d\n", (int) ret); exit(1); }
    free(output);
}

static void run_adxr(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][1] = {{5.0}, {14.0}, {24.0}, {30.0}};
    printf("\n--- ADXR ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            AdxrCtx ctx = {
                .stock = &stocks[s],
                .period = option_sets[o][0],
            };

            TimingResult t = time_fn(bench_adxr, &ctx, number, repeat, warmup);
            log_and_print("adxr", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 1, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_adxr, &ctx, number, repeat, warmup);
            log_and_print("adxr", "C_tulip", stocks[s].symbol, option_sets[o], 1, t_c, (int) stocks[s].len);

            TimingResult t_talib = time_fn(bench_talib_adxr, &ctx, number, repeat, warmup);
            log_and_print("adxr", "talib", stocks[s].symbol, option_sets[o], 1, t_talib, (int) stocks[s].len);
        }
    }
}
