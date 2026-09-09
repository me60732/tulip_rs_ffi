// Quick manual smoke test for tulip_rs_ffi: exercises adosc + macd,
// including batch/streaming continuation, and prints outputs so they can
// be sanity-checked. Build/run with:
//
//   cc -O2 -o verify verify.c -L target/release -ltulip_rs_ffi
//   (add -Wl,-rpath,target/release so it finds the .so at runtime)
//   ./verify

#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

typedef enum {
    C_OK = 0,
    C_INVALID_INPUTS = 1,
    C_NOT_ENOUGH_DATA = 2,
    C_INVALID_OPTIONS = 3,
    C_INVALID_INDICATOR_STATE = 4,
} CIndicatorError;

typedef struct {
    CIndicatorError error;
    double **outputs;
    size_t *output_lens;
    size_t num_outputs;
    void *state;
} CIndicatorResult;

typedef struct {
    CIndicatorError error;
    double ***outputs;
    size_t **output_lens;
    size_t num_outputs;
    void **states;
    size_t num_results;
} CSimdResult;

typedef struct {
    CIndicatorError error;
    double **outputs;
    size_t *output_lens;
    size_t num_outputs;
} CBatchResult;

extern void tulip_ffi_result_free(CIndicatorResult result);
extern void tulip_ffi_batch_result_free(CBatchResult result);
extern void tulip_ffi_simd_result_free(CSimdResult result);

// Tulip-Indicators-style signatures: inputs is an array of pointers (one
// per input series), options is a flat array -- e.g. compare to
// https://tulipindicators.org/adosc's `ti_adosc(data_len, inputs, options, outputs)`.
// Parameter order: every pointer parameter is immediately followed by the
// count(s) that describe it.
extern CIndicatorResult adosc_indicator(
    double const *const *inputs, size_t data_len, double const *options,
    bool const *optional_outputs, size_t num_optional);
extern CBatchResult adosc_batch(
    void *state, double const *const *inputs, size_t data_len,
    bool const *optional_outputs, size_t num_optional);
extern void adosc_state_free(void *state);

extern CIndicatorResult macd_indicator(
    double const *const *inputs, size_t data_len, double const *options,
    bool const *optional_outputs, size_t num_optional);
extern CBatchResult macd_batch(
    void *state, double const *const *inputs, size_t data_len,
    bool const *optional_outputs, size_t num_optional);
extern void macd_state_free(void *state);

extern CSimdResult adosc_simd_by_assets(
    double const *const *const *inputs, size_t num_assets, size_t data_len,
    double const *options, bool const *optional_outputs, size_t num_optional);
extern CSimdResult adosc_simd_by_options(
    double const *const *inputs, size_t data_len, double const *const *options,
    size_t num_option_sets, bool const *optional_outputs, size_t num_optional);

extern CSimdResult macd_simd_by_assets(
    double const *const *const *inputs, size_t num_assets, size_t data_len,
    double const *options, bool const *optional_outputs, size_t num_optional);
extern CSimdResult macd_simd_by_options(
    double const *const *inputs, size_t data_len, double const *const *options,
    size_t num_option_sets, bool const *optional_outputs, size_t num_optional);

static void print_row(const char *label, const double *row, size_t len, size_t max_print) {
    printf("  %s (len=%zu): ", label, len);
    size_t n = len < max_print ? len : max_print;
    for (size_t i = 0; i < n; i++) printf("%.4f ", row[i]);
    if (len > max_print) printf("...");
    printf("\n");
}

static double *make_series(size_t n, double seed) {
    double *v = malloc(sizeof(double) * n);
    double x = seed;
    for (size_t i = 0; i < n; i++) {
        x += (double) ((i * 37) % 7) - 3.0;
        v[i] = 100.0 + x * 0.1;
    }
    return v;
}

int main(void) {
    const size_t total = 200;
    const size_t initial = 150; // first chunk fed to *_indicator()
    const size_t rest = total - initial; // fed to *_batch()

    double *high = make_series(total, 1.0);
    double *low = make_series(total, 0.5);
    double *close = make_series(total, 0.8);
    double *volume = make_series(total, 1000.0);
    for (size_t i = 0; i < total; i++) {
        if (low[i] > high[i]) { double t = low[i]; low[i] = high[i]; high[i] = t; }
    }

    printf("=== adosc ===\n");
    {
        const double *adosc_inputs[4] = {high, low, close, volume};
        const double adosc_options[2] = {5.0, 20.0};

        CIndicatorResult r = adosc_indicator(adosc_inputs, initial, adosc_options, NULL, 0);
        if (r.error != C_OK) { fprintf(stderr, "adosc_indicator failed: %d\n", r.error); return 1; }
        printf("initial call: num_outputs=%zu\n", r.num_outputs);
        print_row("adosc", r.outputs[0], r.output_lens[0], 5);
        void *state = r.state;
        tulip_ffi_result_free(r); // frees outputs only, state untouched

        const double *adosc_inputs_rest[4] = {
            high + initial, low + initial, close + initial, volume + initial};
        CBatchResult b = adosc_batch(state, adosc_inputs_rest, rest, NULL, 0);
        if (b.error != C_OK) { fprintf(stderr, "adosc_batch failed: %d\n", b.error); return 1; }
        printf("batch call: num_outputs=%zu\n", b.num_outputs);
        print_row("adosc (continued)", b.outputs[0], b.output_lens[0], 5);
        tulip_ffi_batch_result_free(b);

        adosc_state_free(state);
    }

    printf("=== macd ===\n");
    {
        const double *macd_inputs[1] = {close};
        const double macd_options[3] = {12.0, 26.0, 9.0};

        CIndicatorResult r = macd_indicator(macd_inputs, initial, macd_options, NULL, 0);
        if (r.error != C_OK) { fprintf(stderr, "macd_indicator failed: %d\n", r.error); return 1; }
        printf("initial call: num_outputs=%zu\n", r.num_outputs);
        print_row("macd_line", r.outputs[0], r.output_lens[0], 5);
        print_row("signal_line", r.outputs[1], r.output_lens[1], 5);
        print_row("histogram", r.outputs[2], r.output_lens[2], 5);
        void *state = r.state;
        tulip_ffi_result_free(r);

        const double *macd_inputs_rest[1] = {close + initial};
        CBatchResult b = macd_batch(state, macd_inputs_rest, rest, NULL, 0);
        if (b.error != C_OK) { fprintf(stderr, "macd_batch failed: %d\n", b.error); return 1; }
        printf("batch call: num_outputs=%zu\n", b.num_outputs);
        print_row("macd_line (continued)", b.outputs[0], b.output_lens[0], 5);
        tulip_ffi_batch_result_free(b);

        macd_state_free(state);
    }

    free(high); free(low); free(close); free(volume);
    printf("OK\n");
    return 0;
}
