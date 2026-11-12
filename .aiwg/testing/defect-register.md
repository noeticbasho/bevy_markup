# Defect Register

| Field | Value |
|---|---|
| Derived from | `docs/agents/bugs/INDEX.md`, `UPSTREAM.md`, `docs/agents/reviews/`, CI history |
| Source state | bug_0238 (after `ffc9a12`), 2026-11-12 |

An index; the bug reports are authoritative. 238 reports, bug_0001–0238 (2026-10-04 to 2026-11-12).

## By Status and Severity

| Status | Count |
|---|---|
| Fixed, with regression test | 234 |
| Fixed, measured (no test) | 1 — bug_0211, an idle-frame cost |
| Won't fix (known limit) | 1 — bug_0170, a one-frame row shift in the windowed build (low) |
| Withdrawn (not a bug) | 1 — bug_0026 |
| Open upstream, no local workaround | 1 — bug_0203, tall inline images and parley 0.11's line metrics (low; fixed in parley 0.12, which Bevy 0.20 doesn't use) |

| Severity | Count |
|---|---|
| High (reachable from user data, panics included) | 74 |
| Medium | 94 (+1 low–medium) |
| Low | 68 |

bug_0203 waits on a Bevy release (UPSTREAM U37). review_0007's findings were filed up front as open reports bug_0204–0221 (on request; the usual rule files a finding when its fix lands); all were fixed the same day (three after the threat-model decision, ADR 0044: every input is untrusted). review_0008's were filed the same way (bug_0223–0237) and fixed the next day (ADR 0046); the fixes found two more upstream defects (U42 simplecss's quadratic parse, U43 usvg's per-span reshaping).

## Open Review Findings (review_0007, 2026-11-11, at `f76b320`)

18 findings, `K01`–`K18`; 14 reproduced by probes. Fixed the same day (`d7ddbe2`): K01–K17. K18 is records drift. Filed as open reports: K01–K14 → bug_0204–0217, K15 → bug_0218 (+ bug_0219), K16 → bug_0220, K17 → bug_0221; K18 (records drift) isn't a defect and has no report.

| Severity | Findings |
|---|---|
| High | K01 (a pattern on SVG text aborts the process, `svg-text`), K10 (nested `:is()` with descendant combinators: 104 B of CSS, a 121 s build) |
| Medium | K02 (dashed strokes ~1000× their cost estimate), K03 (glyph outlines uncosted), K04 (`<style>` rules × elements quadratic), K05 (a refused inline `<svg>` re-parsed per build), K06 (no bound on the rasters' sum), K11 (`%` image sizes wrong at scale factors other than 1), K12 (a framed canvas loses its picture or frame), K13 (a hidden canvas resizes its camera's image to 1 × 1) |
| Low | K07–K09, K14–K18 |

Decided (ADR 0044): every input is untrusted, templates and CSS included; templates are bounded by a static check at load (`src/tera_check/`).

## Open Review Findings (review_0008, 2026-11-12, at `54bad82`)

16 findings, `L01`–`L16`; 14 reproduced by probes. Fixed 2026-11-12: L01–L14, L16; L15's records drift corrected where it was this repo's records (the INDEX statuses, the README's claim, AGENTS.md's layout). Filed as open reports (2026-11-12): L01–L14 → bug_0223–0236, L16 → bug_0237; L15 (records drift) isn't a defect and has no report.

| Severity | Findings |
|---|---|
| High | L01 (the template check overflows the asset thread's stack: abort; a regression), L02 (`set_global` in a component body passes the check: bug_0220's abort), L03 (`get`'s `default` isn't bounded: the same abort), L04 (loops over context data count once at any depth), L07 (a descendant selector in SVG `<style>`: 555 B, a 36 s parse) |
| Medium | L05 (building a value costs one step), L06 (a counter in two nested loops is refused; a regression), L08 (the SVG drawing estimate misses overlap: round-capped dashes 33 s at 48% of the budget), L09 (`<style>` charge assumes cheap selectors), L10 (SVG text conversion quadratic, `svg-text`), L11 (a `pointer-events: none` canvas swallows clicks; a regression) |
| Low | L12 (the canvas picture ignores `border-radius`; a regression), L13–L16 |

## How They Were Found

