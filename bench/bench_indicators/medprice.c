// medprice (Median Price) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a multi-input indicator with no options.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    const double *inputs_buf; // high ++ low, each stock->len long
} MedpriceCtx;

static void bench_medprice(void *ctx_) {
    MedpriceCtx *ctx = ctx_;
    const double *inputs[MEDPRICE_INPUTS] = {ctx->stock->high, ctx->stock->low};
    struct CIndicatorResult r = medprice_indicator(inputs, ctx->stock->len, NULL, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] medprice_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    medprice_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_medprice.rs's bench_c_medprice exactly.
// Note: medprice has no options, so ti_medprice_start is called with null pointer.
// ---------------------------------------------------------------------------

static void bench_tulipc_medprice(void *ctx_) {
    MedpriceCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    double options[0] = {};
    int start_index = ti_medprice_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_medprice_start returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[2] = {ctx->stock->high, ctx->stock->low};
    double *outputs[1] = {output};
    int ret = ti_medprice((int) len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_medprice returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib". Mirrors C_tulip pattern.
// TA_MEDPRICE_Lookback() returns void (no parameters), uses high/low arrays.
// ---------------------------------------------------------------------------

static void bench_talib_medprice(void *ctx_) {
    MedpriceCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    int start_index = TA_MEDPRICE_Lookback();
    if (start_index < 0) { fprintf(stderr, "[error] TA_MEDPRICE_Lookback returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    int out_begin = 0, out_nb_element = 0;
    TA_RetCode ret = TA_MEDPRICE(0, (int) len - 1,
                                 ctx->stock->high, ctx->stock->low,
                                 &out_begin, &out_nb_element, output);
    if (ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_MEDPRICE returned %d\n", (int) ret); exit(1); }
    free(output);
}

static void run_medprice(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][MEDPRICE_OPTIONS] = {{}};
    printf("\n--- MEDPRICE ---\n");
    for (int s = 0; s < num_stocks; s++) {
        size_t len = stocks[s].len;
        double *inputs_buf = malloc(sizeof(double) * len * 2);
        memcpy(inputs_buf, stocks[s].high, sizeof(double) * len);
        memcpy(inputs_buf + len, stocks[s].low, sizeof(double) * len);

        for (int o = 0; o < 1; o++) {
            MedpriceCtx ctx = {
                .stock = &stocks[s],
                .inputs_buf = inputs_buf,
            };

            TimingResult t = time_fn(bench_medprice, &ctx, number, repeat, warmup);
            log_and_print("medprice", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 0, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_medprice, &ctx, number, repeat, warmup);
            log_and_print("medprice", "C_tulip", stocks[s].symbol, option_sets[o], 0, t_c, (int) stocks[s].len);

            TimingResult t_talib = time_fn(bench_talib_medprice, &ctx, number, repeat, warmup);
            log_and_print("medprice", "talib", stocks[s].symbol, option_sets[o], 0, t_talib, (int) stocks[s].len);
        }

        free(inputs_buf);
    }
}
