// bop (Balance of Power) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating an indicator with multiple input series but
// no options and no requested optional outputs. The pointer array is built
// inside the timed region since constructing it is what a real C caller's
// hot loop looks like.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
} BopCtx;

static void bench_bop(void *ctx_) {
    BopCtx *ctx = ctx_;
    const double *inputs[BOP_INPUTS] = {ctx->stock->open, ctx->stock->high, ctx->stock->low, ctx->stock->close};
    double opts[BOP_OPTIONS] = {};
    struct CIndicatorResult r = bop_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] bop_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    bop_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_bop.rs's bench_c_bop exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_bop(void *ctx_) {
    BopCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    double options[0] = {};
    int start_index = ti_bop_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_bop_start returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[4] = {ctx->stock->open, ctx->stock->high, ctx->stock->low, ctx->stock->close};
    double *outputs[1] = {output};
    int ret = ti_bop((int) len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_bop returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib". Mirrors
// tulip_test/benches/benchmark_bop.rs's bench_talib_bop exactly.
// ---------------------------------------------------------------------------

static void bench_talib_bop(void *ctx_) {
    BopCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    int start_index = TA_BOP_Lookback();
    if (start_index < 0) { fprintf(stderr, "[error] TA_BOP_Lookback returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    int out_begin = 0, out_nb_element = 0;
    TA_RetCode ret = TA_BOP(0, (int) len - 1,
                            ctx->stock->open, ctx->stock->high, ctx->stock->low, ctx->stock->close,
                            &out_begin, &out_nb_element, output);
    if (ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_BOP returned %d\n", (int) ret); exit(1); }
    free(output);
}

static void run_bop(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][BOP_OPTIONS] = {{}};
    printf("\n--- BOP ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            BopCtx ctx = {.stock = &stocks[s]};

            TimingResult t = time_fn(bench_bop, &ctx, number, repeat, warmup);
            log_and_print("bop", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], BOP_OPTIONS, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_bop, &ctx, number, repeat, warmup);
            log_and_print("bop", "C_tulip", stocks[s].symbol, option_sets[o], BOP_OPTIONS, t_c, (int) stocks[s].len);

            TimingResult t_talib = time_fn(bench_talib_bop, &ctx, number, repeat, warmup);
            log_and_print("bop", "talib", stocks[s].symbol, option_sets[o], BOP_OPTIONS, t_talib, (int) stocks[s].len);
        }
    }
}
