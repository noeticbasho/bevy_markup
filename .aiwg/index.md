# SDLC Index — bevy_markup

_Generated: 2026-10-28 — 9 pages · consumer `sdlc-complete` · workspace `noetic-basho-technologies` (root: `../aiwg`)_

Derived pages cite their sources with `@path` mentions; provenance records are in `.aiwg/research/provenance/records/`. Authoritative: `AGENTS.md`, `docs/agents/adr/`, `docs/agents/bugs/`, `docs/agents/reviews/`, `docs/agents/skills/`.

## Intake (3)
- [Project intake](intake/project-intake.md) — HTML/Tera + CSS + Fluent → Bevy UI library; 0.7.0 on crates.io; UI layer of prototype_19 and vtuber
- [Solution profile](intake/solution-profile.md) — published 0.x library; very strong verification, nightly red since 2026-10-14
- [Option matrix](intake/option-matrix.md) — priorities: web fidelity, robustness, small surface, update performance

## Requirements (2)
- [Use-case briefs](requirements/use-cases.md) — UC-001–013 built (templates, CSS, Fluent, Markdown, interaction, focus, updates, overlays, custom elements, text input, windowing, hot reload, agent QA); UC-101–104 planned (ADR 0028 tier 4)
- [Supplementary specification](requirements/supplementary-spec.md) — reliability, conformance (oracles), performance, supportability, security requirements with their checks

## Risks (1)
- [Risk list](risks/risk-list.md) — R-001–008; highest: nightly red and untriaged, fluent-syntax patch not reaching apps, silent CSS deviations, Bevy churn

## Testing (2)
- [Test strategy](testing/test-strategy.md) — unit, vectors, oracles, property/model tests, golden images, five fuzzers, mutants, coverage, benchmarks
- [Defect register](testing/defect-register.md) — bug_0001–0178: 176 fixed, none open; 13 upstream; 3 untriaged nightly findings

## Architecture (1)
- [Architecture](architecture/README.md) — pointer to `AGENTS.md` and ADRs 0001–0033
