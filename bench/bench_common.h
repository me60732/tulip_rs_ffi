#ifndef TULIP_RS_FFI_BENCH_COMMON_H
#define TULIP_RS_FFI_BENCH_COMMON_H

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdbool.h>
#include <time.h>
#include <math.h>

// Project headers, included here (rather than only in bench.c) so each
// bench_indicators/*.c fragment parses standalone in the IDE: the fragments
// are #included midway through bench.c, so without this they'd never see
// the FFI / C_tulip / TA-Lib declarations on their own. Include guards and
// the Makefile's -I flags make the redundant includes harmless at build.
#include "tulip_rs_ffi.h"
#include "indicators.h"
#include "ta_libc.h"

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

#define DATA_LIMIT 6705

// ---------------------------------------------------------------------------
// Stock data
// ---------------------------------------------------------------------------

typedef struct {
    char symbol[32];
    double *open, *high, *low, *close, *volume;
    size_t len;
} Stock;

// ---------------------------------------------------------------------------
// Timing
// ---------------------------------------------------------------------------

typedef struct {
    long long mean_ns;
    long long stddev_ns;
    long long min_ns;
    long long max_ns;
    int sample_count;
} TimingResult;

static double now_ns(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (double) ts.tv_sec * 1e9 + (double) ts.tv_nsec;
}

// fn(ctx) performs one full call-and-destroy cycle of the indicator under test.
typedef void (*BenchFn)(void *ctx);

static TimingResult time_fn(BenchFn fn, void *ctx, int number, int repeat, int warmup) {
    for (int i = 0; i < warmup; i++) fn(ctx);

    double *samples_ns = calloc((size_t) repeat, sizeof(double));
    for (int r = 0; r < repeat; r++) {
        double start = now_ns();
        for (int n = 0; n < number; n++) fn(ctx);
        double end = now_ns();
        samples_ns[r] = (end - start) / number;
    }

    double sum = 0.0, min = samples_ns[0], max = samples_ns[0];
    for (int r = 0; r < repeat; r++) {
        sum += samples_ns[r];
        if (samples_ns[r] < min) min = samples_ns[r];
        if (samples_ns[r] > max) max = samples_ns[r];
    }
    double mean = sum / repeat;

    double var = 0.0;
    if (repeat > 1) {
        for (int r = 0; r < repeat; r++) {
            double d = samples_ns[r] - mean;
            var += d * d;
        }
        var /= (repeat - 1); // sample stddev, matches Python's statistics.stdev
    }
    double stddev = sqrt(var);

    free(samples_ns);

    TimingResult res = {
        .mean_ns = (long long) mean,
        .stddev_ns = (long long) stddev,
        .min_ns = (long long) min,
        .max_ns = (long long) max,
        .sample_count = repeat,
    };
    return res;
}

// ---------------------------------------------------------------------------
// Result collection + reporting
// ---------------------------------------------------------------------------

typedef struct {
    char indicator[16];
    char impl_type[40];
    char stock_symbol[32];
    double options[8];
    int num_options;
    TimingResult timing;
    int input_size;
} BenchRow;

// Dynamically growing array of result rows. With 94 indicators x up to 3
// implementations x 4 stocks x 4 option sets, the row count can exceed 4000,
// so a fixed-size array is not viable -- grow it via realloc as needed.
static BenchRow *g_rows = NULL;
static int g_row_count = 0;
static int g_row_capacity = 0;

static void record_row(const char *indicator, const char *impl_type, const char *symbol, const double *options,
                        int num_options, TimingResult timing, int input_size) {
    if (g_row_count >= g_row_capacity) {
        g_row_capacity = g_row_capacity == 0 ? 1024 : g_row_capacity * 2;
        g_rows = realloc(g_rows, sizeof(BenchRow) * (size_t) g_row_capacity);
        if (!g_rows) { fprintf(stderr, "[error] failed to grow g_rows to %d entries\n", g_row_capacity); exit(1); }
    }
    BenchRow *row = &g_rows[g_row_count++];
    snprintf(row->indicator, sizeof(row->indicator), "%s", indicator);
    snprintf(row->impl_type, sizeof(row->impl_type), "%s", impl_type);
    snprintf(row->stock_symbol, sizeof(row->stock_symbol), "%s", symbol);
    memcpy(row->options, options, sizeof(double) * num_options);
    row->num_options = num_options;
    row->timing = timing;
    row->input_size = input_size;
}

static void print_row(const BenchRow *row) {
    char opts[128] = {0};
    int pos = 0;
    pos += snprintf(opts + pos, sizeof(opts) - pos, "[");
    for (int i = 0; i < row->num_options; i++) {
        pos += snprintf(opts + pos, sizeof(opts) - pos, "%s%.0f", i ? "," : "", row->options[i]);
    }
    snprintf(opts + pos, sizeof(opts) - pos, "]");

    printf("    %-8s %-26s %-10s %-16s %10lld ns +/- %lld\n", row->indicator, row->impl_type, row->stock_symbol,
           opts, row->timing.mean_ns, row->timing.stddev_ns);
}

// Convenience used by every per-indicator driver in bench_indicators/: records a
// row and immediately prints it, so each driver only needs one call site per
// (indicator, implementation, stock, option-set) combination timed.
static void log_and_print(const char *indicator, const char *impl_type, const char *symbol, const double *options,
                           int num_options, TimingResult timing, int input_size) {
    record_row(indicator, impl_type, symbol, options, num_options, timing, input_size);
    print_row(&g_rows[g_row_count - 1]);
}

#endif // TULIP_RS_FFI_BENCH_COMMON_H
