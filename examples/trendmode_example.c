// TRENDMODE (Trend Mode) example for tulip_rs_ffi.
// Mirrors tulip_rs_python/examples/ti_trendmode_example.py: same sample data,
// same options, same "compute partial, continue via batch, verify against a
// full recompute" flow, plus SIMD by-assets/by-options demonstrations.
//
// Build (from the tulip_rs_ffi directory):
//   cc -O2 -o trendmode_example examples/trendmode_example.c \
//       -L target/release -ltulip_rs_ffi -Wl,-rpath,target/release
// Run:
//   ./trendmode_example

#include <stdio.h>
#include <stdlib.h>

#include "../include/tulip_rs_ffi.h"

static const double close[] = {
    81.59, 81.06, 82.87, 83.00, 83.61, 83.15, 82.84, 83.99, 84.55, 84.36,
    85.53, 86.54, 86.89, 87.77, 87.29, 87.50, 88.10, 88.50, 87.90, 88.20,
    88.80, 89.10, 88.70, 89.30, 89.70, 90.10, 89.50, 90.20, 90.80, 91.10,
    90.50, 91.20, 91.80, 92.10, 91.50, 92.20, 92.80, 93.10, 92.50, 93.20,
    93.80, 94.10, 93.50, 94.20, 94.80, 95.10, 94.50, 95.20, 95.80, 96.10,
    95.50, 96.20, 96.80, 97.10, 96.50, 97.20, 97.80, 98.10, 97.50, 98.20,
    98.80, 99.10, 98.50, 99.20, 99.80, 100.10, 99.50, 100.20, 100.80,
    101.10, 100.50, 101.20, 101.80, 102.10, 101.50, 102.20, 102.80, 103.10,
    102.50, 103.20
};

