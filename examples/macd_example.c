// MACD (Moving Average Convergence/Divergence) example for tulip_rs_ffi.
// Mirrors tulip_rs_python/examples/ti_macd_example.py: same sample data,
// same options, same "compute partial, continue via batch, verify against a
// full recompute" flow.
//
// Build:
//   cc -O2 -o macd_example examples/macd_example.c \
//       -L target/release -ltulip_rs_ffi -Wl,-rpath,target/release
// Run:
//   ./macd_example

#include <stdio.h>
#include <stdlib.h>

#include "tulip_rs_ffi.h"

static const double close[] = {81.59, 81.06, 82.87, 83.00, 83.61, 83.15, 82.84, 83.99, 84.55, 84.36, 85.53, 86.54, 86.89, 87.77, 87.29};

#define TOTAL 15
#define PARTIAL 14
#define REST (TOTAL - PARTIAL)

static void print_row(const char *label, const double *row, size_t len) {
    printf("  %-14s: [", label);
    for (size_t i = 0; i < len; i++) {
        printf("%.4f", row[i]);
        if (i + 1 < len) printf(", ");
    }
    printf("]\n");
}

int main(void) {
    const double options[3] = {2.0, 5.0, 9.0}; // short_period, long_period, signal_period

    printf("=== MACD: full calculation (all optional outputs) ===\n");
    {
        const double *inputs[1] = {close};
        bool optional_outputs[2] = {true, true}; // short_ema, long_ema

        CIndicatorResult r = macd_indicator(TOTAL, inputs, options, optional_outputs, 2);
        if (r.error != C_OK) {
            fprintf(stderr, "macd_indicator failed: error=%d\n", r.error);
            return 1;
        }
        printf("num_outputs=%zu\n", r.num_outputs);
        print_row("macd_line", r.outputs[0], r.output_lens[0]);
        print_row("signal_line", r.outputs[1], r.output_lens[1]);
        print_row("histogram", r.outputs[2], r.output_lens[2]);
        print_row("short_ema", r.outputs[3], r.output_lens[3]);
        print_row("long_ema", r.outputs[4], r.output_lens[4]);

        // Keep a copy of the mandatory "macd_line" row for later verification.
        double full_macd[TOTAL];
        size_t full_macd_len = r.output_lens[0];
        for (size_t i = 0; i < full_macd_len; i++) full_macd[i] = r.outputs[0][i];

        void *unused_state = r.state;
        tulip_ffi_result_free(r);
        macd_state_free(unused_state);

        printf("\n=== MACD: partial calculation + batch continuation ===\n");
        const double *partial_inputs[1] = {close};
        bool partial_optional_outputs[2] = {true, true};
        CIndicatorResult pr = macd_indicator(
            PARTIAL, partial_inputs, options, partial_optional_outputs, 2);
        if (pr.error != C_OK) {
            fprintf(stderr, "macd_indicator (partial) failed: error=%d\n", pr.error);
            return 1;
        }
        print_row("macd_line (partial)", pr.outputs[0], pr.output_lens[0]);
        void *state = pr.state;
        tulip_ffi_result_free(pr); // outputs freed; state kept alive

        const double *rest_inputs[1] = {close + PARTIAL};
        CBatchResult br = macd_batch(state, REST, rest_inputs, NULL, 0);
        if (br.error != C_OK) {
            fprintf(stderr, "macd_batch failed: error=%d\n", br.error);
            return 1;
        }
        print_row("macd_line (continued)", br.outputs[0], br.output_lens[0]);
        print_row("signal_line (continued)", br.outputs[1], br.output_lens[1]);
        print_row("histogram (continued)", br.outputs[2], br.output_lens[2]);

        printf("\n=== Verification: partial+continued vs. full recompute ===\n");
        int ok = 1;
        size_t continued_len = br.output_lens[0];
        size_t tail_start = full_macd_len - continued_len;
        for (size_t i = 0; i < continued_len; i++) {
            double a = full_macd[tail_start + i];
            double b = br.outputs[0][i];
            double diff = a > b ? a - b : b - a;
            if (diff > 1e-9) {
                ok = 0;
                printf("  mismatch at %zu: full=%.6f continued=%.6f\n", i, a, b);
            }
        }
        printf(ok ? "  MATCH: partial+continued macd_line equals full recompute\n"
                  : "  MISMATCH detected!\n");

        tulip_ffi_batch_result_free(br);
        macd_state_free(state);
    }

    return 0;
}
