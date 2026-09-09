// ADOSC (Accumulation/Distribution Oscillator) example for tulip_rs_ffi.
// Mirrors tulip_rs_python/examples/ti_adosc_example.py: same sample data,
// same options, same "compute partial, continue via batch, verify against a
// full recompute" flow.
//
// Build:
//   cc -O2 -o adosc_example examples/adosc_example.c \
//       -L target/release -ltulip_rs_ffi -Wl,-rpath,target/release
// Run:
//   ./adosc_example

#include <stdio.h>
#include <stdlib.h>

#include "tulip_rs_ffi.h"

static const double high[]   = {82.15, 81.89, 83.03, 83.30, 83.85, 83.90, 83.33, 84.30, 84.84, 85.00, 85.90, 86.58, 87.87, 88.15, 87.60};
static const double low[]    = {81.29, 80.64, 82.65, 82.70, 83.07, 82.65, 82.20, 83.35, 84.15, 84.11, 85.39, 86.04, 86.58, 87.32, 87.00};
static const double close[]  = {81.59, 81.06, 82.87, 83.00, 83.61, 83.15, 82.84, 83.99, 84.55, 84.36, 85.53, 86.54, 86.89, 87.77, 87.29};
static const double volume[] = {1000000, 1100000, 900000, 1200000, 800000, 950000, 1050000, 1150000, 1300000, 1000000, 1100000, 1250000, 980000, 1180000, 1080000};

#define TOTAL 15
#define PARTIAL 10
#define REST (TOTAL - PARTIAL)

static void print_row(const char *label, const double *row, size_t len) {
    printf("  %-12s: [", label);
    for (size_t i = 0; i < len; i++) {
        printf("%.4f", row[i]);
        if (i + 1 < len) printf(", ");
    }
    printf("]\n");
}

int main(void) {
    const double options[2] = {3.0, 10.0}; // short_period, long_period

    printf("=== ADOSC: full calculation (all optional outputs) ===\n");
    {
        const double *inputs[4] = {high, low, close, volume};
        bool optional_outputs[3] = {true, true, true}; // short_ema, long_ema, ad

        CIndicatorResult r = adosc_indicator(TOTAL, inputs, options, optional_outputs, 3);
        if (r.error != C_OK) {
            fprintf(stderr, "adosc_indicator failed: error=%d\n", r.error);
            return 1;
        }
        printf("num_outputs=%zu\n", r.num_outputs);
        print_row("adosc", r.outputs[0], r.output_lens[0]);
        print_row("short_ema", r.outputs[1], r.output_lens[1]);
        print_row("long_ema", r.outputs[2], r.output_lens[2]);
        print_row("ad", r.outputs[3], r.output_lens[3]);

        // Keep a copy of the mandatory "adosc" row for later verification.
        double full_adosc[TOTAL];
        size_t full_adosc_len = r.output_lens[0];
        for (size_t i = 0; i < full_adosc_len; i++) full_adosc[i] = r.outputs[0][i];

        // We don't need this state; drop both the outputs and the state.
        void *unused_state = r.state;
        tulip_ffi_result_free(r);
        adosc_state_free(unused_state);

        printf("\n=== ADOSC: partial calculation + batch continuation ===\n");
        const double *partial_inputs[4] = {high, low, close, volume};
        CIndicatorResult pr = adosc_indicator(PARTIAL, partial_inputs, options, NULL, 0);
        if (pr.error != C_OK) {
            fprintf(stderr, "adosc_indicator (partial) failed: error=%d\n", pr.error);
            return 1;
        }
        print_row("adosc (partial)", pr.outputs[0], pr.output_lens[0]);
        void *state = pr.state;
        tulip_ffi_result_free(pr); // outputs freed; state kept alive

        const double *rest_inputs[4] = {
            high + PARTIAL, low + PARTIAL, close + PARTIAL, volume + PARTIAL};
        CBatchResult br = adosc_batch(state, REST, rest_inputs, NULL, 0);
        if (br.error != C_OK) {
            fprintf(stderr, "adosc_batch failed: error=%d\n", br.error);
            return 1;
        }
        print_row("adosc (continued)", br.outputs[0], br.output_lens[0]);

        printf("\n=== Verification: partial+continued vs. full recompute ===\n");
        int ok = 1;
        size_t continued_len = br.output_lens[0];
        // Compare the tail of the full recompute against the continued output.
        // Both should describe the same final `REST` bars of adosc.
        size_t tail_start = full_adosc_len - continued_len;
        for (size_t i = 0; i < continued_len; i++) {
            double a = full_adosc[tail_start + i];
            double b = br.outputs[0][i];
            double diff = a > b ? a - b : b - a;
            if (diff > 1e-9) {
                ok = 0;
                printf("  mismatch at %zu: full=%.6f continued=%.6f\n", i, a, b);
            }
        }
        printf(ok ? "  MATCH: partial+continued adosc equals full recompute\n"
                  : "  MISMATCH detected!\n");

        tulip_ffi_batch_result_free(br);
        adosc_state_free(state);
    }

    return 0;
}
