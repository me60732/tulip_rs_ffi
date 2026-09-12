// emv (Ease of Movement) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a multi-input indicator with no options
// and optional_outputs requested.
#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
} EmvCtx;

static void bench_emv(void *ctx_) {
    EmvCtx *ctx = ctx_;
    const double *inputs[EMV_INPUTS] = {ctx->stock->high, ctx->stock->low, ctx->stock->volume};
    struct CIndicatorResult r = emv_indicator(inputs, ctx->stock->len, NULL, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] emv_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    emv_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_emv.rs's bench_c_emv exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_emv(void *ctx_) {
    EmvCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    int start_index = ti_emv_start(NULL);
    if (start_index < 0) { fprintf(stderr, "[error] ti_emv_start returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[EMV_INPUTS] = {ctx->stock->high, ctx->stock->low, ctx->stock->volume};
    double *outputs[1] = {output};
    int ret = ti_emv((int) len, inputs, NULL, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_emv returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- not available. TA-Lib has no EMV (Ease of Movement)
// function in ta_func.h, so this comparison is omitted.
// ---------------------------------------------------------------------------

static void run_emv(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][EMV_OPTIONS] = {{}};
    printf("\n--- EMV ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 1; o++) {
            EmvCtx ctx = {.stock = &stocks[s]};

            TimingResult t = time_fn(bench_emv, &ctx, number, repeat, warmup);
            log_and_print("emv", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 0, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_emv, &ctx, number, repeat, warmup);
            log_and_print("emv", "C_tulip", stocks[s].symbol, option_sets[o], 0, t_c, (int) stocks[s].len);

            // No TA-Lib comparison available
        }
    }
}
