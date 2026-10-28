# Supplementary Specification (Non-Functional Requirements)

| Field | Value |
|---|---|
| Template | `requirements/supplementary-spec-template.md` |
| Derived from | `AGENTS.md` ("Conventions", "Testing", "Verification", "Limits and next steps"), ADRs 0002, 0004, 0021, 0034, bug ledger, `UPSTREAM.md` |
| Source state | `cf0da55`, 2026-10-28 |

Numbered, testable statements with what verifies each ("—" = no automated check). `AGENTS.md` and the ADRs are authoritative. "Nightly" checks report, they don't block merges.

## Reliability

| ID | Requirement | Source | Verified by |
|---|---|---|---|
| NFR-REL-001 | No template, stylesheet, Fluent bundle or Markdown page makes the library panic or overflow the stack | ADR 0004, bug ledger | `tests/arbtest.rs` (`arbitrary_*_never_panics`), `tests/fuzz_corpus.rs` (replays), cargo-fuzz `css`/`html`/`ftl` (nightly, 15 min each), honggfuzz, fuzzcheck |
| NFR-REL-002 | A stylesheet that fails to load leaves an unstyled, working UI and recovers when it loads | bug_0001, bug_0004 | `tests/stateful.rs` (proptest-stateful) |
| NFR-REL-003 | A template render failure shows the error and recovers on the next good render | `AGENTS.md` Verification | `html_ui::template_render_failure_shows_the_error_and_recovers` |
| NFR-REL-004 | Correctness never depends on unspecified system order | `AGENTS.md` Verification | every `cargo test` run shuffles schedule order (`shuffle_seed`, `tests/common`) |
| NFR-REL-005 | Interaction signals follow the documented semantics for any page and pointer sequence | ADR 0006, 0011 | `tests/signal_properties.rs` (reference model), `tests/signals.rs` |
| NFR-REL-006 | A windowed build shows the same UI as an unwindowed one, except the one documented frame | ADR 0031–0033, bug_0170 | `tests/windowing_model.rs` |

## Conformance

| ID | Requirement | Source | Verified by |
|---|---|---|---|
| NFR-CONF-001 | Computed styles (font, size, color, box widths, border-image) match Chromium for every plain-HTML vector | ADR 0004 | `browser_oracle` (`tests/vectors/*/browser.json`); nightly re-checks against current Chrome |
| NFR-CONF-002 | Layout boxes match Chromium's within 1px | ADR 0004, 0007 | `layout_oracle` (`tests/vectors/layout_*`) |
| NFR-CONF-006 | Selectors match as lightningcss parsed them, with spec specificity (`:where()` zero, `:is()` its most specific argument); `:nth-child(0)` matches nothing | ADR 0034 | `browser_oracle` on `tests/vectors/selectors_level4`; `nth_child_zero_matches_no_element`; `tests/selector_model.rs` (Level 4 grammar) |
| NFR-CONF-003 | Localized output equals `@fluent/dom`'s for the same inputs | ADR 0004, 0013 | `fluent_oracle`; CI checks `fluent.html` is what `scripts/fluent_oracle.sh` produces |
| NFR-CONF-004 | Rendered pixels match the golden references on lavapipe (≤0.05 % differing pixels) | `AGENTS.md` Testing | `tests/golden.rs` (nightly) |
| NFR-CONF-005 | Every CSS deviation from the web is a documented limit and logs the skipped value at `debug` | `AGENTS.md` Limits | — (convention) |

## Performance

| ID | Requirement | Source | Verified by |
|---|---|---|---|
| NFR-PERF-001 | Unchanged subtrees skip the cascade, spec construction and diff (600-element change: ≈1.2 ms vs ≈2.1 ms without) | ADR 0021 | `benches/pipeline.rs` (criterion), `benches/pipeline-divan.rs` (nightly) |
| NFR-PERF-002 | Updates write a kept element's components only where they differ (change ticks preserved) | ADR 0009 | test vectors (`tests/html_ui.rs`) |
| NFR-PERF-003 | Hot pipeline functions don't regress beyond the hotpath policy | `AGENTS.md` Public API | `hotpath/policy.toml` |

## Supportability

| ID | Requirement | Source | Verified by |
|---|---|---|---|
| NFR-SUP-001 | The library builds warning-free with its minimal Bevy features, and with `text-input` | ADR 0002 | CI: `cargo check --lib` (both), `RUSTFLAGS=-D warnings` |
| NFR-SUP-002 | Rustdoc is warning-free and doc examples compile | `AGENTS.md` Conventions | CI: `cargo doc --no-deps`, doctests |
| NFR-SUP-003 | Every user-visible change has a `CHANGELOG.md` line under `[Unreleased]` | `AGENTS.md` Conventions | — (convention) |
| NFR-SUP-004 | Every example template renders in every locale and context, with no untranslated text | `AGENTS.md` Testing | `tests/content_lint.rs` (`every_template_is_listed` keeps it complete) |
| NFR-SUP-005 | Library code has no debug prints and writes no files | `AGENTS.md` | `source_lint::library_code_has_no_debug_prints`, `library_code_writes_no_files` |
| NFR-SUP-006 | Every defect gets a bug report and a regression test when its fix lands | `AGENTS.md` Bug reports | — (convention; ledger in `docs/agents/bugs/INDEX.md`) |

## Security

| ID | Requirement | Source | Verified by |
|---|---|---|---|
| NFR-SEC-001 | Template input is treated as untrusted: a panic reachable from user data is high severity | `AGENTS.md` Bug reports | fuzzing (NFR-REL-001) |
| NFR-SEC-002 | Tera autoescaping round-trips; entities are decoded one level only | `AGENTS.md` Testing | `template.rs` unit tests and proptest |
| NFR-SEC-005 | Selector forms parcel's matcher panics on (`:nth-child(… of S)`, `:has()`, `&` nesting, `:nth-col()`) never reach it: the allowlist skips their rules | ADR 0034 | `cascade::tests::the_engine_allowlist`; the testbench's selector-engine scenario |
| NFR-SEC-003 | Known upstream panics are patched locally until fixed upstream, and apps are told to apply the patch | `UPSTREAM.md` U1, U2 | `[patch.crates-io]` in `Cargo.toml`; README section |
| NFR-SEC-004 | The `dev-tools` surface (localhost BRP + MCP) is never enabled in a shipped build | `Cargo.toml` features | — (opt-in feature) |

## Design Constraints

| ID | Requirement | Source |
|---|---|---|
| NFR-DES-001 | Bevy 0.20; the library maps HTML/CSS/Fluent/Tera to Bevy UI and nothing else; app decisions (panels, input bindings, fonts, asset paths) stay in apps | ADR 0002 |
| NFR-DES-002 | Global configuration is resources; per-entity configuration is components; public items go in the prelude | `AGENTS.md` Conventions |
| NFR-DES-003 | When to update is decided by the pure `rebuild` state machine, unit-tested in `rebuild::tests` | ADR 0005 |

## Sources

- @AGENTS.md
- @docs/agents/adr/0002-library-shape-and-public-api.md
- @docs/agents/adr/0004-verification-strategy.md
- @docs/agents/adr/0005-update-decisions-as-a-pure-state-machine.md
- @docs/agents/adr/0009-in-place-update-model.md
- @docs/agents/adr/0021-per-element-dirty-tracking.md
- @docs/agents/adr/0031-windowed-build-spec-level-culling.md
- @docs/agents/adr/0034-match-selectors-with-parcel-selectors.md
- @docs/agents/bugs/bug_0001.md
- @docs/agents/bugs/bug_0170.md
- @docs/agents/bugs/UPSTREAM.md
