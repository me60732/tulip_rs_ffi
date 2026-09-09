// Shared C declarations for tulip_rs_ffi examples. Mirrors the extern "C"
// surface defined in src/common.rs, src/adosc.rs, src/macd.rs.
//
// This header is hand-written (not generated) since tulip_rs_ffi is a
// hand-rolled FFI wrapper, not a codegen'd binding.

#ifndef TULIP_RS_FFI_H
#define TULIP_RS_FFI_H

#include <stdbool.h>
#include <stddef.h>

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
    double **outputs;
    size_t *output_lens;
    size_t num_outputs;
} CBatchResult;

extern void tulip_ffi_result_free(CIndicatorResult result);
extern void tulip_ffi_batch_result_free(CBatchResult result);

// Tulip-Indicators-style signatures: inputs is an array of pointers (one
// per input series), options is a flat array -- e.g. compare to
// https://tulipindicators.org/adosc's `ti_adosc(size, inputs, options, outputs)`.
extern CIndicatorResult adosc_indicator(
    size_t size, double const *const *inputs, double const *options,
    bool const *optional_outputs, size_t num_optional);
extern CBatchResult adosc_batch(
    void *state, size_t size, double const *const *inputs,
    bool const *optional_outputs, size_t num_optional);
extern void adosc_state_free(void *state);

extern CIndicatorResult macd_indicator(
    size_t size, double const *const *inputs, double const *options,
    bool const *optional_outputs, size_t num_optional);
extern CBatchResult macd_batch(
    void *state, size_t size, double const *const *inputs,
    bool const *optional_outputs, size_t num_optional);
extern void macd_state_free(void *state);

#endif // TULIP_RS_FFI_H
