// ppo (Percentage Price Oscillator) -- tulip_rs_ffi extern "C" API.
//
// Template indicator demonstrating a single-input indicator with options and
// optional_outputs requested. The flattened inputs buffer is built once per
// stock and reused across all option sets for that stock.

#include "tulip_rs_ffi.h"

typedef struct {
    const Stock *stock;
    double short_period, long_period;
} PpoCtx;

static void bench_ppo(void *ctx_) {
    PpoCtx *ctx = ctx_;
    const double *inputs[PPO_INPUTS] = {ctx->stock->close};
    double opts[PPO_OPTIONS] = {ctx->short_period, ctx->long_period};
    struct CIndicatorResult r = ppo_indicator(inputs, ctx->stock->len, opts, NULL, 0);
    if (r.error != C_INDICATOR_ERROR_OK) {
        fprintf(stderr, "[error] ppo_indicator failed: %d\n", (int) r.error);
        exit(1);
    }
    tulip_ffi_result_free(r);
    ppo_state_free(r.state);
}

// ---------------------------------------------------------------------------
// Tulip Indicators (C) comparison -- implementation_type = "C_tulip". Mirrors
// tulip_test/benches/benchmark_ppo.rs's bench_c_ppo exactly.
// ---------------------------------------------------------------------------

static void bench_tulipc_ppo(void *ctx_) {
    PpoCtx *ctx = ctx_;
    double options[PPO_OPTIONS] = {ctx->short_period, ctx->long_period};
    int start_index = ti_ppo_start(options);
    if (start_index < 0) { fprintf(stderr, "[error] ti_ppo_start returned negative index\n"); exit(1); }
    int output_len = (int) ctx->stock->len - start_index;
    double *ppo = malloc(sizeof(double) * (size_t) output_len);
    const double *inputs[PPO_INPUTS] = {ctx->stock->close};
    double *outputs[1] = {ppo};
    int ret = ti_ppo((int) ctx->stock->len, inputs, options, outputs);
    if (ret != 0) { fprintf(stderr, "[error] ti_ppo returned %d\n", ret); exit(1); }
    free(ppo);
}

// ---------------------------------------------------------------------------
// TA-Lib comparison -- implementation_type = "talib". Mirrors
// tulip_test/benches/benchmark_ppo.rs's bench_talib_ppo exactly.
// Note: TA_PPO requires a MAType parameter; using TA_MAType_SMA (0).
// ---------------------------------------------------------------------------

static void bench_talib_ppo(void *ctx_) {
    PpoCtx *ctx = ctx_;
    int size = (int) ctx->stock->len;
    int start_index = TA_PPO_Lookback((int) ctx->short_period, (int) ctx->long_period, TA_MAType_SMA);
    if (start_index < 0) { fprintf(stderr, "[error] TA_PPO_Lookback returned negative index\n"); exit(1); }
    int output_len = size - start_index;
    double *ppo = malloc(sizeof(double) * (size_t) output_len);
    int out_begin = 0, out_nb_element = 0;
    TA_RetCode ret =
        TA_PPO(0, size - 1, ctx->stock->close,
               (int) ctx->short_period, (int) ctx->long_period, TA_MAType_SMA,
               &out_begin, &out_nb_element, ppo);
    if (ret != TA_SUCCESS) { fprintf(stderr, "[error] TA_PPO returned %d\n", (int) ret); exit(1); }
    free(ppo);
}

static void run_ppo(const Stock *stocks, int num_stocks, int number, int repeat, int warmup) {
    static const double option_sets[][PPO_OPTIONS] = {{12.0, 26.0}, {8.0, 18.0}, {5.0, 13.0}, {3.0, 9.0}};
    printf("\n--- PPO ---\n");
    for (int s = 0; s < num_stocks; s++) {
        for (int o = 0; o < 4; o++) {
            PpoCtx ctx = {
                .stock = &stocks[s],
                .short_period = option_sets[o][0],
                .long_period = option_sets[o][1],
            };

            TimingResult t = time_fn(bench_ppo, &ctx, number, repeat, warmup);
            log_and_print("ppo", "tulip_rs_ffi_c", stocks[s].symbol, option_sets[o], PPO_OPTIONS, t, (int) stocks[s].len);

            TimingResult t_c = time_fn(bench_tulipc_ppo, &ctx, number, repeat, warmup);
            log_and_print("ppo", "C_tulip", stocks[s].symbol, option_sets[o], PPO_OPTIONS, t_c, (int) stocks[s].len);

            TimingResult t_talib = time_fn(bench_talib_ppo, &ctx, number, repeat, warmup);
            log_and_print("ppo", "talib", stocks[s].symbol, option_sets[o], PPO_OPTIONS, t_talib, (int) stocks[s].len);
        }
    }
}
