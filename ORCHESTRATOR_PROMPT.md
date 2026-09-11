# Orchestrator Prompt: Roll out `tulip_rs_ffi` wrappers for all remaining indicators

You are the **main local orchestrating agent** for this task. Your job is to
drive this work to completion by spawning and managing sub-agents in small
batches, validating each batch's output, and integrating the results. You
should do very little hands-on file editing yourself -- your value is in
correctly scoping each sub-agent's task, catching regressions early, and
keeping the whole crate in a consistently buildable state between batches.

## Context

`tulip_rs_ffi` (this repo) is a hand-rolled `extern "C"` FFI wrapper crate
around the core `tulip_rs` Rust indicator library (a sibling directory,
typically at `../tulip_rs/tulip_rs` relative to this repo -- confirm the
actual relative path from this repo's `Cargo.toml` dependency entry before
starting). It exposes a Tulip-Indicators-style C calling convention: raw
pointers in, output allocation happens inside the wrapper, a `Result`-style
struct out. Two indicators are already fully wrapped and serve as the
canonical reference implementation: `adosc` (`src/adosc.rs`) and `macd`
(`src/macd.rs`), backed by shared plumbing in `src/common.rs`.

**Your task**: implement the same wrapper pattern for every other indicator
in the core `tulip_rs` crate, **except** `candlestick` (skip it -- it has a
fundamentally different shape, driven by a pattern-registry macro, and will
be handled as a separate hand-coded task later, not through this rollout).

## Required reading before spawning any sub-agent

