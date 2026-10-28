# Test Strategy

| Field | Value |
|---|---|
| Template | `test/test-strategy-template.md` (condensed) |
| Derived from | `AGENTS.md` ("Testing", "Verification"), `docs/agents/skills/testing.md`, ADRs 0004, 0018, 0019 |
| Source state | `f6a2d6a`, 2026-10-28 |

## Context and Objectives

The library's promise is that HTML/CSS/Fluent behave as an author expects and that no input breaks it. ADR 0004 sets the method: verify against reference implementations (Chromium, `@fluent/dom`), then attack with every kind of generated input. Agents change the code often, so the suite is the safety net. How-to: @docs/agents/skills/testing.md.

## Test Levels and Scope

| Level | What | Where | When |
|---|---|---|---|
| Unit | cascade, templates and entities, l10n, fonts, 9-slice, rebuild state machine | `src/*` `#[cfg(test)]` | every push |
| Test vectors | inputs → text dump of the built world (24 vector directories plus inline vectors) | `tests/html_ui.rs`, `tests/vectors/` | every push |
| Oracles | computed styles and layout vs Chromium; localized output vs `@fluent/dom` | `browser_oracle`, `layout_oracle`, `fluent_oracle` | every push; nightly re-checks Chrome |
| Interaction | signals over the real picking stack; property test against a reference model | `tests/signals.rs`, `tests/signal_properties.rs` | every push |
| Property and model-based | layout invariants, selector model, stateful op sequences, windowing model, quickcheck, arbtest | `tests/layout_properties.rs`, `selector_model.rs`, `stateful.rs`, `windowing_model.rs`, `quickcheck.rs`, `arbtest.rs` | every push |
| Lints | examples' content (locales, templates, CSS); library source (no prints, no file writes) | `tests/content_lint.rs`, `tests/source_lint.rs` | every push |
| Golden images | rendered scenes vs references on lavapipe | `tests/golden.rs`, `scripts/golden.sh` | nightly (ignored locally without the flag) |
| Fuzzing | cargo-fuzz `css`/`html`/`ftl` (15 min each, corpora persisted), honggfuzz, fuzzcheck, test-fuzz; corpus replay | `fuzz/`, `honggfuzz/`, `fuzzcheck/`, `test-fuzz/`, `tests/fuzz_corpus.rs` | nightly |
| Mutation | cargo-mutants, sharded | `mutants.yml` | scheduled |
| Coverage | line coverage per testing layer | `scripts/coverage.py` | nightly |
| Performance | criterion and divan suites; hotpath policy | `benches/`, `hotpath/policy.toml` | nightly |
| Visual / agent | screenshots and playtests of the examples | `--features dev-tools`, bevy_mcp_harness | by hand |

**In scope:** the library and its examples. **Out of scope:** Bevy itself, the apps using the library (they test their own UI), upstream crates (tracked in `UPSTREAM.md`).

## Automation Strategy

`ci.yml` on every push and PR: warning-free checks and docs, `cargo test` (all of the push-time levels above), the Fluent oracle regeneration check. `nightly.yml`: golden images, browser oracle vs current Chrome, fuzzing, coverage, benchmarks — failures are reports to triage, not merge blockers. `cache.yml` builds shared caches once. Schedule order is shuffled in every test run.

Status on 2026-10-28: CI green on the last push; the nightly red every night since 2026-10-14 (risk R-001).

## Defect Management

Every defect gets `docs/agents/bugs/bug_NNNN.md` with discovery, minimal reproduction, root cause, fix commit and regression test; ledger in `INDEX.md`; upstream defects in `UPSTREAM.md`; whole-crate review sessions in `docs/agents/reviews/`. Index: @.aiwg/testing/defect-register.md.

## Requirements and Risk Coverage

Each requirement's "Verified by" column in @.aiwg/requirements/supplementary-spec.md names its check. No automated check: NFR-CONF-005, NFR-SUP-003, NFR-SUP-006, NFR-SEC-004 (conventions). Coverage was last measured at 88.1 % merged over ~3112 lines (2026-10-05); mutation results and coverage are read together (`AGENTS.md`).

## Sources

- @AGENTS.md
- @docs/agents/skills/testing.md
- @docs/agents/adr/0004-verification-strategy.md
- @docs/agents/adr/0018-benchmarks-with-criterion.md
- @docs/agents/adr/0019-benchmarks-with-divan.md
