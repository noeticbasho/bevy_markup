# ADR 0034 trial: selector matching with parcel_selectors

| Field | Value |
|---|---|
| Date | 2026-10-28 19:21 +0400 |
| Author | Claude Opus 5.5 (Anthropic), via Claude Code |
| Branch | `spike/adr-0034-parcel-selectors`, from `cca245b Add ADR 34`; benchmark commit `d0ccd7b`, the rest uncommitted |
| Machine | Arch Linux (kernel 7.2.11), AMD Ryzen 7 9800X3D, rustc 1.98.1; Chromium 156 for the oracle |

The trial replaced the hand-written selector parser and matcher with
lightningcss's engine (`parcel_selectors` 0.28.3) and measured
correctness and performance against the old code. Phase A kept the
supported grammar identical; phase B turned on the new forms.

## What was built

- `src/cascade/selector.rs`: `Selector::from_parsed` (the allowlist, the
  `SelectorReads` and the specificity, from lightningcss's components) and
  `Node`, an `Element` implementation. The old `Selector`, `Compound`,
  `StructuralPseudo`, `AnB`, `AttributeSelector`, `Tri`, `Subject` and the
  string round-trip are gone (641 code lines removed from `cascade.rs`;
  547 code lines added in `selector.rs`).
- **No separate element arena** (ADR 0034 decision 2 proposed one). The
  build walk already holds what the engine needs: the ancestor chain
  (nearest first) and each parent's shared `SiblingList`. Each
  `Ancestor` now carries a `SiblingLink` (its place in its parent's
  list), so every node the engine reaches has a parent and both
  siblings. Where no list is threaded (unit tests, image runs, the
  root), stand-in siblings are derived from the element's `Position`, so
  positional matches agree with it.
- **Reactivity (open question 1).** In ADR 0029's "could it match either
  way" mode, an interaction pseudo-class answers `!context.in_negation()`:
  true outside a `:not()`, false inside. That is the old three-valued
  logic exactly, provided no `:not()` sits inside another, which the
  allowlist rejects (also through `:is()`).
- **Cache keys (open question 2)** needed no change: positions were
  already in the key, and the new positional forms (`:nth-last-*`,
  `:only-*`) read only what `Position` holds. Sibling combinators away
  from the subject stay off (below), so no new sibling reads.
- `parcel_selectors = "=0.28.3"` as a direct dependency, pinned to
  lightningcss's.

## Correctness

| Check | Phase A (parity) | Phase B (new grammar) |
|---|---|---|
| `cargo test` (all binaries) | pass | pass |
| Browser oracle (Chrome) | all vectors pass | all pass, plus the new `selectors_level4` vector (15 elements, 9 runs) |
| `tests/selector_model.rs`, 5000 cases (`PROPTEST_CASES`) | pass | pass, grammar extended with `:is()`, `:where()`, `:not()` lists, `:nth-last-*`, `:only-*`, `^=`/`$=` |
| Differential: `fuzz::cascade` over 28 237 css corpus inputs, old vs engine | 1 difference | 1 difference (same) |
| cargo-fuzz `css` | — | 300 s, 985 814 runs, no crash or timeout (cov 14 399, from 11 359 on the old matcher) |
| Fuzz corpus replay | pass | pass |
| Warning-free `check --lib` (± `text-input`), `build --all-targets`, `doc`; clippy on the new module | — | clean |

**The one differential difference** is `:nth-child(-n + 0)`: no element
sits at position 0, but the old matcher matched elements with no
position, which includes the root `html` (its style is computed with
`ElementState::default()`). On `main`, `:nth-child(0) { color: red }`
colours the whole page. The engine matches nothing, as Chrome does.
Regression test: `nth_child_zero_matches_no_element` (fails on `main`).

**The new vector fails on `main`** (`browser_oracle`), so it tests what
the engine added.

**Engine divergence found:** parcel computes `index - b` in checked
`i32`. For `:nth-child(n-2147483647)` it overflows and answers no, where
CSS matches every element (the old matcher used `i64`). No document has
the 2^31 siblings that would tell. The tests record the engine's answer
(`an_b_extreme_offsets_match_without_overflow`,
`nth_child_extreme_offsets_neither_panic_nor_mismatch`). An upstream
report candidate.

**Engine panics guarded:** `the_engine_allowlist` feeds `:nth-child(… of
…)`, `:has()`, `:nth-col()` and nested `&` rules through `from_sheet` and
a match; the fuzz run above was seeded with them.

## Performance

`benches/` `selectors` workload (added for the trial in `d0ccd7b`, before
the change): cards of ten-item lists under a sheet of every supported
selector kind plus 80 near-miss rules, the whole document re-collected
per frame. Criterion medians, `--baseline before`; divan medians in
brackets.

| Bench | Before | Phase A | Phase B |
|---|---|---|---|
| frame/selectors/120 | 1.478 ms [1.387] | 1.361 ms, −7.9 % [1.284] | 1.367 ms, −8.4 % [1.285] |
| frame/selectors/600 | 6.133 ms [5.761] | 6.053 ms, −1.3 % [5.694] | 6.037 ms, −1.6 % [5.625] |
| frame/rebuilding/120 | 2.408 ms | 2.338 ms, −2.9 % | 2.312 ms, −4.0 % |
| frame/rebuilding/600 | 10.40 ms | 10.37 ms | 10.40 ms |
| frame/changing/600 | 1.536 ms | 1.511 ms | 1.531 ms |
| divan hover_flicker/600 | 423.5 µs | 392.0 µs | — |
| divan focus_navigation/600 | 195.7 µs | 184.2 µs | — |

The noise floor is about ±5 %: `render` and `parse`, which the change
doesn't touch, moved 2–7 % between runs. `markdown` read +4 % in one
divan run; re-runs gave 3.47–3.58 ms / 18.5–18.7 ms against
3.58 / 18.9 ms on the old matcher, so no regression. Allocation counts
are unchanged. The ADR's budget (no regression above 10 %) holds with
room; the Bloom filter wasn't needed.

## Not done

- **Sibling combinators away from the subject** (`.a + .b p`) stay
  skipped. The engine matches them, but the cascade can't invalidate
  them: a hover flip on `.a` must restyle elements inside its following
  sibling, and ADR 0029's restyle scoping covers only the element's
  chain and its own siblings. A separate change.
- **Attribute selectors on non-`data-*` attributes**: the element model
  carries only the dataset; those rules still skip.
- `:root`, `:has()`, `:nth-child(… of …)`, pseudo-elements: skipped.
- Golden images weren't run locally (lavapipe; the nightly runs them).
