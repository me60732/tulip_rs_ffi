// pivotpoint (Pivot Point) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a multi-input indicator with options and
// optional outputs requested. The inputs array is built inside the timed region
// since constructing it is what a real C caller's hot loop looks like.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    double period;
} PivotPointCtx;

static void bench_pivotpoint(void *ctx_) {
    PivotPointCtx *ctx = ctx_;
    const double *inputs[PIVOTPOINT_INPUTS] = {ctx->stock->high, ctx->stock->low, ctx->stock->close};
    double opts[PIVOTPOINT_OPTIONS] = {ctx->period};
    struct CIndicatorResult r = pivotpoint_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] pivotpoint_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    pivotpoint_state_free(r.state);
}

// Note: PivotPoint has no C_tulip (ti_*) bindings; only tulip_rs_ffi_c comparison is available.

static void run_pivotpoint(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][PIVOTPOINT_OPTIONS] = {{5.0}, {14.0}, {20.0}, {30.0}};
    printf("\n--- PIVOTPOINT ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            PivotPointCtx ctx = {
                .stock = &stocks[s],
                .period = option_sets[o][0],
            };

            TimingResult t = time_fn(bench_pivotpoint, &ctx, number, repeat, warmup);
            log_and_print("pivotpoint", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], PIVOTPOINT_OPTIONS, t, (int) stocks[s].len);
        }
    }
}
