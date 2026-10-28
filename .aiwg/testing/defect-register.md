# Defect Register

| Field | Value |
|---|---|
| Derived from | `docs/agents/bugs/INDEX.md`, `UPSTREAM.md`, `docs/agents/reviews/`, CI history |
| Source state | `f6a2d6a`, 2026-10-28 |

An index; the bug reports are authoritative. 178 reports, bug_0001–0178 (2026-10-04 to 2026-10-26).

## By Status and Severity

| Status | Count |
|---|---|
| Fixed, with regression test | 176 |
| Won't fix (known limit) | 1 — bug_0170, a one-frame row shift in the windowed build (low) |
| Withdrawn (not a bug) | 1 — bug_0026 |

| Severity | Count |
|---|---|
| High (reachable from user data, panics included) | 46 |
| Medium | 77 (+1 low–medium) |
| Low | 53 |

No open bugs.

## How They Were Found

Property testing, stateful testing and fuzzing (bug_0001–0005), the Fluent and layout oracles (0006–0013), mutation-testing triage, corpus coverage replays, example smoke runs, prototype_19 integration (0021), user reports (0023, 0028–0030), and whole-crate review sessions: review_0001–0006 found most of bug_0031 onward, each finding filed when its fix landed. Per-session metadata in `INDEX.md`.

## Upstream Defects (`UPSTREAM.md`)

U1–U13: fluent-syntax panic and stack overflow (forked, PR pending), pulldown-cmark panic (U13, fixed by updating), honggfuzz-rs, fuzzcheck (three, vendored patches), cargo-mutants, cargo-fuzz, Bevy picking (two), lightningcss, tl (switched to `astral-tl`).

## Untriaged (nightly, 2026-10-28)

Not bug reports yet; see risk R-001.

| Job | Finding |
|---|---|
| Fuzz css (cargo-fuzz) | timeout after 15 s on one input (`timeout-849f2243…`; also a slow unit) |
| Coverage per testing layer | stack overflow replaying the fuzz corpora (`fuzz_corpus -- --ignored`) under llvm-cov |
| Browser oracle | current Chrome computes differently from the committed `browser.json` |

## Sources

- @docs/agents/bugs/INDEX.md
- @docs/agents/bugs/UPSTREAM.md
- @docs/agents/bugs/bug_0026.md
- @docs/agents/bugs/bug_0170.md
- @docs/agents/reviews/review_0006.md
