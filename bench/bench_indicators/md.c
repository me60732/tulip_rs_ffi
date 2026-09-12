// md (Mean Deviation) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a single-input indicator with one option.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    double period;
} MdCtx;

static void bench_md(void *ctx_) {
    MdCtx *ctx = ctx_;
    const double *inputs[MD_INPUTS] = {ctx->stock->close};
    double opts[MD_OPTIONS] = {ctx->period};
    struct CIndicatorResult r = md_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] md_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    md_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_md.rs's bench_c_md exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_md(void *ctx_) {
    MdCtx *ctx = ctx_;
    double options[1] = {ctx->period};
    int start_index = ti_md_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_md_start returned negative index\n"); exit(1); }
    int output_len = (int) ctx->stock->len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[1] = {ctx->stock->close};
    double *outputs[1] = {output};
    int ret = ti_md((int) ctx->stock->len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_md returned %d\n", ret); exit(1); }
    free(output);
}

static void run_md(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][MD_OPTIONS] = {{5.0}, {14.0}, {20.0}, {30.0}};
    printf("\n--- MD ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            MdCtx ctx = {.stock = &stocks[s], .period = option_sets[o][0]};

            TimingResult t = time_fn(bench_md, &ctx, number, repeat, warmup);
            log_and_print("md", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], 1, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_md, &ctx, number, repeat, warmup);
            log_and_print("md", "C_tulip", stocks[s].symbol, option_sets[o], 1, t_c, (int) stocks[s].len);
        }
    }
}
