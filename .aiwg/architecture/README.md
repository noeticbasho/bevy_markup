# Architecture

Kept outside `.aiwg/` and not copied here:

- **Current state:** @AGENTS.md ("Public API", "Layout", "Pipeline", "Gotchas").
- **Decisions:** @docs/agents/adr/README.md (ADRs 0001–0046; roadmap: 0028; selector matching: 0034, its trial in @docs/agents/performance/adr-0034-trial.md; HTML parsing and decoding: 0035 rejected, 0036 accepted, trials in @docs/agents/performance/adr-0035-trial.md and @docs/agents/performance/adr-0036-trial.md; overlays: 0037 rejected; SVG: 0038 accepted; CSS nesting pre-scan on cssparser's tokens: 0039; SVG conversion-work bound: 0040; `<canvas>`: 0041, its source on a child node: 0043; threat model and the template safety check: 0044; FTL checked at load instead of the fluent-syntax fork: 0045; the template check's loop nesting and depth budget: 0046; SVG object boxes by a first usvg pass: 0042).
- **User guide:** the crate docs (`cargo doc --open`) and @README.md.

Record new decisions as ADRs in `docs/agents/adr/` (format: @docs/agents/skills/adr.md).
