// vhf (Vertical Horizontal Filter) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a single-input indicator with one option
// and no optional outputs.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    double period;
} VhfCtx;

static void bench_vhf(void *ctx_) {
    VhfCtx *ctx = ctx_;
    const double *inputs[VHF_INPUTS] = {ctx->stock->close};
    double opts[VHF_OPTIONS] = {ctx->period};
    struct CIndicatorResult r = vhf_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] vhf_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    vhf_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_vhf.rs's bench_c_vhf exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_vhf(void *ctx_) {
    VhfCtx *ctx = ctx_;
    double options[VHF_OPTIONS] = {ctx->period};
    int start_index = ti_vhf_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_vhf_start returned negative index\n"); exit(1); }
    int output_len = (int) ctx->stock->len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[VHF_INPUTS] = {ctx->stock->close};
    double *outputs[1] = {output};
    int ret = ti_vhf((int) ctx->stock->len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_vhf returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- not available. TA-Lib has no VHF (Vertical Horizontal
// Filter) function.
// ---------------------------------------------------------------------------

static void run_vhf(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][VHF_OPTIONS] = {{14.0}, {20.0}, {28.0}, {50.0}};
    printf("\n--- VHF ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            VhfCtx ctx = {.stock = &stocks[s], .period = option_sets[o][0]};

            TimingResult t = time_fn(bench_vhf, &ctx, number, repeat, warmup);
            log_and_print("vhf", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 1, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_vhf, &ctx, number, repeat, warmup);
            log_and_print("vhf", "C_tulip", stocks[s].symbol, option_sets[o], 1, t_c, (int) stocks[s].len);

            // No TA-Lib comparison available
        }
    }
}
