# Option Matrix (Project Context and Priorities)

| Field | Value |
|---|---|
| Derived from | `AGENTS.md`, ADRs 0001, 0002, 0004, 0028, 0035–0037 |
| Source state | `7792cea`, 2026-10-29 |

## Project Reality

- A library, not an app: its users are Bevy developers and template authors; its end users are the players of those games.
- One developer with AI agents; verification is the method that lets agents change it safely (ADR 0004).
- Consumers inside the studio: prototype_19, vtuber; published on crates.io.

## Priorities (weights sum to 10)

| Priority | Weight | Evidence |
|---|---|---|
| Fidelity to web semantics (what an author expects from HTML/CSS/Fluent) | 3 | oracles against Chromium and `@fluent/dom`; deviations are documented limits |
| Robustness against any input | 3 | untrusted-input stance; fuzzers; "reachable from user data = high" |
| Small surface and dependency set | 2 | ADR 0002; minimal Bevy features; app decisions stay in apps; dependencies stay in the HTML/CSS/Fluent domain, where a maintained crate may replace code the crate maintains (ADR 0036) |
| Update performance | 2 | ADR 0009, 0021, 0031; benchmark suites |

## Trade-offs Already Made

- Bevy's capabilities bound the subset: no sticky positioning, no inline boxes with content, no multi-line text input (ADR 0028).
- `flex-shrink: 0` default kept for compatibility with existing layouts, against CSS (open question).
- Application input bindings stay in apps; the library offers `HtmlFocus` (directional and Tab) instead (ADR 0008).
- `tl`'s tag soup kept over html5ever: a spec parser costs 6–9× on the per-change parse, 10–20 % of a changing frame (ADR 0035).
- Overlays stay separate roots placed by the crate, not Bevy `Popover` children: a child node collapses a text block's layout (ADR 0037).

## Sources

- @AGENTS.md
- @docs/agents/adr/0001-html-tera-fluent-css-onto-bevy-ui.md
- @docs/agents/adr/0002-library-shape-and-public-api.md
- @docs/agents/adr/0004-verification-strategy.md
- @docs/agents/adr/0008-focus-and-navigation-in-the-library.md
- @docs/agents/adr/0028-modern-authoring-gaps-roadmap.md
- @docs/agents/adr/0035-replace-tl-with-html5ever.md
- @docs/agents/adr/0036-decode-character-references-with-htmlize.md
- @docs/agents/adr/0037-anchored-overlays-with-bevy-popover.md
