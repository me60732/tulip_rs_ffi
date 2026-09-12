// dx (Directional Movement Index) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a multi-input indicator with options and
// optional outputs requested.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    double period;
} DxCtx;

static void bench_dx(void *ctx_) {
    DxCtx *ctx = ctx_;
    const double *inputs[DX_INPUTS] = {ctx->stock->high, ctx->stock->low, ctx->stock->close};
    double opts[DX_OPTIONS] = {ctx->period};
    struct CIndicatorResult r = dx_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] dx_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    dx_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_dx.rs's bench_c_dx exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_dx(void *ctx_) {
    DxCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    double options[1] = {ctx->period};
    int start_index = ti_dx_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_dx_start returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[3] = {ctx->stock->high, ctx->stock->low, ctx->stock->close};
    double *outputs[1] = {output};
    int ret = ti_dx((int) len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_dx returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib". Mirrors
// tulip_test/benches/benchmark_dx.rs's benchmark pattern exactly.
// ---------------------------------------------------------------------------

static void bench_talib_dx(void *ctx_) {
    DxCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    int start_index = TA_DX_Lookback((int) ctx->period);
    if (start_index < 0) { fprintf(stderr, "[error] TA_DX_Lookback returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    int out_begin = 0, out_nb_element = 0;
    TA_RetCode ret = TA_DX(0, (int) len - 1,
                           ctx->stock->high, ctx->stock->low, ctx->stock->close,
                           (int) ctx->period,
                           &out_begin, &out_nb_element, output);
    if (ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_DX returned %d\n", (int) ret); exit(1); }
    free(output);
}

static void run_dx(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][DX_OPTIONS] = {{5.0}, {14.0}, {24.0}, {30.0}};
    printf("\n--- DX ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            DxCtx ctx = {
                .stock = &stocks[s],
                .period = option_sets[o][0],
            };

            TimingResult t = time_fn(bench_dx, &ctx, number, repeat, warmup);
            log_and_print("dx", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], DX_OPTIONS, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_dx, &ctx, number, repeat, warmup);
            log_and_print("dx", "C_tulip", stocks[s].symbol, option_sets[o], DX_OPTIONS, t_c, (int) stocks[s].len);

            TimingResult t_talib = time_fn(bench_talib_dx, &ctx, number, repeat, warmup);
            log_and_print("dx", "talib", stocks[s].symbol, option_sets[o], DX_OPTIONS, t_talib, (int) stocks[s].len);
        }
    }
}
