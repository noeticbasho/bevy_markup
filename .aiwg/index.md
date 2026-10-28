# SDLC Index — bevy_markup

_Generated: 2026-10-28, refreshed 2026-10-29 (source `7792cea`) — 9 pages · consumer `sdlc-complete` · workspace `noetic-basho-technologies` (root: `../aiwg`)_

Derived pages cite their sources with `@path` mentions; provenance records are in `.aiwg/research/provenance/records/`. Authoritative: `AGENTS.md`, `docs/agents/adr/`, `docs/agents/bugs/`, `docs/agents/reviews/`, `docs/agents/skills/`.

## Intake (3)
- [Project intake](intake/project-intake.md) — HTML/Tera + CSS + Fluent → Bevy UI library; 0.7.0 on crates.io; UI layer of prototype_19 and vtuber
- [Solution profile](intake/solution-profile.md) — published 0.x library; very strong verification, nightly green again since 2026-10-29
- [Option matrix](intake/option-matrix.md) — priorities: web fidelity, robustness, small surface, update performance; tl kept over html5ever, overlays not Popover children

## Requirements (2)
- [Use-case briefs](requirements/use-cases.md) — UC-001–013 built (templates, CSS, Fluent, Markdown, interaction, focus and Tab, updates, overlays, custom elements, text input, windowing, hot reload, agent QA); UC-101–104 planned (ADR 0028 tier 4)
- [Supplementary specification](requirements/supplementary-spec.md) — reliability, conformance (oracles), performance, supportability, security requirements with their checks (incl. ADR 0034's selector engine and allowlist, ADR 0036's decoding, linear sibling lists)

## Risks (1)
- [Risk list](risks/risk-list.md) — R-001–011; highest: nightly (fixed, green since 2026-10-29), fluent-syntax patch not reaching apps, silent CSS deviations, Bevy churn; releases lag with breaking changes

## Testing (2)
- [Test strategy](testing/test-strategy.md) — unit, vectors, oracles, property/model tests, golden images, five fuzzers, mutants, coverage, benchmarks
- [Defect register](testing/defect-register.md) — bug_0001–0181: 179 fixed, none open; 15 upstream; nightly failures and the CI scaling failure (bug_0181) fixed

## Architecture (1)
- [Architecture](architecture/README.md) — pointer to `AGENTS.md` and ADRs 0001–0037
