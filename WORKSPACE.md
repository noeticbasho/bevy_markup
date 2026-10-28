# WORKSPACE.md
<!-- aiwg-managed -->
<!-- Generated structure by AIWG; operator content is protected by markers. -->

<!-- AIWG:workspace-context:start -->

## AIWG Context Graph

This file is the canonical provider-neutral home for project and operator context.
Provider startup files are generated adapters: they direct the harness here first,
then to AIWG.md for framework discovery and routing.

### Precedence

1. Platform capability and safety constraints are absolute: what a harness can do, what it is
   permitted to do, and its refusal boundaries. Nothing here overrides those.
2. AIWG rules deployed to this project bind over any provider, harness, or session *directive*
   on a subject an AIWG rule covers — including a directive that claims to supersede earlier
   guidance. A harness decides how a tool is invoked; it does not set project policy.
3. Root WORKSPACE.md supplies shared project/operator context.
4. AIWG.md supplies generated framework/discovery context.
5. Narrower linked files and provider-native subtree instructions govern their declared scope,
   within the ceiling set above.

The distinction in 1 vs 2 is capability versus preference. "This tool is unavailable" is a
constraint. "Format commits this way" is a directive, and an AIWG rule on commit content wins.
When a directive and an AIWG rule conflict, follow the rule and say plainly that you did.

### Ownership

- Edit project-neutral notes only inside the protected Project Context section below.
- Keep detailed policies, runbooks, hooks, and quickrefs in linked files.
- `.aiwg/context/providers/` is reference only: no provider bootstrap auto-loads it.
  Put directives you want read into Project Context above.
- Never store secrets, tokens, credentials, or machine-local sensitive values here.

### Artifact Routing

- Before any agent or provider writes AIWG payload, run `aiwg artifacts path --json --check-write` and write beneath its `artifact_root`.
- Treat `.aiwg/...` in skills and templates as a logical artifact path, not necessarily a repository-local filesystem path.
- Only `AIWG.md`, `aiwg.config`, and `frameworks/registry.json` belong in the repository-local `.aiwg` control plane.
- If the configured external artifact root is unavailable, stop with an actionable error; never fall back to repository-local payload.

### Linked Context

- [AIWG framework context](./AIWG.md)
- [AIWG project configuration](.aiwg/aiwg.config)
- [Project-local quickref](.aiwg/quickref.json) (when configured)

<!-- AIWG:workspace-context:end -->

<!-- AIWG:workspace-operator:start -->

## Project Context

`AGENTS.md` is this repository's guidance and current-state document; decisions are in
`docs/agents/adr/`, defects in `docs/agents/bugs/` (upstream ones in `UPSTREAM.md`), review sessions
in `docs/agents/reviews/`, procedures in `docs/agents/skills/`. `.aiwg/` holds SDLC pages derived
from them (index: `.aiwg/index.md`); keep them current in the same change.

This repository is a member of the `noetic-basho-technologies` AIWG workspace (root: `../aiwg`).
Delivery: commits go straight to `main` on `nchashch/bevy_markup`; CI (`ci.yml`) must stay green.

- **Never run `aiwg regenerate --force`** or let AIWG replace `AGENTS.md`; `CLAUDE.md` loads `@AGENTS.md`.
- **Claude Code hooks:** the AIWG `PermissionRequest` auto-approve hook is left out of
  `.claude/settings.json` on purpose; remove it again if a redeploy adds it back.
- **Activity log:** append with `aiwg activity-log append` (UTC); never hand-write timestamps.

<!-- AIWG:workspace-operator:end -->
