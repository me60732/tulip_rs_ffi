// adaptivemsw (Adaptive Mesa Sine Wave) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating an indicator with a single input but no
// options, with optional_outputs requested (passing bool array of true).

typedef struct {
    const Stock *stock;
} AdaptiveMSWCtx;

static void bench_adaptivemsw(void *ctx_) {
    AdaptiveMSWCtx *ctx = ctx_;
    const double *inputs[ADAPTIVEMSW_INPUTS] = {ctx->stock->close};
    bool optionals[1] = {true}; // dc_period
    struct CIndicatorResult r = adaptivemsw_indicator(inputs, ctx->stock->len, NULL, optionals, 1);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] adaptivemsw_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    adaptivemsw_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip".
// NOTE: No ti_adaptivemsw_start/ti_adaptivemsw pair exists in tulip_test/src/c_bindings.rs.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib".
// NOTE: TA-Lib does not provide an equivalent for adaptivemsw.
// ---------------------------------------------------------------------------

static void run_adaptivemsw(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][0] = {{}};
    printf("\n--- ADAPTIVEMSW ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            AdaptiveMSWCtx ctx = {.stock = &stocks[s]};

            TimingResult t = time_fn(bench_adaptivemsw, &ctx, number, repeat, warmup);
            log_and_print("adaptivemsw", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 0, t, (int) stocks[s].len);

            // No C_tulip comparison available
            // No talib comparison available
        }
    }
}
