// mfi (Money Flow Index) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a multi-input indicator with one option.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    const double *inputs_buf; // high ++ low ++ close ++ volume, each stock->len long
    double period;
} MfiCtx;

static void bench_mfi(void *ctx_) {
    MfiCtx *ctx = ctx_;
    const double *inputs[MFI_INPUTS] = {ctx->stock->high, ctx->stock->low, ctx->stock->close, ctx->stock->volume};
    double opts[MFI_OPTIONS] = {ctx->period};
    struct CIndicatorResult r = mfi_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] mfi_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    mfi_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_mfi.rs's bench_c_mfi exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_mfi(void *ctx_) {
    MfiCtx *ctx = ctx_;
    double options[1] = {ctx->period};
    int start_index = ti_mfi_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_mfi_start returned negative index\n"); exit(1); }
    int output_len = (int) ctx->stock->len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[4] = {ctx->stock->high, ctx->stock->low, ctx->stock->close, ctx->stock->volume};
    double *outputs[1] = {output};
    int ret = ti_mfi((int) ctx->stock->len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_mfi returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib". Mirrors C_tulip pattern.
// TA_MFI_Lookback(optInTimePeriod) returns int, uses high/low/close/volume arrays.
// ---------------------------------------------------------------------------

static void bench_talib_mfi(void *ctx_) {
    MfiCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    int start_index = TA_MFI_Lookback((int) ctx->period);
    if (start_index < 0) { fprintf(stderr, "[error] TA_MFI_Lookback returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    int out_begin = 0, out_nb_element = 0;
    TA_RetCode ret = TA_MFI(0, (int) len - 1,
                            ctx->stock->high, ctx->stock->low, ctx->stock->close, ctx->stock->volume,
                            (int) ctx->period,
                            &out_begin, &out_nb_element, output);
    if (ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_MFI returned %d\n", (int) ret); exit(1); }
    free(output);
}

static void run_mfi(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][MFI_OPTIONS] = {{14.0}, {20.0}, {25.0}, {30.0}};
    printf("\n--- MFI ---\n");
    for (int s = 0; s < num_stocks; s++) {
        size_t len = stocks[s].len;
        double *inputs_buf = malloc(sizeof(double) * len * 4);
        memcpy(inputs_buf, stocks[s].high, sizeof(double) * len);
        memcpy(inputs_buf + len, stocks[s].low, sizeof(double) * len);
        memcpy(inputs_buf + 2 * len, stocks[s].close, sizeof(double) * len);
        memcpy(inputs_buf + 3 * len, stocks[s].volume, sizeof(double) * len);

        for (int o = 0; o < 4; o++) {
            MfiCtx ctx = {
                .stock = &stocks[s],
                .inputs_buf = inputs_buf,
                .period = option_sets[o][0],
            };

            TimingResult t = time_fn(bench_mfi, &ctx, number, repeat, warmup);
            log_and_print("mfi", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 1, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_mfi, &ctx, number, repeat, warmup);
            log_and_print("mfi", "C_tulip", stocks[s].symbol, option_sets[o], 1, t_c, (int) stocks[s].len);

            TimingResult t_talib = time_fn(bench_talib_mfi, &ctx, number, repeat, warmup);
            log_and_print("mfi", "talib", stocks[s].symbol, option_sets[o], 1, t_talib, (int) stocks[s].len);
        }

        free(inputs_buf);
    }
}
