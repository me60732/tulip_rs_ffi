// hilberttransform (Ehlers Hilbert Transform) -- tulip_rs_ffi extern "C" API.
//
// This is one of the three "template" indicators (alongside macd.c and
// stoch.c) that establish the pattern every other file under bench_indicators/
// follows. See ../README.md "Adding a new indicator to this harness".

typedef struct {
    const Stock *stock;
    double ss_period, hp_period;
} HilbertTransformCtx;

static void bench_hilberttransform(void *ctx_) {
    HilbertTransformCtx *ctx = ctx_;
    const double *inputs[1] = {ctx->stock->close};
    double opts[2] = {ctx->ss_period, ctx->hp_period};
    struct CIndicatorResult r = hilberttransform_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] hilberttransform_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    hilberttransform_state_free(r.state);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib". Mirrors
// tulip_test/benches/benchmark_hilberttransform.rs's bench_talib_ht_phasor.
// TA-HT_PHASOR outputs in_phase and quadrature, which match our hilberttransform
// outputs [in_phase, quadrature].
// ---------------------------------------------------------------------------

static void bench_talib_hilberttransform(void *ctx_) {
    HilbertTransformCtx *ctx = ctx_;
    int size = (int) ctx->stock->len;
    int start_index = TA_HT_PHASOR_Lookback();
    if (start_index < 0) { fprintf(stderr, "[error] TA_HT_PHASOR_Lookback returned negative index\n"); exit(1); }
    int output_len = size - start_index;
    
    double *out_in_phase = malloc(sizeof(double) * (size_t) output_len);
    double *out_quadrature = malloc(sizeof(double) * (size_t) output_len);
    
    int out_begin = 0, out_nb_element = 0;
    TA_RetCode ret = TA_HT_PHASOR(
        0,
        size - 1,
        ctx->stock->close,
        &out_begin,
        &out_nb_element,
        out_in_phase,
        out_quadrature
    );
    
    if (ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_HT_PHASOR returned %d\n", (int) ret); exit(1); }
    
    free(out_in_phase);
    free(out_quadrature);
}

static void run_hilberttransform(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][2] = {{10.0, 20.0}, {15.0, 30.0}, {20.0, 40.0}, {25.0, 50.0}};
    printf("\n--- HILBERTTRANSFORM ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            HilbertTransformCtx ctx = {.stock = &stocks[s], .ss_period = option_sets[o][0], .hp_period = option_sets[o][1]};

            TimingResult t = time_fn(bench_hilberttransform, &ctx, number, repeat, warmup);
            log_and_print("hilberttransform", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 2, t, (int) stocks[s].len);

            // No C_tulip comparison - no ti_hilberttransform implementation

            TimingResult t_talib = time_fn(bench_talib_hilberttransform, &ctx, number, repeat, warmup);
            log_and_print("hilberttransform", "talib", stocks[s].symbol, option_sets[o], 2, t_talib, (int) stocks[s].len);
        }
    }
}
