// StdDev (Standard Deviation) example for tulip_rs_ffi.
// Mirrors tulip_rs_python/examples/ti_stddev_example.py: same sample data,
// same options, same "compute partial, continue via batch, verify against a
// full recompute" flow, plus SIMD by-assets/by-options demonstrations.
//
// Build (from the tulip_rs_ffi directory):
//   cc -O2 -o stddev_example examples/stddev_example.c \
//       -L target/release -ltulip_rs_ffi -Wl,-rpath,target/release
// Run:
//   ./stddev_example

#include <stdio.h>
#include <stdlib.h>

#include "../include/tulip_rs_ffi.h"

static const double close[] = {81.59, 81.06, 82.87, 83.00, 83.61, 83.15, 82.84, 83.99, 84.55, 84.36, 85.53, 86.54, 86.89, 87.77, 87.29};

#define TOTAL 15
#define PARTIAL (TOTAL - 5)
#define REST (TOTAL - PARTIAL)

static void print_row(const char *label, const double *row, size_t len) {
    printf("  %-14s: [", label);
    for (size_t i = 0; i < len; i++) {
        printf("%.4f", row[i]);
        if (i + 1 < len) printf(", ");
    }
    printf("]\n");
}

static void print_row_head(const char *label, const double *row, size_t len, size_t max_print) {
    printf("  %-14s: [", label);
    size_t n = len < max_print ? len : max_print;
    for (size_t i = 0; i < n; i++) {
        printf("%.4f", row[i]);
        if (i + 1 < n) printf(", ");
    }
    printf(len > max_print ? ", ...]\n" : "]\n");
}

static int allclose(const double *a, const double *b, size_t len) {
    for (size_t i = 0; i < len; i++) {
        double diff = a[i] - b[i];
        if (diff < 0) diff = -diff;
        if (diff > 1e-9) return 0;
    }
    return 1;
}