#define TOTAL 70
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
    const double options[TRENDMODE_OPTIONS] = {0.0}; // alpha=0.0 for adaptive mode
    double full_trendmode[TOTAL];
    size_t full_trendmode_len = 0;

    CIndicatorInfo info = trendmode_info();
    printf("=== %s (%s) ===\n", info.name, info.full_name);
    printf("Inputs: %u (real), Options: %u (alpha), Optional outputs: %u\n",
           (unsigned)TRENDMODE_INPUTS, (unsigned)TRENDMODE_OPTIONS,
           (unsigned)info.optional_outputs.len);
    printf("Minimum data required: %zu\n", trendmode_min_data(options));

    printf("\n=== TRENDMODE: full calculation (all optional outputs) ===\n");
    {
        const double *inputs[TRENDMODE_INPUTS] = {close};
        bool optional_outputs[3] = {true, true, true}; // trend, cycle, peak

        CIndicatorResult r = trendmode_indicator(inputs, TOTAL, options, optional_outputs, 3);
        if (r.error != C_INDICATOR_ERROR_OK) {
            fprintf(stderr, "trendmode_indicator failed: error=%d\n", r.error);
            return 1;
        }
        printf("num_outputs=%zu\n", r.num_outputs);
        print_row("trendmode", r.outputs[0], r.output_lens[0]);
        print_row("cycle", r.outputs[1], r.output_lens[1]);
        print_row("peak", r.outputs[2], r.output_lens[2]);

        // Keep a copy of the mandatory "trendmode" row for later verification.
        full_trendmode_len = r.output_lens[0];
        for (size_t i = 0; i < full_trendmode_len; i++) full_trendmode[i] = r.outputs[0][i];

        void *unused_state = r.state;
        tulip_ffi_result_free(r);
        trendmode_state_free(unused_state);
    }

    printf("\n=== TRENDMODE: partial calculation + batch continuation ===\n");
    {
        const double *partial_inputs[TRENDMODE_INPUTS] = {close};
        CIndicatorResult pr = trendmode_indicator(partial_inputs, PARTIAL, options, NULL, 0);
        if (pr.error != C_INDICATOR_ERROR_OK) {
            fprintf(stderr, "trendmode_indicator (partial) failed: error=%d\n", pr.error);
            return 1;
        }
        print_row("trendmode (partial)", pr.outputs[0], pr.output_lens[0]);
        void *state = pr.state;
        tulip_ffi_result_free(pr); // outputs freed; state kept alive

        const double *rest_inputs[TRENDMODE_INPUTS] = {close + PARTIAL};
        CBatchResult br = trendmode_batch(state, rest_inputs, REST, NULL, 0);
        if (br.error != C_INDICATOR_ERROR_OK) {
            fprintf(stderr, "trendmode_batch failed: error=%d\n", br.error);
            return 1;
        }
        print_row("trendmode (continued)", br.outputs[0], br.output_lens[0]);

        printf("\n=== Verification: partial+continued vs. full recompute ===\n");
        int ok = 1;
        size_t continued_len = br.output_lens[0];
        size_t tail_start = full_trendmode_len - continued_len;
        for (size_t i = 0; i < continued_len; i++) {
            double a = full_trendmode[tail_start + i];
            double b = br.outputs[0][i];
            double diff = a > b ? a - b : b - a;
            if (diff > 1e-9) {
                ok = 0;
                printf("  mismatch at %zu: full=%.6f continued=%.6f\n", i, a, b);
            }
        }
        printf(ok ? "  MATCH: partial+continued trendmode equals full recompute\n"
                  : "  MISMATCH detected!\n");

        tulip_ffi_batch_result_free(br);
        trendmode_state_free(state);
    }

    printf("\n=== TRENDMODE: SIMD by assets (N=4) ===\n");
    {
        // Asset 1: original data.
        const double *const asset1[TRENDMODE_INPUTS] = {close};

        // Asset 2: scaled up (+20%).
        double close_2[TOTAL];
        for (size_t i = 0; i < TOTAL; i++) close_2[i] = close[i] * 1.2;
        const double *const asset2[TRENDMODE_INPUTS] = {close_2};

        // Asset 3: different upward trend.
        double close_3[TOTAL];
        for (size_t i = 0; i < TOTAL; i++)
            close_3[i] = 90.0 + (double)i * 0.5 + close[i] * 0.1;
        const double *const asset3[TRENDMODE_INPUTS] = {close_3};

        // Asset 4: downward trend.
        double close_4[TOTAL];
        for (size_t i = 0; i < TOTAL; i++)
            close_4[i] = 100.0 - (double)i * 0.3 + close[i] * 0.05;
        const double *const asset4[TRENDMODE_INPUTS] = {close_4};

        // simd_inputs is indexed by asset (the N=4 SIMD lanes), NOT by input
        // series -- hence [4] here stays a lane count.
        const double *const *const simd_inputs[4] = {asset1, asset2, asset3, asset4};
        bool optional_outputs[3] = {true, true, true};

        CSimdResult r = trendmode_simd_by_assets(simd_inputs, 4, TOTAL, options, optional_outputs, 3);
        if (r.error != C_INDICATOR_ERROR_OK) {
            fprintf(stderr, "trendmode_simd_by_assets failed: error=%d\n", r.error);
            return 1;
        }
        printf("num_results=%zu, num_outputs=%zu\n", r.num_results, r.num_outputs);
        for (size_t i = 0; i < r.num_results; i++) {
            printf("Asset %zu ", i + 1);
            print_row("trendmode", r.outputs[i][0], r.output_lens[i][0]);
        }

        printf("\nVerification - calculating each asset individually:\n");
        int simd_ok = 1;
        for (size_t i = 0; i < r.num_results; i++) {
            CIndicatorResult ind = trendmode_indicator(
                simd_inputs[i], TOTAL, options, optional_outputs, 3);
            if (ind.error != C_INDICATOR_ERROR_OK) {
                fprintf(stderr, "trendmode_indicator (asset %zu) failed: error=%d\n", i + 1, ind.error);
                simd_ok = 0;
                continue;
            }
            int match = allclose(r.outputs[i][0], ind.outputs[0], r.output_lens[i][0]);
            printf("  Asset %zu: %s\n", i + 1, match ? "MATCH" : "MISMATCH");
            if (!match) simd_ok = 0;

            void *ind_state = ind.state;
            tulip_ffi_result_free(ind);
            trendmode_state_free(ind_state);
        }

        for (size_t i = 0; i < r.num_results; i++) trendmode_state_free(r.states[i]);
        tulip_ffi_simd_result_free(r);
        printf(simd_ok ? "  ALL MATCH: SIMD by-assets equals individual calculation\n"
                       : "  MISMATCH detected!\n");
    }

    printf("\n=== TRENDMODE: SIMD by options (N=4) ===\n");
    {
        const double *inputs[TRENDMODE_INPUTS] = {close};

        // simd_options is indexed by option set (the N=4 lanes), each lane
        // pointing at TRENDMODE_OPTIONS values.
        static const double options_1[TRENDMODE_OPTIONS] = {0.0};
        static const double options_2[TRENDMODE_OPTIONS] = {0.05};
        static const double options_3[TRENDMODE_OPTIONS] = {0.07};
        static const double options_4[TRENDMODE_OPTIONS] = {0.10};
        const double *const simd_options[4] = {options_1, options_2, options_3, options_4};

        bool optional_outputs[3] = {true, true, true};

        CSimdResult r = trendmode_simd_by_options(
            inputs, TOTAL, simd_options, 4, optional_outputs, 3);
        if (r.error != C_INDICATOR_ERROR_OK) {
            fprintf(stderr, "trendmode_simd_by_options failed: error=%d\n", r.error);
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
            CIndicatorResult ind = trendmode_indicator(
                inputs, TOTAL, simd_options[i], optional_outputs, 3);
            if (ind.error != C_INDICATOR_ERROR_OK) {
                fprintf(stderr, "trendmode_indicator (option set %zu) failed: error=%d\n", i + 1, ind.error);
                simd_ok = 0;
                continue;
            }
            int match = allclose(r.outputs[i][0], ind.outputs[0], r.output_lens[i][0]);
            printf("  Option set %zu: %s\n", i + 1, match ? "MATCH" : "MISMATCH");
            if (!match) simd_ok = 0;

            void *ind_state = ind.state;
            tulip_ffi_result_free(ind);
            trendmode_state_free(ind_state);
        }

        for (size_t i = 0; i < r.num_results; i++) trendmode_state_free(r.states[i]);
        tulip_ffi_simd_result_free(r);
        printf(simd_ok ? "  ALL MATCH: SIMD by-options equals individual calculation\n"
                       : "  MISMATCH detected!\n");
    }

    printf("\n=== TRENDMODE: state persistence (serialize / deserialize) ===\n");
    {
        const double *pinputs[TRENDMODE_INPUTS] = {close};
        CIndicatorResult pr = trendmode_indicator(pinputs, PARTIAL, options, NULL, 0);
        if (pr.error != C_INDICATOR_ERROR_OK) { fprintf(stderr, "trendmode_indicator failed\n"); return 1; }
        void *st = pr.state;
        tulip_ffi_result_free(pr);

        CBytes blob = tulip_state_serialize(C_INDICATOR_ID_TRENDMODE, C_STATE_FORMAT_BINCODE, st);
        if (blob.ptr == NULL) { fprintf(stderr, "serialize failed\n"); return 1; }
        printf("  blob: %zu bytes, magic=%.4s, name=%.32s\n",
               blob.len, (const char *)blob.ptr, (const char *)blob.ptr + 6);

        void *rs = tulip_state_deserialize(blob.ptr, blob.len);
        tulip_ffi_bytes_free(blob);
        if (rs == NULL) { fprintf(stderr, "deserialize failed\n"); return 1; }

        const double *rinputs[TRENDMODE_INPUTS] = {close + PARTIAL};
        CBatchResult a = trendmode_batch(st, rinputs, REST, NULL, 0);
        CBatchResult b = trendmode_batch(rs, rinputs, REST, NULL, 0);
        int persist_ok = a.error == C_INDICATOR_ERROR_OK && b.error == C_INDICATOR_ERROR_OK &&
                         a.output_lens[0] == b.output_lens[0] &&
                         allclose(a.outputs[0], b.outputs[0], a.output_lens[0]);
        printf(persist_ok ? "  MATCH: deserialized state continues identically\n"
                          : "  MISMATCH detected!\n");
        tulip_ffi_batch_result_free(a);
        tulip_ffi_batch_result_free(b);
        trendmode_state_free(st);
        trendmode_state_free(rs);
    }

    return 0;
}
