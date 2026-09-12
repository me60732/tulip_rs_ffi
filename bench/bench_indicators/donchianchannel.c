// donchianchannel (Donchian Channel) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a multi-input indicator with options and
// multiple outputs (lower, middle, upper). The middle is computed as medprice(max, min).

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    double period;
} DonchianChannelCtx;

static void bench_donchianchannel(void *ctx_) {
    DonchianChannelCtx *ctx = ctx_;
    const double *inputs[DONCHIANCHANNEL_INPUTS] = {ctx->stock->high, ctx->stock->low};
    double opts[DONCHIANCHANNEL_OPTIONS] = {ctx->period};
    struct CIndicatorResult r = donchianchannel_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] donchianchannel_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    donchianchannel_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_donchianchannel.rs's bench_c_donchianchannel exactly.
//
// Note: DonchianChannel uses ti_max and ti_min separately since the middle is
// computed as medprice(max, min) in the Rust implementation. The C benchmark
// calls these separately to match the Rust implementation pattern.
// ---------------------------------------------------------------------------

static void bench_tulipc_donchianchannel(void *ctx_) {
    DonchianChannelCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    double options[1] = {ctx->period};

    // Get lookback from ti_max (same as ti_min for same period)
    int start_index = ti_max_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_max_start returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;

    double *output_lower = malloc(sizeof(double) * (size_t) output_len);
    double *output_upper = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[2] = {ctx->stock->high, ctx->stock->low};
    double *outputs_max[1] = {output_upper};
    double *outputs_min[1] = {output_lower};

    // Call ti_max for upper band
    int ret_max = ti_max((int) len, inputs, options, outputs_max);
    if (ret_max != 0) { fprintf(stderr, "[error] ti_max returned %d\n", ret_max); exit(1); }

    // Call ti_min for lower band
    int ret_min = ti_min((int) len, inputs, options, outputs_min);
    if (ret_min != 0) { fprintf(stderr, "[error] ti_min returned %d\n", ret_min); exit(1); }

    free(output_lower);
    free(output_upper);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib".
//
// Note: TA-Lib does not have a DONCHIANCHANNEL function. The Donchian Channel
// is typically implemented as max(high, period) and min(low, period).
// We benchmark using the equivalent TA_MAX and TA_MIN calls.
// ---------------------------------------------------------------------------

static void bench_talib_donchianchannel(void *ctx_) {
    DonchianChannelCtx *ctx = ctx_;
    size_t len = ctx->stock->len;
    int start_index = TA_MAX_Lookback((int) ctx->period);
    if (start_index < 0) { fprintf(stderr, "[error] TA_MAX_Lookback returned negative index\n"); exit(1); }
    int output_len = (int) len - start_index;

    double *output_upper = malloc(sizeof(double) * (size_t) output_len);
    double *output_lower = malloc(sizeof(double) * (size_t) output_len);
    int out_begin = 0, out_nb_element = 0;

    // TA_MAX for upper band
    TA_RetCode ret_max = TA_MAX(0, (int) len - 1,
                                ctx->stock->high,
                                (int) ctx->period,
                                &out_begin, &out_nb_element, output_upper);
    if (ret_max != TA_SUCCESS) { fprintf(stderr, "[error] TA_MAX returned %d\n", (int) ret_max); exit(1); }

    // TA_MIN for lower band
    out_begin = 0; out_nb_element = 0;
    TA_RetCode ret_min = TA_MIN(0, (int) len - 1,
                                ctx->stock->low,
                                (int) ctx->period,
                                &out_begin, &out_nb_element, output_lower);
    if (ret_min != TA_SUCCESS) { fprintf(stderr, "[error] TA_MIN returned %d\n", (int) ret_min); exit(1); }

    free(output_upper);
    free(output_lower);
}

static void run_donchianchannel(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][DONCHIANCHANNEL_OPTIONS] = {{5.0}, {14.0}, {20.0}, {50.0}};
    printf("\n--- DONCHIANCHANNEL ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            DonchianChannelCtx ctx = {
                .stock = &stocks[s],
                .period = option_sets[o][0],
            };

            TimingResult t = time_fn(bench_donchianchannel, &ctx, number, repeat, warmup);
            log_and_print("donchianchannel", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], DONCHIANCHANNEL_OPTIONS, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_donchianchannel, &ctx, number, repeat, warmup);
            log_and_print("donchianchannel", "C_tulip", stocks[s].symbol, option_sets[o], DONCHIANCHANNEL_OPTIONS, t_c, (int) stocks[s].len);

            // TA-Lib doesn't have DONCHIANCHANNEL - use MAX/MIN equivalent
            TimingResult t_talib = time_fn(bench_talib_donchianchannel, &ctx, number, repeat, warmup);
            log_and_print("donchianchannel", "talib_max_min", stocks[s].symbol, option_sets[o], DONCHIANCHANNEL_OPTIONS, t_talib, (int) stocks[s].len);
        }
    }
}