int main(void) {
    const double options[STDDEV_OPTIONS] = {5.0}; // period
    double full_stddev[TOTAL];
    size_t full_stddev_len = 0;

    CIndicatorInfo info = stddev_info();
    printf("=== %s (%s) ===\n", info.name, info.full_name);
    printf("Inputs: %u, Options: %u (period), Optional outputs: %u\n",
           (unsigned)STDDEV_INPUTS, (unsigned)STDDEV_OPTIONS,
           (unsigned)info.optional_outputs.len);
    printf("Minimum data required: %zu\n", stddev_min_data(options));

    printf("\n=== StdDev: full calculation ===\n");
    {
        const double *inputs[STDDEV_INPUTS] = {close};

        CIndicatorResult r = stddev_indicator(inputs, TOTAL, options, NULL, 0);
        if (r.error != C_INDICATOR_ERROR_OK) {
            fprintf(stderr, "stddev_indicator failed: error=%d\n", r.error);
            return 1;
        }
        printf("num_outputs=%zu\n", r.num_outputs);
        print_row("stddev", r.outputs[0], r.output_lens[0]);

        // Keep a copy of the mandatory "stddev" row for later verification.
        full_stddev_len = r.output_lens[0];
        for (size_t i = 0; i < full_stddev_len; i++) full_stddev[i] = r.outputs[0][i];

        void *unused_state = r.state;
        tulip_ffi_result_free(r);
        stddev_state_free(unused_state);
    }

    printf("\n=== StdDev: partial calculation + batch continuation ===\n");
    {
        const double *partial_inputs[STDDEV_INPUTS] = {close};
        CIndicatorResult pr = stddev_indicator(partial_inputs, PARTIAL, options, NULL, 0);
        if (pr.error != C_INDICATOR_ERROR_OK) {
            fprintf(stderr, "stddev_indicator (partial) failed: error=%d\n", pr.error);
            return 1;
        }
        print_row("stddev (partial)", pr.outputs[0], pr.output_lens[0]);
        void *state = pr.state;
        tulip_ffi_result_free(pr); // outputs freed; state kept alive

        const double *rest_inputs[STDDEV_INPUTS] = {close + PARTIAL};
        CBatchResult br = stddev_batch(state, rest_inputs, REST, NULL, 0);
        if (br.error != C_INDICATOR_ERROR_OK) {
            fprintf(stderr, "stddev_batch failed: error=%d\n", br.error);
            return 1;
        }
        print_row("stddev (continued)", br.outputs[0], br.output_lens[0]);

        printf("\n=== Verification: partial+continued vs. full recompute ===\n");
        int ok = 1;
        size_t continued_len = br.output_lens[0];
        size_t tail_start = full_stddev_len - continued_len;
        for (size_t i = 0; i < continued_len; i++) {
            double a = full_stddev[tail_start + i];
            double b = br.outputs[0][i];
            double diff = a > b ? a - b : b - a;
            if (diff > 1e-9) {
                ok = 0;
                printf("  mismatch at %zu: full=%.6f continued=%.6f\n", i, a, b);
            }
        }
        printf(ok ? "  MATCH: partial+continued stddev equals full recompute\n"
                  : "  MISMATCH detected!\n");

        tulip_ffi_batch_result_free(br);
        stddev_state_free(state);
    }

    printf("\n=== StdDev: SIMD by assets (N=4) ===\n");
    {
        // Asset 1: original data.
        const double *const asset1[STDDEV_INPUTS] = {close};

        // Asset 2: scaled up (+20%).
        double close_2[TOTAL];
        for (size_t i = 0; i < TOTAL; i++) close_2[i] = close[i] * 1.2;
        const double *const asset2[STDDEV_INPUTS] = {close_2};

        // Asset 3: different upward trend.
        double close_3[TOTAL];
        for (size_t i = 0; i < TOTAL; i++)
            close_3[i] = 90.0 + (double)i * 0.5 + close[i] * 0.1;
        const double *const asset3[STDDEV_INPUTS] = {close_3};

        // Asset 4: downward trend.
        double close_4[TOTAL];
        for (size_t i = 0; i < TOTAL; i++)
            close_4[i] = 100.0 - (double)i * 0.3 + close[i] * 0.05;
        const double *const asset4[STDDEV_INPUTS] = {close_4};

        // simd_inputs is indexed by asset (the N=4 SIMD lanes), NOT by input
        // series -- hence [4] here stays a lane count.
        const double *const *const simd_inputs[4] = {asset1, asset2, asset3, asset4};

        CSimdResult r = stddev_simd_by_assets(simd_inputs, 4, TOTAL, options, NULL, 0);
        if (r.error != C_INDICATOR_ERROR_OK) {
            fprintf(stderr, "stddev_simd_by_assets failed: error=%d\n", r.error);
            return 1;
        }
        printf("num_results=%zu, num_outputs=%zu\n", r.num_results, r.num_outputs);
        for (size_t i = 0; i < r.num_results; i++) {
            printf("Asset %zu ", i + 1);
            print_row("stddev", r.outputs[i][0], r.output_lens[i][0]);
        }

        printf("\nVerification - calculating each asset individually:\n");
        int simd_ok = 1;
        for (size_t i = 0; i < r.num_results; i++) {
            CIndicatorResult ind = stddev_indicator(
                simd_inputs[i], TOTAL, options, NULL, 0);
            if (ind.error != C_INDICATOR_ERROR_OK) {
                fprintf(stderr, "stddev_indicator (asset %zu) failed: error=%d\n", i + 1, ind.error);
                simd_ok = 0;
                continue;
            }
            int match = allclose(r.outputs[i][0], ind.outputs[0], r.output_lens[i][0]);
            printf("  Asset %zu: %s\n", i + 1, match ? "MATCH" : "MISMATCH");
            if (!match) simd_ok = 0;

            void *ind_state = ind.state;
            tulip_ffi_result_free(ind);
            stddev_state_free(ind_state);
        }

        for (size_t i = 0; i < r.num_results; i++) stddev_state_free(r.states[i]);
        tulip_ffi_simd_result_free(r);
        printf(simd_ok ? "  ALL MATCH: SIMD by-assets equals individual calculation\n"
                       : "  MISMATCH detected!\n");
    }

    printf("\n=== StdDev: SIMD by options (N=4) ===\n");
    {
        // Tile the base series 20x so longer-period option sets have enough data.
        #define EXPANDED_LEN (TOTAL * 20)
        static double close_expanded[EXPANDED_LEN];
        for (size_t i = 0; i < 20; i++) {
            for (size_t j = 0; j < TOTAL; j++) {
                close_expanded[i * TOTAL + j] = close[j];
            }
        }
        const double *expanded_inputs[STDDEV_INPUTS] = {close_expanded};

        static const double options_1[STDDEV_OPTIONS] = {3.0};
        static const double options_2[STDDEV_OPTIONS] = {5.0};
        static const double options_3[STDDEV_OPTIONS] = {7.0};
        static const double options_4[STDDEV_OPTIONS] = {10.0};
        // simd_options is indexed by option set (the N=4 lanes), each lane
        // pointing at STDDEV_OPTIONS values.
        const double *const simd_options[4] = {options_1, options_2, options_3, options_4};

        CSimdResult r = stddev_simd_by_options(
            expanded_inputs, EXPANDED_LEN, simd_options, 4, NULL, 0);
        if (r.error != C_INDICATOR_ERROR_OK) {
            fprintf(stderr, "stddev_simd_by_options failed: error=%d\n", r.error);
            return 1;
        }
        printf("num_results=%zu, num_outputs=%zu\n", r.num_results, r.num_outputs);
        for (size_t i = 0; i < r.num_results; i++) {
            char label[32];
            snprintf(label, sizeof(label), "option set %zu", i + 1);
            print_row_head(label, r.outputs[i][0], r.output_lens[i][0], 5);
        }

        printf("\nVerification - calculating each option set individually:\n");
        int simd_ok = 1;
        for (size_t i = 0; i < r.num_results; i++) {
            CIndicatorResult ind = stddev_indicator(
                expanded_inputs, EXPANDED_LEN, simd_options[i], NULL, 0);
            if (ind.error != C_INDICATOR_ERROR_OK) {
                fprintf(stderr, "stddev_indicator (option set %zu) failed: error=%d\n", i + 1, ind.error);
                simd_ok = 0;
                continue;
            }
            int match = allclose(r.outputs[i][0], ind.outputs[0], r.output_lens[i][0]);
            printf("  Option set %zu: %s\n", i + 1, match ? "MATCH" : "MISMATCH");
            if (!match) simd_ok = 0;

            void *ind_state = ind.state;
            tulip_ffi_result_free(ind);
            stddev_state_free(ind_state);
        }

        for (size_t i = 0; i < r.num_results; i++) stddev_state_free(r.states[i]);
        tulip_ffi_simd_result_free(r);
        printf(simd_ok ? "  ALL MATCH: SIMD by-options equals individual calculation\n"
                       : "  MISMATCH detected!\n");
    }

    return 0;
}
