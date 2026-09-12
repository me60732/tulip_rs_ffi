// cvi (Chande Volatility Index) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a multi-input indicator with one option.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    double period;
} CviCtx;

static void bench_cvi(void *ctx_) {
    CviCtx *ctx = ctx_;
    const double *inputs[CVI_INPUTS] = {ctx->stock->high, ctx->stock->low};
    double opts[CVI_OPTIONS] = {ctx->period};
    struct CIndicatorResult r = cvi_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] cvi_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    cvi_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_cvi.rs's bench_c_cvi exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_cvi(void *ctx_) {
    CviCtx *ctx = ctx_;
    double options[1] = {ctx->period};
    int start_index = ti_cvi_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_cvi_start returned negative index\n"); exit(1); }
    int output_len = (int) ctx->stock->len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[2] = {ctx->stock->high, ctx->stock->low};
    double *outputs[1] = {output};
    int ret = ti_cvi((int) ctx->stock->len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_cvi returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib".
// NOTE: TA-Lib does not provide an equivalent for cvi.
// ---------------------------------------------------------------------------

static void run_cvi(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][CVI_OPTIONS] = {{5.0}, {14.0}, {20.0}, {30.0}};
    printf("\n--- CVI ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            CviCtx ctx = {
                .stock = &stocks[s],
                .period = option_sets[o][0],
            };

            TimingResult t = time_fn(bench_cvi, &ctx, number, repeat, warmup);
            log_and_print("cvi", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], CVI_OPTIONS, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_cvi, &ctx, number, repeat, warmup);
            log_and_print("cvi", "C_tulip", stocks[s].symbol, option_sets[o], CVI_OPTIONS, t_c, (int) stocks[s].len);

            // No talib comparison available
        }
    }
}
