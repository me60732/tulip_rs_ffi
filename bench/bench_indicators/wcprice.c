// wcprice (Weighted Close Price) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a multi-input indicator with no options and
// no optional_outputs. The flattened inputs buffer is built once per stock
// and reused across all option sets for that stock.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    const double *inputs_buf; // high ++ low ++ close, each stock->len long
} WcPriceCtx;

static void bench_wcprice(void *ctx_) {
    WcPriceCtx *ctx = ctx_;
    const double *inputs[WCPRICE_INPUTS] = {ctx->stock->high, ctx->stock->low, ctx->stock->close};
    // WCPRICE has 0 options
    double opts[0] = {};
    struct CIndicatorResult r = wcprice_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] wcprice_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    wcprice_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_wcprice.rs's bench_c_wcprice exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_wcprice(void *ctx_) {
    WcPriceCtx *ctx = ctx_;
    double options[0] = {};
    int start_index = ti_wcprice_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_wcprice_start returned negative index\n"); exit(1); }
    int output_len = (int) ctx->stock->len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[3] = {ctx->stock->high, ctx->stock->low, ctx->stock->close};
    double *outputs[1] = {output};
    int ret = ti_wcprice((int) ctx->stock->len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_wcprice returned %d\n", ret); exit(1); }
    free(output);
}

static void run_wcprice(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    printf("\n--- WCPRICE ---\n");
    for (int s = 0; s < num_stocks; s++) {
        size_t len = stocks[s].len;
        double *inputs_buf = malloc(sizeof(double) * len * 3);
        memcpy(inputs_buf, stocks[s].high, sizeof(double) * len);
        memcpy(inputs_buf + len, stocks[s].low, sizeof(double) * len);
        memcpy(inputs_buf + 2 * len, stocks[s].close, sizeof(double) * len);

        WcPriceCtx ctx = {
            .stock = &stocks[s],
            .inputs_buf = inputs_buf,
        };

        TimingResult t = time_fn(bench_wcprice, &ctx, number, repeat, warmup);
        log_and_print("wcprice", "tulip_rs_ffi_c", stocks[s].symbol, (double[]){}, 0, t, (int) stocks[s].len);

        TimingResult t_c = time_fn(bench_tulipc_wcprice, &ctx, number, repeat, warmup);
        log_and_print("wcprice", "C_tulip", stocks[s].symbol, (double[]){}, 0, t_c, (int) stocks[s].len);

        free(inputs_buf);
    }
}
