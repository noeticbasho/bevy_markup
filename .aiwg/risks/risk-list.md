# Risk List

| Field | Value |
|---|---|
| Template | `management/risk-list-template.md` |
| Derived from | `AGENTS.md`, ADRs 0028, 0034–0037, bug ledger, `UPSTREAM.md`, `CHANGELOG.md`, CI history (GitHub Actions, 2026-10-29) |
| Source state | `7792cea`, 2026-10-29 |
| Owner | Project owner (all risks) |

Scales High / Medium / Low; Exposure = Impact × Likelihood. Never renumber.

## Register

| Risk ID | Description | Impact | Likelihood | Exposure | Mitigation Strategy | Owner | Status | Residual Risk |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| R-001 | The nightly workflow failed every night 2026-10-14 to 2026-10-28, so its findings went unseen (and timeouts masked a panic, bug_0180). Causes: two lightningcss defects and a vector without its Chrome reference (defect register). | High | High | HxH=High | Fixed (bug_0179, bug_0180, oracle script; merged to `main` 2026-10-28); no nightly has run since (last: 2026-10-28 09:40 UTC, before the fixes). Confirm the next run, keep it green so a new failure is a signal, and triage a red night within days. CI itself is green at `0262408` (a timing test that failed on CI was a real quadratic, bug_0181, fixed). | Owner | Mitigating | Low once green |
| R-002 | Two upstream fluent-syntax defects (panic, stack overflow from any FTL asset) are fixed only in a fork applied by `[patch.crates-io]`, which works only in the top-level project: apps without the same entry inherit them (both studio apps have it: prototype_19, and vtuber since 2026-10-28). | High | Medium | HxM=High | README asks apps to add the patch; get the upstream PRs merged (U1, U2), then drop the fork. | Owner | Mitigating | Low after upstream fix |
| R-003 | CSS copied from the web misbehaves silently where the subset deviates, notably `flex-shrink` defaulting to 0 (bug_0028); skipped values are logged only at `debug`. | Medium | High | MxH=High | Decide `flex-shrink` as a deliberate change (re-derive vectors against browser defaults); keep limits documented. | Owner | Open | Medium |
| R-004 | Releases lag `main`: a large `[Unreleased]` changelog since 0.7.0 (2026-10-15), now with breaking changes (argument attributes need `\| json`; `Focusable.tab_index`, `HtmlAnchor.flip`; anchors flip by default); prototype_19 still on 0.4.0, so its fixes and features don't reach the studio's game, and each consumer's migration grows. | Medium | Medium | MxM=Medium | Release from `[Unreleased]` as a minor bump with migration notes; bump prototype_19 (a migration across 0.5–0.8) and vtuber (0.7 → next: its templates' args need `\| json`). | Owner | Open | Low |
| R-005 | Each Bevy release changes UI internals the library leans on (0.20's weakly ordered `UiSystems`, picking, text): migrations are large and can break subtly. | Medium | High | MxH=High | Schedule-order shuffling in every test run; oracles and golden images catch regressions; pin explicit ordering. | Owner | Accepted | Medium |
| R-006 | Golden images depend on the CI runner's Mesa (lavapipe is canonical; `ubuntu-latest` moved to Ubuntu 26 on 2026-10-19): a renderer change fails the job without a code change. | Low | Medium | LxM=Low | Re-record with the `Update golden references (lavapipe)` job; `runs-on: ubuntu-24.04` as fallback. | Owner | Accepted | Low |
| R-007 | The repository lives on the personal `nchashch` account while the studio's other repositories are on the `noeticbasho` org. | Medium | Low | MxL=Low | Transfer to the org when convenient (crates.io `repository` field follows). | Owner | Open | Low |
| R-008 | `AGENTS.md` is ~130 KB; every agent session that loads it spends a large share of its context before work starts, and keeping one file that long current invites drift. | Medium | Medium | MxM=Medium | Keep reference material (Testing history, Gotchas) in linked files and the file to current state. | Owner | Open | Low |
| R-009 | Selector matching depends on parcel_selectors, pinned to lightningcss's version: its matcher panics on four parsed forms (kept out by the allowlist), and its checked `i32` An+B answers no for `:nth-child(n-2147483647)`. A lightningcss upgrade can add parts the allowlist doesn't know. | Medium | Low | MxL=Low | Bump both together; `the_engine_allowlist` and the selector-engine testbench scenario on every upgrade; report the An+B overflow upstream. | Owner | Mitigating | Low |
| R-011 | Character-reference decoding depends on `htmlize`, a small crate with one maintainer: a stalled crate would leave a spec change (rare) or a defect unaddressed. | Low | Low | LxL=Low | The algorithm is the spec's and stable; the differential test against the old decoder and the `character_references` oracle vector pin the behaviour; vendoring or restoring the generated table is a small change (ADR 0036). | Owner | Accepted | Low |
| R-010 | Sibling combinators away from the subject stay unsupported: matching works, but a hover flip must restyle elements inside the following sibling, which ADR 0029's scoping doesn't cover. | Low | Medium | LxM=Low | A design change of its own (restyle invalidation across a sibling's subtree) when a page needs it. | Owner | Accepted | Low |

## Sources

- @AGENTS.md
- @CHANGELOG.md
- @docs/agents/adr/0028-modern-authoring-gaps-roadmap.md
- @docs/agents/adr/0034-match-selectors-with-parcel-selectors.md
- @docs/agents/adr/0036-decode-character-references-with-htmlize.md
- @docs/agents/bugs/bug_0028.md
- @docs/agents/bugs/bug_0181.md
- @docs/agents/bugs/UPSTREAM.md
- @docs/agents/bugs/bug_0179.md
- @docs/agents/skills/testing.md
