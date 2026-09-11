// elderray (Elder-ray Index) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a multi-input indicator with options and
// multiple outputs (bull, bear) plus optional output (ema).

typedef struct {
    const Stock *stock;
    double period;
} ElderrayCtx;

static void bench_elderray(void *ctx_) {
    ElderrayCtx *ctx = ctx_;
    const double *inputs[3] = {ctx->stock->high, ctx->stock->low, ctx->stock->close};
    double opts[1] = {ctx->period};
    struct CIndicatorResult r = elderray_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] elderray_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    elderray_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_elderray.rs's bench_c_elderray exactly.
//
// Note: Elder-ray is computed as bull = high - ema, bear = low - ema,
// where ema is calculated from close prices. The C benchmark uses ti_ema
// and computes the bull/bear values manually to match the Rust implementation.
// ---------------------------------------------------------------------------

static void bench_tulipc_elderray(void *ctx_) {
    ElderrayCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    double options[1] = {ctx->period};
    
    // Get lookback from ti_ema
    int start_index = ti_ema_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_ema_start returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    
    double *ema_output = malloc(sizeof(double) * (size_t) output_len);
    double *bull_output = malloc(sizeof(double) * (size_t) output_len);
    double *bear_output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[1] = {ctx->stock->close};
    double *outputs[1] = {ema_output};
    
    // Calculate EMA from close
    int ret = ti_ema((int) len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_ema returned %d\n", ret); exit(1); }
    
    // Compute bull and bear: bull = high - ema, bear = low - ema
    for (int i = 0; i < output_len; i++) {
        bull_output[i] = ctx->stock->high[start_index + i] - ema_output[i];
        bear_output[i] = ctx->stock->low[start_index + i] - ema_output[i];
    }
    
    free(ema_output);
    free(bull_output);
    free(bear_output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- not available. TA-Lib has no Elder-ray function.
// ---------------------------------------------------------------------------

static void run_elderray(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][1] = {{5.0}, {13.0}, {26.0}, {30.0}};
    printf("\n--- ELDERRAY ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            ElderrayCtx ctx = {.stock = &stocks[s], .period = option_sets[o][0]};

            TimingResult t = time_fn(bench_elderray, &ctx, number, repeat, warmup);
            log_and_print("elderray", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 1, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_elderray, &ctx, number, repeat, warmup);
            log_and_print("elderray", "C_tulip", stocks[s].symbol, option_sets[o], 1, t_c, (int) stocks[s].len);

            // No talib comparison available
        }
    }
}
