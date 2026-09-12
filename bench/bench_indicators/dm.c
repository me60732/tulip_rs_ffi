// dm (Directional Movement) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a multi-input indicator with options and
// multiple mandatory outputs (+DM, -DM).

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    double period;
} DmCtx;

static void bench_dm(void *ctx_) {
    DmCtx *ctx = ctx_;
    const double *inputs[DM_INPUTS] = {ctx->stock->high, ctx->stock->low};
    double opts[DM_OPTIONS] = {ctx->period};
    struct CIndicatorResult r = dm_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] dm_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    dm_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_dm.rs's bench_c_dm exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_dm(void *ctx_) {
    DmCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    double options[1] = {ctx->period};
    int start_index = ti_dm_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_dm_start returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;
    double *output_plus_dm = malloc(sizeof(double) * (size_t) output_len);
    double *output_minus_dm = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[2] = {ctx->stock->high, ctx->stock->low};
    double *outputs[2] = {output_plus_dm, output_minus_dm};
    int ret = ti_dm((int) len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_dm returned %d\n", ret); exit(1); }
    free(output_plus_dm);
    free(output_minus_dm);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib".
// NOTE: TA-Lib does not provide an equivalent for dm.
// ---------------------------------------------------------------------------

static void run_dm(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][DM_OPTIONS] = {{24.0}, {14.0}, {5.0}, {30.0}};
    printf("\n--- DM ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            DmCtx ctx = {
                .stock = &stocks[s],
                .period = option_sets[o][0],
            };

            TimingResult t = time_fn(bench_dm, &ctx, number, repeat, warmup);
            log_and_print("dm", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], DM_OPTIONS, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_dm, &ctx, number, repeat, warmup);
            log_and_print("dm", "C_tulip", stocks[s].symbol, option_sets[o], DM_OPTIONS, t_c, (int) stocks[s].len);

            // No talib comparison available
        }
    }
}
