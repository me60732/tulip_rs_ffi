// keltnerchannel (Keltner Channel) -- tulip_rs_ffi extern "C" API.
//
// This indicator has no C_tulip or talib comparison because neither the Tulip
// Indicators C library nor TA-Lib implement Keltner Channel.

typedef struct {
    const Stock *stock;
    double period, step;
} KeltnerChannelCtx;

static void bench_keltnerchannel(void *ctx_) {
    KeltnerChannelCtx *ctx = ctx_;
    const double *inputs[3] = {ctx->stock->high, ctx->stock->low, ctx->stock->close};
    double opts[2] = {ctx->period, ctx->step};
    struct CIndicatorResult r = keltnerchannel_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] keltnerchannel_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    keltnerchannel_state_free(r.state);
}

static void run_keltnerchannel(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][2] = {{20.0, 2.0}, {20.0, 1.5}, {14.0, 2.0}, {10.0, 1.5}};
    printf("\n--- KELTNERCHANNEL ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            KeltnerChannelCtx ctx = {.stock = &stocks[s], .period = option_sets[o][0], .step = option_sets[o][1]};

            TimingResult t = time_fn(bench_keltnerchannel, &ctx, number, repeat, warmup);
            log_and_print("keltnerchannel", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 2, t, (int) stocks[s].len);
        }
    }
}
