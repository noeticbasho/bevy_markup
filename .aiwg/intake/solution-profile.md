# Solution Profile (Current System)

| Field | Value |
|---|---|
| Derived from | `AGENTS.md`, ADRs 0004, 0018, 0019, 0021, 0028, bug ledger, `UPSTREAM.md`, CI history |
| Source state | `f6a2d6a`, 2026-10-28 |

## Current Profile

**Published library, early versions (0.x), used by the studio's own projects.** Verification is far above the norm for its size: reference oracles (Chromium styles and layout, `@fluent/dom`), golden images, property, stateful and model-based tests, five fuzzing drivers, mutation testing, coverage per testing layer, benchmarks. The weak spot is operations: the nightly workflow that runs the oracles, fuzzers and coverage has been red for two weeks without triage.

| Dimension | Level | Evidence |
|---|---|---|
| Security | Strong for scope | untrusted-input stance, fuzzing, two upstream panics patched (U1, U2) |
| Reliability | High | 176 of 178 filed bugs fixed with regression tests; pure update state machine; schedule-order shuffling in every test run |
| Testing & quality | Very high | oracles, golden images, property/model tests, fuzzing, mutants, coverage (88–98 % by layer) |
| Process rigor | High | 33 ADRs, bug reports, six review sessions, changelog convention, CI on every push |
| Operations | Medium | nightly red since 2026-10-14; releases lag `main` (large `[Unreleased]`) |

## Recommended Profile Adjustments

- Triage the nightly failures and get it green again; treat a red nightly as a reason to file bugs, not noise (R-001).
- Release the `[Unreleased]` work and move prototype_19 off 0.4.0 (R-004).

## Improvement Roadmap

1. ADR 0028 tier 4: SVG, `@media`, shadows, gradients, `@keyframes`.
2. Decide `flex-shrink` deliberately (re-derive the vectors against browser defaults).
3. Get the fluent-syntax fixes merged upstream so apps no longer need the patch.

## Sources

- @AGENTS.md
- @docs/agents/adr/0004-verification-strategy.md
- @docs/agents/adr/0021-per-element-dirty-tracking.md
- @docs/agents/adr/0028-modern-authoring-gaps-roadmap.md
- @docs/agents/bugs/INDEX.md
- @docs/agents/bugs/UPSTREAM.md
