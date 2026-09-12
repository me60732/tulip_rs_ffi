// tr (True Range) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a multi-input indicator with no options.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
} TrCtx;

static void bench_tr(void *ctx_) {
    TrCtx *ctx = ctx_;
    const double *inputs[TR_INPUTS] = {ctx->stock->high, ctx->stock->low, ctx->stock->close};
    // TR_OPTIONS == 0, omit opts array
    struct CIndicatorResult r = tr_indicator(inputs, ctx->stock->len, NULL, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] tr_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    tr_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_tr.rs's bench_c_tr exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_tr(void *ctx_) {
    TrCtx *ctx = ctx_;
    double options[0] = {};
    int start_index = ti_tr_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_tr_start returned negative index\n"); exit(1); }
    int output_len = (int) ctx->stock->len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[3] = {ctx->stock->high, ctx->stock->low, ctx->stock->close};
    double *outputs[1] = {output};
    int ret = ti_tr((int) ctx->stock->len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_tr returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib". Mirrors
// tulip_test/benches/benchmark_tr.rs's bench_talib_tr exactly.
// ---------------------------------------------------------------------------

static void bench_talib_tr(void *ctx_) {
    TrCtx *ctx = ctx_;
    int size = (int) ctx->stock->len;
    int start_index = TA_TRANGE_Lookback();
    if (start_index < 0) { fprintf(stderr, "[error] TA_TRANGE_Lookback returned negative index\n"); exit(1); }
    int output_len = size - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    int out_begin = 0, out_nb_element = 0;
    TA_RetCode ret =
        TA_TRANGE(0, size - 1,
                  ctx->stock->high, ctx->stock->low, ctx->stock->close,
                  &out_begin, &out_nb_element, output);
    if (ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_TRANGE returned %d\n", (int) ret); exit(1); }
    free(output);
}

static void run_tr(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][TR_OPTIONS] = {{}};
    printf("\n--- TR ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 1; o++) {
            TrCtx ctx = {.stock = &stocks[s]};

            TimingResult t = time_fn(bench_tr, &ctx, number, repeat, warmup);
            log_and_print("tr", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], TR_OPTIONS, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_tr, &ctx, number, repeat, warmup);
            log_and_print("tr", "C_tulip", stocks[s].symbol, option_sets[o], TR_OPTIONS, t_c, (int) stocks[s].len);

            TimingResult t_talib = time_fn(bench_talib_tr, &ctx, number, repeat, warmup);
            log_and_print("tr", "talib", stocks[s].symbol, option_sets[o], TR_OPTIONS, t_talib, (int) stocks[s].len);
        }
    }
}
