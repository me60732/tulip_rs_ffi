// dpo (Detrended Price Oscillator) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a single-input indicator with options and
// optional outputs requested.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    double period;
} DpoCtx;

static void bench_dpo(void *ctx_) {
    DpoCtx *ctx = ctx_;
    const double *inputs[DPO_INPUTS] = {ctx->stock->close};
    double opts[DPO_OPTIONS] = {ctx->period};
    struct CIndicatorResult r = dpo_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] dpo_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    dpo_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_dpo.rs's bench_c_dpo exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_dpo(void *ctx_) {
    DpoCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    double options[1] = {ctx->period};
    int start_index = ti_dpo_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_dpo_start returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[1] = {ctx->stock->close};
    double *outputs[1] = {output};
    int ret = ti_dpo((int) len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_dpo returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- not available. TA-Lib has no DPO (Detrended Price
// Oscillator) function in ta_func.h, so this comparison is omitted.
// ---------------------------------------------------------------------------

static void run_dpo(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][DPO_OPTIONS] = {{5.0}, {14.0}, {20.0}, {30.0}};
    printf("\n--- DPO ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            DpoCtx ctx = {
                .stock = &stocks[s],
                .period = option_sets[o][0],
            };

            TimingResult t = time_fn(bench_dpo, &ctx, number, repeat, warmup);
            log_and_print("dpo", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], DPO_OPTIONS, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_dpo, &ctx, number, repeat, warmup);
            log_and_print("dpo", "C_tulip", stocks[s].symbol, option_sets[o], DPO_OPTIONS, t_c, (int) stocks[s].len);

            // No TA-Lib comparison available
        }
    }
}
