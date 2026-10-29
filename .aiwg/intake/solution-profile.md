# Solution Profile (Current System)

| Field | Value |
|---|---|
| Derived from | `AGENTS.md`, ADRs 0004, 0018, 0019, 0021, 0028, bug ledger, `UPSTREAM.md`, CI history |
| Source state | `dffb748`, 2026-10-29 |

## Current Profile

**Published library, early versions (0.x), used by the studio's own projects.** Verification is far above the norm for its size: reference oracles (Chromium styles and layout, `@fluent/dom`), golden images, property, stateful and model-based tests, five fuzzing drivers, mutation testing, coverage per testing layer, benchmarks. The weak spot is operations: the nightly workflow that runs the oracles, fuzzers and coverage was red for two weeks before its causes were fixed (2026-10-28); a timing-based CI test also failed on a slower runner once, a real quadratic (bug_0181).

| Dimension | Level | Evidence |
|---|---|---|
| Security | Strong for scope | untrusted-input stance, fuzzing, two upstream panics patched (U1, U2) |
| Reliability | High | 179 of 181 filed bugs fixed with regression tests (the other two: one won't-fix, one withdrawn); pure update state machine; schedule-order shuffling in every test run |
| Testing & quality | Very high | oracles, golden images, property/model tests, fuzzing, mutants, coverage (88–98 % by layer) |
| Process rigor | High | 37 ADRs (0034 and 0036 accepted after measured trials; 0035 and 0037 rejected on measured evidence), bug reports, six review sessions, changelog convention, CI on every push |
| Operations | Medium | nightly green again since 2026-10-29 (red 2026-10-14 to 2026-10-28); 0.8.0 released 2026-10-29 (the consumers still on 0.7.0 and 0.4.0) |

## Recommended Profile Adjustments

- Keep the nightly green: triage a red one within days and treat it as a reason to file bugs, not noise (R-001).
- Move the consumers to 0.8.0 with its upgrade notes: vtuber from 0.7.0, prototype_19 from 0.4.0 (R-004).

## Improvement Roadmap

1. ADR 0028 tier 4: SVG, `@media`, shadows, gradients, `@keyframes`.
2. Decide `flex-shrink` deliberately (re-derive the vectors against browser defaults).
3. Get the fluent-syntax fixes merged upstream so apps no longer need the patch.

## Sources

- @AGENTS.md
- @docs/agents/adr/0004-verification-strategy.md
- @docs/agents/adr/0021-per-element-dirty-tracking.md
- @docs/agents/adr/0028-modern-authoring-gaps-roadmap.md
- @docs/agents/adr/0035-replace-tl-with-html5ever.md
- @docs/agents/adr/0036-decode-character-references-with-htmlize.md
- @docs/agents/adr/0037-anchored-overlays-with-bevy-popover.md
- @docs/agents/bugs/INDEX.md
- @docs/agents/bugs/bug_0181.md
- @docs/agents/bugs/UPSTREAM.md
