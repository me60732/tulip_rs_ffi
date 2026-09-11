// highpass (Ehlers High Pass Filter) -- tulip_rs_ffi extern "C" API.
//
// This is one of the three "template" indicators (alongside macd.c and
// stoch.c) that establish the pattern every other file under bench_indicators/
// follows. See ../README.md "Adding a new indicator to this harness".

typedef struct {
    const Stock *stock;
    double period;
} HighPassCtx;

static void bench_highpass(void *ctx_) {
    HighPassCtx *ctx = ctx_;
    const double *inputs[1] = {ctx->stock->close};
    double opts[1] = {ctx->period};
    struct CIndicatorResult r = highpass_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] highpass_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    highpass_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- not available. No ti_highpass function.
// ---------------------------------------------------------------------------

static void run_highpass(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][1] = {{20.0}, {40.0}, {60.0}, {80.0}};
    printf("\n--- HIGHPASS ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            HighPassCtx ctx = {.stock = &stocks[s], .period = option_sets[o][0]};

            TimingResult t = time_fn(bench_highpass, &ctx, number, repeat, warmup);
            log_and_print("highpass", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 1, t, (int) stocks[s].len);

            // No C_tulip comparison - no ti_highpass implementation
        }
    }
}
