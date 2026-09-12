// mass (Mass Index) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating an indicator with multiple input series and
// options. The inputs array is built inside the timed region since constructing
// it is what a real C caller's hot loop looks like.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    double period;
} MassCtx;

static void bench_mass(void *ctx_) {
    MassCtx *ctx = ctx_;
    const double *inputs[MASS_INPUTS] = {ctx->stock->high, ctx->stock->low};
    double opts[MASS_OPTIONS] = {ctx->period};
    // mass has no optional outputs
    struct CIndicatorResult r = mass_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] mass_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    mass_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_mass.rs's bench_c_mass exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_mass(void *ctx_) {
    MassCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    double options[MASS_OPTIONS] = {ctx->period};
    int start_index = ti_mass_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_mass_start returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    double *output = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[MASS_INPUTS] = {ctx->stock->high, ctx->stock->low};
    double *outputs[1] = {output};
    int ret = ti_mass((int) len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_mass returned %d\n", ret); exit(1); }
    free(output);
}

// ---------------------------------------------------------------------------
// No TA-Lib comparison: TA_MASS does not exist in the official TA-Lib library.
// ---------------------------------------------------------------------------

static void run_mass(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][MASS_OPTIONS] = {{14.0}, {20.0}, {25.0}, {30.0}};
    printf("\n--- MASS ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            MassCtx ctx = {
                .stock = &stocks[s],
                .period = option_sets[o][0],
            };

            TimingResult t = time_fn(bench_mass, &ctx, number, repeat, warmup);
            log_and_print("mass", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], MASS_OPTIONS, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_mass, &ctx, number, repeat, warmup);
            log_and_print("mass", "C_tulip", stocks[s].symbol, option_sets[o], MASS_OPTIONS, t_c, (int) stocks[s].len);
        }
    }
}
