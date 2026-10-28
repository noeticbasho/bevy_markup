# Risk List

| Field | Value |
|---|---|
| Template | `management/risk-list-template.md` |
| Derived from | `AGENTS.md`, ADRs 0028, bug ledger, `UPSTREAM.md`, `CHANGELOG.md`, CI history (GitHub Actions, 2026-10-28) |
| Source state | `f6a2d6a`, 2026-10-28 |
| Owner | Project owner (all risks) |

Scales High / Medium / Low; Exposure = Impact × Likelihood. Never renumber.

## Register

| Risk ID | Description | Impact | Likelihood | Exposure | Mitigation Strategy | Owner | Status | Residual Risk |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| R-001 | The nightly workflow has failed every night since 2026-10-14, so its findings go unseen: on 2026-10-28 a css fuzz timeout, a stack overflow replaying the fuzz corpora under coverage, and the browser oracle disagreeing with current Chrome. | High | High | HxH=High | Triage each failing job; file bug reports for real defects (testing guide); re-check `browser.json` against Chrome and regenerate if the browser is right; keep the nightly green so a new failure is a signal. | Owner | Open | Low once green |
| R-002 | Two upstream fluent-syntax defects (panic, stack overflow from any FTL asset) are fixed only in a fork applied by `[patch.crates-io]`, which works only in the top-level project: apps without the same entry (vtuber today) inherit them. | High | Medium | HxM=High | README asks apps to add the patch; get the upstream PRs merged (U1, U2), then drop the fork. | Owner | Mitigating | Low after upstream fix |
| R-003 | CSS copied from the web misbehaves silently where the subset deviates, notably `flex-shrink` defaulting to 0 (bug_0028); skipped values are logged only at `debug`. | Medium | High | MxH=High | Decide `flex-shrink` as a deliberate change (re-derive vectors against browser defaults); keep limits documented. | Owner | Open | Medium |
| R-004 | Releases lag `main`: a large `[Unreleased]` changelog since 0.7.0 (2026-10-15); prototype_19 still on 0.4.0, so its fixes and features don't reach the studio's game. | Medium | Medium | MxM=Medium | Release from `[Unreleased]`; bump prototype_19 (a migration across 0.5–0.7). | Owner | Open | Low |
| R-005 | Each Bevy release changes UI internals the library leans on (0.20's weakly ordered `UiSystems`, picking, text): migrations are large and can break subtly. | Medium | High | MxH=High | Schedule-order shuffling in every test run; oracles and golden images catch regressions; pin explicit ordering. | Owner | Accepted | Medium |
| R-006 | Golden images depend on the CI runner's Mesa (lavapipe is canonical; `ubuntu-latest` moved to Ubuntu 26 on 2026-10-19): a renderer change fails the job without a code change. | Low | Medium | LxM=Low | Re-record with the `Update golden references (lavapipe)` job; `runs-on: ubuntu-24.04` as fallback. | Owner | Accepted | Low |
| R-007 | The repository lives on the personal `nchashch` account while the studio's other repositories are on the `noeticbasho` org. | Medium | Low | MxL=Low | Transfer to the org when convenient (crates.io `repository` field follows). | Owner | Open | Low |
| R-008 | `AGENTS.md` is ~130 KB; every agent session that loads it spends a large share of its context before work starts, and keeping one file that long current invites drift. | Medium | Medium | MxM=Medium | Keep reference material (Testing history, Gotchas) in linked files and the file to current state. | Owner | Open | Low |

## Sources

- @AGENTS.md
- @CHANGELOG.md
- @docs/agents/adr/0028-modern-authoring-gaps-roadmap.md
- @docs/agents/bugs/bug_0028.md
- @docs/agents/bugs/UPSTREAM.md
- @docs/agents/skills/testing.md