Property testing, stateful testing and fuzzing (bug_0001–0005), the Fluent and layout oracles (0006–0013), mutation-testing triage, corpus coverage replays, example smoke runs, prototype_19 integration (0021), user reports (0023, 0028–0030), and whole-crate review sessions: review_0001–0006 found most of bug_0031 onward, each finding filed when its fix landed; CI itself (bug_0181, a scaling test); nightly fuzzing and its triage (bug_0182–0184, bug_0188–0202); the ADR 0038 SVG work's tests, testbench and example (bug_0185–0187), and its layout oracle vector (bug_0203); review_0007 (bug_0204–0221, filed open); review_0008 (bug_0223–0237, filed open); the first nightly run of every fuzz driver (bug_0238, honggfuzz). Per-session metadata in `INDEX.md`.

## Upstream Defects (`UPSTREAM.md`)

U1–U15: lightningcss exponential parsing and unbounded recursion (U14, U15; worked around by the nesting pre-scan), fluent-syntax panic and stack overflow (refused at load by `ftl_check`, ADR 0045; the fork's PR pending), pulldown-cmark panic (U13, fixed by updating), honggfuzz-rs, fuzzcheck (three, vendored patches), cargo-mutants, cargo-fuzz, Bevy picking (two), lightningcss, tl (switched to `astral-tl`). U38: Bevy's viewport resize unwrapping a missing target image (bug_0218; bound only once the image exists). U39: Tera's unbounded render (bug_0220; the load-time check, ADR 0044). U40: simplecss's exponential descendant matching (bug_0229) and U42 its quadratic parse (bug_0231); U41: usvg's text layout quadratic in a chunk's characters, U43 its per-span reshaping (bug_0232) — all four charged into `MAX_SVG_WORK` before they run. U16–U37: parley 0.11's line metrics for tall inline boxes (U37, bug_0203; fixed in parley 0.12, no workaround), Tera's operator recursion (U16), astral-tl's `<b/>` (U21), and the SVG stack: usvg's unbounded recursion and conversion work (U17, U25; the depth and work bounds, ADR 0040), roxmltree (U18, version floor), usvg's file-reading resolver (U19), resvg's turbulence overflow under debug assertions (U20; `svg::guarded`, excused by the `svg` fuzz target's panic hook) and box blur overflow (U32; refused past `MAX_SVG_BLUR`, bug_0198), tiny-skia-path's `to_int_rect` unwraps (U35; filter regions and offsets refused past `MAX_SVG_FILTER_REGION`, bug_0201), resvg's unbounded pattern tiles (U36; charged by the cost estimate, bug_0200), Bevy's picking ignoring inline boxes (U34, bug_0199) and needing a primary window (U33), tiny-skia's far-off-canvas panics in every build (U24, U27, U28; refused before drawing by `MAX_SVG_EXTENT`, bug_0194), usvg's infinite-`baseFrequency` and `feMorphology` radius unwraps and `feConvolveMatrix` `order` overflow (U26, U30, U29; refused before conversion, bug_0192, bug_0196, bug_0195), tiny-skia-path's overflowed bounding-box rect (U31; boxes measured by a first usvg pass, ADR 0042, bug_0197), svgtypes/kurbo arc subdivision (U22) and usvg's self-inheriting pattern (U23).

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
| Fuzz svg (5 s timing assertion) | a 1.6 KB `<use>` fan-out was refused as too expensive to draw only after usvg converted 10⁵ copies (8.8 s on the runner) | bug_0188: the conversion-work bound (ADR 0040), which also covers markers (bug_0189) and arcs (bug_0190); the triage found bug_0191 (a self-inheriting pattern aborting the process) and, fuzzing the fix, bug_0192 (usvg's `baseFrequency` unwrap) and bug_0193 (an arc the bound undercounted from a rounded start point); a release replay found bug_0194 (tiny-skia panicking far off the canvas), and fuzzing that fix found bug_0195 (usvg's convolve `order` overflow), and fuzzing that fix found bug_0196 (usvg's morphology radius unwrap) and, probing it, bug_0197 (a filter over a huge box; ADR 0042); the nightly on that fix found bug_0198 (resvg's box blur overflow) |

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
- @docs/agents/bugs/bug_0203.md
- @docs/agents/bugs/bug_0204.md (… bug_0221: review_0007's findings)
- @docs/agents/adr/0040-svg-conversion-work-bound.md
- @docs/agents/performance/adr-0034-trial.md
- @docs/agents/reviews/review_0006.md
- @docs/agents/reviews/review_0007.md
- @docs/agents/reviews/review_0008.md
