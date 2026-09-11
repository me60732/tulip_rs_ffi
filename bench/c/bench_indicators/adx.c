// adx (Average Directional Movement Index) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a multi-input indicator with options and
// optional_outputs requested. The inputs array is built inside the timed region.

typedef struct {
    const Stock *stock;
    double period;
} AdxCtx;

static void bench_adx(void *ctx_) {
    AdxCtx *ctx = ctx_;
    const double *inputs[3] = {ctx->stock->high, ctx->stock->low, ctx->stock->close};
    double opts[1] = {ctx->period};
    bool optionals[3] = {true, true, true}; // dx, atr, tr
    struct CIndicatorResult r = adx_indicator(inputs, ctx->stock->len, opts, optionals, 3);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] adx_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    adx_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_adx.rs's bench_c_adx exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_adx(void *ctx_) {
    AdxCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    double options[1] = {ctx->period};
    int start_index = ti_adx_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_adx_start returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[3] = {ctx->stock->high, ctx->stock->low, ctx->stock->close};
    double *outputs[1] = {output};
    int ret = ti_adx((int) len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_adx returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib". Mirrors
// tulip_test/benches/benchmark_adx.rs's bench_talib_adx exactly.
// ---------------------------------------------------------------------------

static void bench_talib_adx(void *ctx_) {
    AdxCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    int start_index = TA_ADX_Lookback((int) ctx->period);
    if (start_index < 0) { fprintf(stderr, "[error] TA_ADX_Lookback returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    int out_begin = 0, out_nb_element = 0;
    TA_RetCode ret = TA_ADX(0, (int) len - 1,
                            ctx->stock->high, ctx->stock->low, ctx->stock->close,
                            (int) ctx->period,
                            &out_begin, &out_nb_element, output);
    if (ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_ADX returned %d\n", (int) ret); exit(1); }
    free(output);
}

static void run_adx(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][1] = {{5.0}, {14.0}, {24.0}, {30.0}};
    printf("\n--- ADX ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            AdxCtx ctx = {
                .stock = &stocks[s],
                .period = option_sets[o][0],
            };

            TimingResult t = time_fn(bench_adx, &ctx, number, repeat, warmup);
            log_and_print("adx", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 1, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_adx, &ctx, number, repeat, warmup);
            log_and_print("adx", "C_tulip", stocks[s].symbol, option_sets[o], 1, t_c, (int) stocks[s].len);

            TimingResult t_talib = time_fn(bench_talib_adx, &ctx, number, repeat, warmup);
            log_and_print("adx", "talib", stocks[s].symbol, option_sets[o], 1, t_talib, (int) stocks[s].len);
        }
    }
}
