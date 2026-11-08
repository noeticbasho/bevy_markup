# Defect Register

| Field | Value |
|---|---|
| Derived from | `docs/agents/bugs/INDEX.md`, `UPSTREAM.md`, `docs/agents/reviews/`, CI history |
| Source state | `ff99cc1` + the ADR 0040 change, 2026-11-08 |

An index; the bug reports are authoritative. 191 reports, bug_0001–0191 (2026-10-04 to 2026-11-08).

## By Status and Severity

| Status | Count |
|---|---|
| Fixed, with regression test | 189 |
| Won't fix (known limit) | 1 — bug_0170, a one-frame row shift in the windowed build (low) |
| Withdrawn (not a bug) | 1 — bug_0026 |

| Severity | Count |
|---|---|
| High (reachable from user data, panics included) | 56 |
| Medium | 79 (+1 low–medium) |
| Low | 54 |

No open bugs.

## How They Were Found

Property testing, stateful testing and fuzzing (bug_0001–0005), the Fluent and layout oracles (0006–0013), mutation-testing triage, corpus coverage replays, example smoke runs, prototype_19 integration (0021), user reports (0023, 0028–0030), and whole-crate review sessions: review_0001–0006 found most of bug_0031 onward, each finding filed when its fix landed; CI itself (bug_0181, a scaling test); nightly fuzzing and its triage (bug_0182–0184, bug_0188–0191); the ADR 0038 SVG work's tests, testbench and example (bug_0185–0187). Per-session metadata in `INDEX.md`.

## Upstream Defects (`UPSTREAM.md`)

U1–U15: lightningcss exponential parsing and unbounded recursion (U14, U15; worked around by the nesting pre-scan), fluent-syntax panic and stack overflow (forked, PR pending), pulldown-cmark panic (U13, fixed by updating), honggfuzz-rs, fuzzcheck (three, vendored patches), cargo-mutants, cargo-fuzz, Bevy picking (two), lightningcss, tl (switched to `astral-tl`). U16–U25: Tera's operator recursion (U16), astral-tl's `<b/>` (U21), and the SVG stack: usvg's unbounded recursion and conversion work (U17, U25; the depth and work bounds, ADR 0040), roxmltree (U18, version floor), usvg's file-reading resolver (U19), resvg and tiny-skia panics under debug assertions (U20, U24; `svg::guarded`), svgtypes/kurbo arc subdivision (U22) and usvg's self-inheriting pattern (U23).

## Nightly Failures (2026-10-14 to 2026-10-28, fixed on `main`)

| Job | Cause | Fix |
|---|---|---|
| Fuzz css (timeouts) | lightningcss parses nested `light-dark(` / `calc(var(…))` in exponential time, inside the old 32-block bound | bug_0179: paren bound 8 |
| Coverage (stack overflow in `replay_css_corpus`) | the css harness skipped the loader's nesting pre-scan; lightningcss recursion is unbounded | bug_0179: the harness applies the pre-scan |
| Browser oracle | `tests/vectors/text_props` had no `browser.json`; the oracle script recorded text without `text-transform` | the script reads rendered text; reference committed |

The ADR 0034 trial's differential run found one more old-matcher defect, fixed by the engine without a bug report: `:nth-child(0)` matched the root (CHANGELOG "Fixed"; test `nth_child_zero_matches_no_element`).

The timeouts had masked bug_0180 (a panic in `Compound::parse`), found by a local fuzz run after the fix.

## Nightly Failure (`ff99cc1`, 2026-11-08, fixed)

| Job | Cause | Fix |
|---|---|---|
| Fuzz svg (5 s timing assertion) | a 1.6 KB `<use>` fan-out was refused as too expensive to draw only after usvg converted 10⁵ copies (8.8 s on the runner) | bug_0188: the conversion-work bound (ADR 0040), which also covers markers (bug_0189) and arcs (bug_0190); the triage found bug_0191 (a self-inheriting pattern aborting the process) |

## CI Failure (`f8629c2`, 2026-10-28, fixed)

| Test | Cause | Fix |
|---|---|---|
| `review_0005::long_sibling_lists_build_linearly` (9.7× for 4× the siblings; bound 8×) | `X ~ subject` walked back through every preceding sibling per element: O(n²) over a list without a match; ADR 0034's engine made each step heavier, so it crossed the bound on CI | bug_0181: one forward scan per list and selector (6,400 siblings 1.6 s → 0.24 s) |

The ADR 0036 change (htmlize) intentionally changed decoding where the spec decodes and the crate didn't (legacy references without `;`); it is a behaviour change with a CHANGELOG entry, not a defect.

## Sources

- @docs/agents/bugs/INDEX.md
- @docs/agents/bugs/UPSTREAM.md
- @docs/agents/bugs/bug_0026.md
- @docs/agents/bugs/bug_0170.md
- @docs/agents/bugs/bug_0179.md
- @docs/agents/bugs/bug_0180.md
- @docs/agents/bugs/bug_0181.md
- @docs/agents/bugs/bug_0188.md
- @docs/agents/adr/0040-svg-conversion-work-bound.md
- @docs/agents/performance/adr-0034-trial.md
- @docs/agents/reviews/review_0006.md
