// Shared C declarations for tulip_rs_ffi examples. Mirrors the extern "C"
// surface defined in src/common.rs, src/adosc.rs, src/macd.rs.
//
// This header is hand-written (not generated) since tulip_rs_ffi is a
// hand-rolled FFI wrapper, not a codegen'd binding.
//
// Parameter order convention: every pointer parameter is immediately
// followed by the count(s) that describe it, e.g.
// `inputs, data_len, options, ..., optional_outputs, num_optional`.

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

// Result of a *_simd_by_assets()/*_simd_by_options() call: num_results
// parallel results, each with its own output rows and its own ordinary
// indicator state (reusable with the matching indicator's *_batch()/
// *_state_free() functions -- there's no separate SIMD state type).
typedef struct {
    CIndicatorError error;
    double ***outputs;      // [num_results][num_outputs] -> f64 row
    size_t **output_lens;   // [num_results][num_outputs] -> row length
    size_t num_outputs;     // output rows per result
    void **states;          // [num_results] opaque state handles
    size_t num_results;     // N: number of assets or option sets
} CSimdResult;

extern void tulip_ffi_result_free(CIndicatorResult result);
extern void tulip_ffi_batch_result_free(CBatchResult result);
// Frees outputs/output_lens and the outer `states` array wrapper, but NOT
// the individual state pointers in `states[i]` -- free each of those first
// with the matching indicator's `*_state_free()`.
extern void tulip_ffi_simd_result_free(CSimdResult result);

// Tulip-Indicators-style signatures: inputs is an array of pointers (one
// per input series), options is a flat array -- e.g. compare to
// https://tulipindicators.org/adosc's `ti_adosc(data_len, inputs, options, outputs)`.
extern CIndicatorResult adosc_indicator(
    double const *const *inputs, size_t data_len, double const *options,
    bool const *optional_outputs, size_t num_optional);
extern CBatchResult adosc_batch(
    void *state, double const *const *inputs, size_t data_len,
    bool const *optional_outputs, size_t num_optional);
extern void adosc_state_free(void *state);

// SIMD: N assets, one shared options array. num_assets must be 2, 4, 8, or 16.
extern CSimdResult adosc_simd_by_assets(
    double const *const *const *inputs, size_t num_assets, size_t data_len,
    double const *options, bool const *optional_outputs, size_t num_optional);
// SIMD: one asset, N option sets. num_option_sets must be 2, 4, 8, or 16.
extern CSimdResult adosc_simd_by_options(
    double const *const *inputs, size_t data_len, double const *const *options,
    size_t num_option_sets, bool const *optional_outputs, size_t num_optional);

extern CIndicatorResult macd_indicator(
    double const *const *inputs, size_t data_len, double const *options,
    bool const *optional_outputs, size_t num_optional);
extern CBatchResult macd_batch(
    void *state, double const *const *inputs, size_t data_len,
    bool const *optional_outputs, size_t num_optional);
extern void macd_state_free(void *state);

// SIMD: N assets, one shared options array. num_assets must be 2, 4, 8, or 16.
extern CSimdResult macd_simd_by_assets(
    double const *const *const *inputs, size_t num_assets, size_t data_len,
    double const *options, bool const *optional_outputs, size_t num_optional);
// SIMD: one asset, N option sets. num_option_sets must be 2, 4, 8, or 16.
extern CSimdResult macd_simd_by_options(
    double const *const *inputs, size_t data_len, double const *const *options,
    size_t num_option_sets, bool const *optional_outputs, size_t num_optional);

#endif // TULIP_RS_FFI_H