Read these yourself first, in full, so you can write accurate, self-contained
sub-agent prompts (sub-agents do not share your context -- see "Sub-agent
prompt template" below for what to hand them):

- `tulip_rs_ffi/src/common.rs` -- shared C-ABI plumbing: `CIndicatorError`,
  `CIndicatorResult`, `CBatchResult`, `CSimdResult`, `pack_outputs`,
  `pack_simd_outputs`, `pack_states`, `free_outputs`,
  `tulip_ffi_result_free`, `tulip_ffi_batch_result_free`,
  `tulip_ffi_simd_result_free`, `optional_outputs_slice`, and the three
  generic pointer-reconstruction helpers: `read_inputs::<N>`,
  `read_simd_assets_inputs::<N, INPUTS>`, `read_simd_options::<N, OPTIONS>`.
  **These are already correct and generic -- every new indicator module
  should reuse them as-is. Do not duplicate or reimplement them per
  indicator.**
- `tulip_rs_ffi/src/adosc.rs` -- the canonical reference. Every new
  indicator module should follow this exact shape and naming convention.
- `tulip_rs_ffi/src/macd.rs` -- second reference example (shows a
  multi-mandatory-output indicator: `macd_line`/`signal_line`/`histogram`).
- `tulip_rs_ffi/src/lib.rs` -- crate root; you'll add one `pub mod <name>;`
  line per new indicator here.
- `tulip_rs_ffi/README.md` -- update its "Status / next steps" checklist as
  work completes.

## The established pattern (must be followed exactly)

Every indicator module (e.g. `src/rsi.rs` for the `rsi` indicator) must
expose exactly these seven `#[no_mangle] extern "C"` functions, following
`adosc.rs`/`macd.rs` verbatim in style, naming, and parameter order:

```rust
<name>_indicator(inputs, data_len, options, optional_outputs, num_optional) -> CIndicatorResult
<name>_batch(state, inputs, data_len, optional_outputs, num_optional) -> CBatchResult
<name>_state_free(state)
<name>_simd_by_assets(inputs, num_assets, data_len, options, optional_outputs, num_optional) -> CSimdResult
<name>_simd_by_options(inputs, data_len, options, num_option_sets, optional_outputs, num_optional) -> CSimdResult
<name>_info() -> CIndicatorInfo
<name>_min_data(options) -> usize
```

Hard rules, all already solved correctly in `adosc.rs`/`macd.rs` -- copy the
pattern, don't reinvent it:

1. **Parameter order convention**: every pointer parameter is immediately
   followed by the count(s) that describe it (`inputs, data_len, options,
   ..., optional_outputs, num_optional`). Note the field is named
   `data_len`, not `size` (a C-programmer-facing name; `size` reads as
   "size in bytes" to a C caller, `data_len` unambiguously means element
   count).
2. **`#[no_mangle]` functions cannot be generic.** `<name>_simd_by_assets`
   and `<name>_simd_by_options` must be non-generic public dispatchers that
   `match num_assets` (or `num_option_sets`) against `{2, 4, 8, 16}` and
   call a private generic `<name>_simd_by_assets_n::<N>()` /
   `<name>_simd_by_options_n::<N>()` helper that does the real work.
   Anything else returns `CSimdResult::err(IndicatorError::InvalidInputs)`.
3. **Do not return references to temporaries.** For `_by_assets_n::<N>`,
   build the *owned* nested array via
   `read_simd_assets_inputs::<N, INPUTS>(inputs, data_len)`, bind it to a
   local (`let owned = ...`), then build the reference array the core API
   wants in the same stack frame: `let refs: [&[&[f64]; INPUTS]; N] =
   std::array::from_fn(|i| &owned[i]);`. Do not try to return
   `[&[&[f64]; INPUTS]; N]` directly from a helper function -- it can't
   outlive the helper's stack frame.
4. Reuse `read_inputs::<INPUTS>`, `read_simd_assets_inputs::<N, INPUTS>`,
   `read_simd_options::<N, OPTIONS>`, `optional_outputs_slice`,
   `pack_outputs`, `pack_simd_outputs`, `pack_states` from `common.rs`.
   Do not copy these into the new indicator file.
5. State handling: `<name>_indicator`/`<name>_simd_by_assets`/
   `<name>_simd_by_options` box the returned `IndicatorState` (or, for SIMD,
   each of the `Vec<IndicatorState>` via `pack_states`) as an opaque
   `*mut c_void`. `<name>_state_free` unboxes and drops it. The SIMD
   variants return *ordinary* per-result states, reusable with
   `<name>_batch`/`<name>_state_free` -- there is no separate SIMD state
   type, matching `adosc.rs`/`macd.rs`.
6. Every function needs a doc comment matching the style/detail level of
   `adosc.rs` (including a `# Safety` section), describing the actual
   `INPUTS`/`OPTIONS`/optional-output count and names for that specific
   indicator (see below for how to find these).
7. Error handling: `CIndicatorResult::err(e)` / `CBatchResult::err(e)` /
   `CSimdResult::err(e)` on any `Err`, `IndicatorError::InvalidIndicatorState`
   if `state.is_null()` in `<name>_batch`.
8. **`<name>_info()`** takes no arguments and returns
   `pack_info(&<Name>::INFO)` (a `CIndicatorInfo`) -- reuse `pack_info`
   from `common.rs` as-is, do not reimplement it. `<Name>::INFO` is a
   `tulip_rs::types::Info` static, and `pack_info` already handles leaking
   its `name`, `full_name`, `indicator_type`, `inputs`, `options`,
   `outputs`, `optional_outputs`, and `display_groups` fields into a
   C-ABI-friendly (intentionally leaked, read-only, never freed by the
   caller) struct. This function is safe (not `unsafe extern "C"`) since it
   takes no pointer arguments.
9. **`<name>_min_data(options: *const f64) -> usize`** reconstructs the
   `[f64; OPTIONS]` array from `options` (same pattern as in
   `<name>_indicator`) and returns `<Name>::min_data(&options)`. This
   function is `unsafe extern "C"` since it dereferences `options`.

## Per-indicator specifics: how to fill in the template

For each indicator, read its core module at
`tulip_rs/tulip_rs/src/indicators/<name>.rs` (confirm the exact relative
path from this repo) and extract:

- `pub const INPUTS: usize = ...;` and `pub const OPTIONS: usize = ...;`
- The `const INFO: Info = Info { ... }` block inside the `impl
  Indicator<INPUTS, OPTIONS> for <Name>` block -- specifically:
  - `inputs: &[...]` -- names/order of the `INPUTS` input series (for doc
    comments).
  - `options: &[...]` -- names/order of the `OPTIONS` values (for doc
    comments).
  - `outputs: &[...]` -- the **mandatory** output names, in order. Some
    indicators have more than one mandatory output (e.g. `macd` has three:
    `macd_line`, `signal_line`, `histogram`; `stoch` likely has two:
    `%K`/`%D`; `bbands` likely has three: upper/middle/lower -- verify per
    indicator, don't assume 1).
  - `optional_outputs: &[...]` -- optional output names, in order. **Many
    indicators have zero optional outputs** (e.g. `sma` -- verify per
    indicator). If empty, the wrapper functions still take
    `optional_outputs`/`num_optional` parameters for signature uniformity,
    but they'll just be ignored by the underlying core `indicator()` call
    (which already accepts `Option<&[bool]>` regardless).
- The type name (e.g. `pub struct Sma;` -> `Sma`, `pub struct IndicatorState`
  -> the state type) to import: `use tulip_rs::indicators::<name>::{<Name>,
  IndicatorState as <Name>State, INPUTS, OPTIONS};` (adjust for indicators
  where the exported names differ slightly -- check the actual file).
- Confirm `impl Indicator<INPUTS, OPTIONS> for <Name>` has a
  `#[cfg(feature = "simd_assets")] fn indicator_by_assets` and that there's
  a separate `#[cfg(feature = "simd_options")] impl IndicatorByOptions<...>
  for <Name>` with `fn indicator_by_options`. These exist for every
  indicator in scope for this rollout (verified during planning) except
  `candlestick`, which is out of scope. If a sub-agent finds an indicator
  genuinely missing one of these, it must stop and report back rather than
  guessing or omitting the corresponding wrapper function silently.

## Indicators in scope

All of these live in `tulip_rs/tulip_rs/src/indicators/<name>.rs`. Already
done (do not touch, do not re-assign): `adosc`, `macd`. Permanently out of
scope for this rollout: `candlestick`.

Everything else (92 indicators) needs a wrapper module. Suggested batching
(5 indicators per sub-agent call, per standing project convention) -- feel
free to adjust batch boundaries, but keep each batch at 5 or fewer:

1. `ad`, `adaptivemsw`, `adx`, `adxr`, `ao`
2. `apo`, `aroon`, `aroonosc`, `atr`, `avgprice`
3. `bbands`, `bop`, `ccfisher`, `cci`, `chaikinmf`
4. `chandelierexit`, `cmo`, `cvi`, `cybercycle`, `dema`
5. `di`, `dm`, `donchianchannel`, `dpo`, `dx`
6. `ef`, `elderray`, `ema`, `emv`, `fisher`
7. `fosc`, `highpass`, `hilberttransform`, `hma`, `homodynediscriminator`
8. `ichimoku`, `instantaneoustrendline`, `kama`, `keltnerchannel`, `kvo`
9. `linreg`, `mama`, `marketfi`, `mass`, `max`
10. `md`, `medprice`, `mfi`, `min`, `mom`
11. `msw`, `natr`, `nvi`, `obv`, `pivotpoint`
12. `ppo`, `psar`, `pvi`, `qstick`, `roc`
13. `rocr`, `roofingfilter`, `rsi`, `sma`, `smaenvelope`
14. `stddev`, `stoch`, `stochrsi`, `supersmoother`, `supertrend`
15. `tema`, `tr`, `trendmode`, `trima`, `trix`
16. `trvi`, `tsf`, `typprice`, `ultosc`, `vhf`
17. `vidya`, `volatility`, `vortex`, `vosc`, `vwap`
18. `vwma`, `wad`, `wcprice`, `wilders`, `willr`
19. `wma`, `zlema`

## Validation each sub-agent must perform before reporting back

Since writing a full C example per indicator (as was done for `adosc`/`macd`)
would be excessive for 92 indicators, use lightweight **Rust unit tests**
instead (added inside each new module file, `#[cfg(test)] mod tests { ... }`)
to catch real bugs without needing the C toolchain per module:

For each indicator in the batch, the test should:
1. Build synthetic input data (e.g. a simple increasing/oscillating series,
   `INPUTS` series of ~60 `f64`s each -- long enough to satisfy
   `min_data(options)` for reasonable option values; check the core
   indicator's `min_data`/`Self::INFO` if unsure how much data is needed).
2. Call `<name>_indicator(...)` with all optional outputs requested (if
   any), assert `result.error == CIndicatorError::Ok`, assert
   `result.num_outputs` matches the expected mandatory+optional count,
   then free via `tulip_ffi_result_free` and keep the state.
3. Call `<name>_batch(...)` with a further small chunk of synthetic data
   using that state, assert `Ok`, free outputs via
   `tulip_ffi_batch_result_free`, then `<name>_state_free(state)`.
4. Call `<name>_simd_by_assets(...)` with `num_assets = 2` (2 synthetic
   "assets" -- can just be the same series twice, or two different series),
   assert `Ok`, free each of the 2 states via `<name>_state_free`, then
   `tulip_ffi_simd_result_free`.
5. Call `<name>_simd_by_options(...)` with `num_option_sets = 2` (2 valid
   option sets), assert `Ok`, free each state via `<name>_state_free`, then
   `tulip_ffi_simd_result_free`.
6. Call `<name>_info()`, assert the returned `CIndicatorInfo`'s `inputs.len`,
   `options.len`, `outputs.len`, and `optional_outputs.len` match `INPUTS`,
   `OPTIONS`, and the expected mandatory/optional output counts from
   `<Name>::INFO`. No need to free anything (its backing memory is
   intentionally leaked, per `common.rs`'s documented convention).
7. Call `<name>_min_data(options)` with a valid options array, assert the
   result is a sane, nonzero (or otherwise expected) `usize`.
8. All of this runs inside `unsafe { ... }` blocks as needed (the wrapper
   functions are all `unsafe extern "C" fn`, except `<name>_info` which
   takes no pointer arguments and is safe to call directly).

This does not need to verify numerical correctness against a reference
(the underlying core `tulip_rs` library is already tested elsewhere) --
it just needs to prove the FFI plumbing (pointer reconstruction, packing,
freeing, state lifecycle, SIMD dispatch) doesn't panic, segfault, or leak
obviously, for every code path.

Each sub-agent's batch is done only when:
- `cargo build --release` succeeds with **no new warnings** (compare
  against a clean build before the batch started, since third-party build
  script noise like `two_bar [...]` `warning:` lines from `tulip_rs`'s
  candlestick-pattern build script is expected and unrelated).
- `cargo test --release` passes for all newly-added tests (and doesn't
  break any pre-existing ones).

## Sub-agent prompt template

When you spawn each sub-agent, give it a fully self-contained prompt (it has
no access to this conversation or this file's context beyond what you paste
in). Use this template, filling in the batch's indicator names:

> Work in the project at path `tulip_rs_ffi` (a hand-rolled `extern "C"` FFI
> wrapper crate around the core `tulip_rs` Rust indicator library). Read
> `tulip_rs_ffi/src/adosc.rs`, `tulip_rs_ffi/src/macd.rs`, and
> `tulip_rs_ffi/src/common.rs` in full first -- these establish the exact
> pattern, naming convention, and shared helpers you must reuse.
>
> Implement FFI wrapper modules for these indicators: `<name1>, <name2>,
> <name3>, <name4>, <name5>`. For each, create `tulip_rs_ffi/src/<name>.rs`
> following the `adosc.rs`/`macd.rs` pattern exactly:
>
> [... paste the full "The established pattern" and "Per-indicator
> specifics" and "Validation" sections from this document verbatim ...]
>
> Add `pub mod <name>;` to `tulip_rs_ffi/src/lib.rs` for each new module
> (keep the list alphabetically sorted, matching the existing style).
>
> Do not touch `adosc.rs`, `macd.rs`, or `common.rs`. Do not implement
> `candlestick`. Do not run `git add`/`git commit`, do not create git
> branches -- leave changes unstaged for review.
>
> Validate with `cargo build --release` (no new warnings) and `cargo test
> --release` (all tests pass, including your new ones) before reporting
> back. If you get stuck on `#[no_mangle]` + generics, or a
> borrow-of-temporary error in the SIMD input-reconstruction code, re-read
> the "hard rules" section above -- these exact bugs have occurred before
> in this codebase and are already solved in `adosc.rs`/`macd.rs`.
>
> Report back: the list of modules completed, confirmation the build and
> tests passed, and any indicator that deviated from the standard pattern
> (e.g. missing SIMD support, unusual output shape, needed a different
> INFO field name) and why.

## Orchestrator loop

1. Spawn a sub-agent for batch 1 using the template above.
2. When it reports back, independently spot-check: run `cargo build
   --release` and `cargo test --release` yourself in the project to confirm
   the sub-agent's claims (don't just trust the report). Skim at least one
   of the new files for obvious deviations from the pattern.
3. If validation fails or the pattern was violated, send a targeted
   follow-up to the same sub-agent session (not a fresh one) describing
   the exact problem and asking it to fix and re-validate.
4. Once a batch is solid, move to the next batch with a new sub-agent.
5. After all 19 batches are complete, do a final full-crate validation
   yourself (`cargo build --release`, `cargo test --release`), update
   `tulip_rs_ffi/README.md`'s "Status / next steps" checklist to reflect
   full non-candlestick indicator coverage, and report a final summary to
   the user. Do not commit -- leave everything staged/unstaged for the
   user's review, per standing project convention.

## Explicit non-goals for this rollout

- Do not write C examples (`examples/*.c`) per indicator -- Rust unit tests
  are sufficient validation for this pass. C examples can be added later,
  selectively, if the user asks.
- Do not implement `candlestick` -- explicitly out of scope, flagged for a
  separate hand-coded task.
- Do not modify the parameter order, naming convention, or shared
  `common.rs` helpers established by `adosc.rs`/`macd.rs` -- consistency
  across the whole crate matters more than any local optimization.
- Do not commit or create branches.
