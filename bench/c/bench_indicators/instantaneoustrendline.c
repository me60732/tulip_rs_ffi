// instantaneoustrendline (Instantaneous Trendline) -- tulip_rs_ffi extern "C" API.
//
// This indicator has no C_tulip comparison because the Tulip Indicators C library
// does not implement Instantaneous Trendline. It has a talib comparison using
// TA-Lib's HT_TRENDLINE, which implements a different algorithm (variable-length SMA + 4-bar WMA)
// than Ehlers' 2-pole IIR used by tulip-rs; this benchmark measures throughput only.

typedef struct {
    const Stock *stock;
} InstantaneousTrendlineCtx;

static void bench_instantaneoustrendline(void *ctx_) {
    InstantaneousTrendlineCtx *ctx = ctx_;
    const double *inputs[1] = {ctx->stock->close};
    double opts[0] = {};
    struct CIndicatorResult r = instantaneoustrendline_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] instantaneoustrendline_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    instantaneoustrendline_state_free(r.state);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib". Mirrors
// tulip_test/benches/benchmark_instantaneoustrendline.rs's bench_talib_ht_trendline.
// Note: HT_TRENDLINE uses a different algorithm than Ehlers' 2-pole IIR;
// this benchmark measures throughput only, not correctness comparison.
// ---------------------------------------------------------------------------

static void bench_talib_ht_trendline(void *ctx_) {
    InstantaneousTrendlineCtx *ctx = ctx_;
    int size = (int) ctx->stock->len;
    int start_index = TA_HT_TRENDLINE_Lookback();
    if (start_index < 0) { fprintf(stderr, "[error] TA_HT_TRENDLINE_Lookback returned negative index\n"); exit(1); }
    int output_len = size - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    int out_begin = 0, out_nb_element = 0;
    TA_RetCode ret = TA_HT_TRENDLINE(0, size - 1, ctx->stock->close, &out_begin, &out_nb_element, output);
    if (ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_HT_TRENDLINE returned %d\n", (int) ret); exit(1); }
    free(output);
}

static void run_instantaneoustrendline(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][0] = {{}};
    printf("\n--- INSTANTANEoustrendline ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 1; o++) {
            InstantaneousTrendlineCtx ctx = {.stock = &stocks[s]};

            TimingResult t = time_fn(bench_instantaneoustrendline, &ctx, number, repeat, warmup);
            log_and_print("instantaneoustrendline", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 0, t, (int) stocks[s].len);

            TimingResult t_talib = time_fn(bench_talib_ht_trendline, &ctx, number, repeat, warmup);
            log_and_print("instantaneoustrendline", "talib", stocks[s].symbol, option_sets[o], 0, t_talib, (int) stocks[s].len);
        }
    }
}
