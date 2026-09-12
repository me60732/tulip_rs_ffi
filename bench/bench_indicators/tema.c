// tema (Triple Exponential Moving Average) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating an indicator with a single input but
// multiple options and optional outputs requested.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    double period;
} TemaCtx;

static void bench_tema(void *ctx_) {
    TemaCtx *ctx = ctx_;
    const double *inputs[TEMA_INPUTS] = {ctx->stock->close};
    double opts[TEMA_OPTIONS] = {ctx->period};
    struct CIndicatorResult r = tema_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] tema_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    tema_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_tema.rs's bench_c_tema exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_tema(void *ctx_) {
    TemaCtx *ctx = ctx_;
    double options[TEMA_OPTIONS] = {ctx->period};
    int start_index = ti_tema_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_tema_start returned negative index\n"); exit(1); }
    int output_len = (int) ctx->stock->len - start_index;
    double *tema = malloc(sizeof(double) * (size_t) output_len);
    double *dema = malloc(sizeof(double) * (size_t) output_len);
    double *ema = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[TEMA_INPUTS] = {ctx->stock->close};
    double *outputs[3] = {tema, dema, ema};
    int ret = ti_tema((int) ctx->stock->len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_tema returned %d\n", ret); exit(1); }
    free(tema);
    free(dema);
    free(ema);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib". Mirrors
// tulip_test/benches/benchmark_tema.rs's bench_talib_tema exactly.
// ---------------------------------------------------------------------------

static void bench_talib_tema(void *ctx_) {
    TemaCtx *ctx = ctx_;
    int size = (int) ctx->stock->len;
    int start_index = TA_TEMA_Lookback((int) ctx->period);
    if (start_index < 0) { fprintf(stderr, "[error] TA_TEMA_Lookback returned negative index\n"); exit(1); }
    int output_len = size - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    int out_begin = 0, out_nb_element = 0;
    TA_RetCode ret =
        TA_TEMA(0, size - 1, ctx->stock->close, (int) ctx->period,
                &out_begin, &out_nb_element, output);
    if (ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_TEMA returned %d\n", (int) ret); exit(1); }
    free(output);
}

static void run_tema(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][TEMA_OPTIONS] = {{5.0}, {14.0}, {20.0}, {50.0}};
    printf("\n--- TEMA ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            TemaCtx ctx = {.stock = &stocks[s], .period = option_sets[o][0]};

            TimingResult t = time_fn(bench_tema, &ctx, number, repeat, warmup);
            log_and_print("tema", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], TEMA_OPTIONS, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_tema, &ctx, number, repeat, warmup);
            log_and_print("tema", "C_tulip", stocks[s].symbol, option_sets[o], TEMA_OPTIONS, t_c, (int) stocks[s].len);

            TimingResult t_talib = time_fn(bench_talib_tema, &ctx, number, repeat, warmup);
            log_and_print("tema", "talib", stocks[s].symbol, option_sets[o], TEMA_OPTIONS, t_talib, (int) stocks[s].len);
        }
    }
}
