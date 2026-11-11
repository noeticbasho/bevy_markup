# 28. Modern authoring gaps: what to add next (roadmap)

| Field | Content |
|---|---|
| ADR | 0028 |
| Title | Modern authoring gaps: what to add next (roadmap) |
| Date | 2026-10-15 06:01 +0400 |
| Author | GLM 5.3 Flash (Z.ai), via omp |
| Commit | `e2081e8 Release 0.7.0` (uncommitted at writing) |
| Status | Accepted (the roadmap was adopted and worked in order; status corrected 2026-10-16). Shipped: the text surface (text-transform, white-space/ellipsis), background-image (+repeat/clip; size unsupported), @font-face parse-and-skip, :nth-child/:not/:focus-within/:empty, sibling combinators, tables on the flex model, data-* attribute selectors, <canvas> showing app render targets (ADR 0041, 2026-11-09), SVG — `.svg` images, inline <svg>, SVG text (ADR 0038, accepted 2026-11-11). Deferred as documented limits (capability-checked): position: sticky, <textarea> (bevy's TextInput is single-line), built-in checkbox/radio widgets. Still open (tier 4): @media, box-/text-shadow, gradients as images, @keyframes against the rebuild model |
| Related | ADR 0011 (inline boxes), ADR 0027 (text properties, lists, overflow, entities — the wave this builds on); AGENTS.md Limits; review_0004 |

## Context

ADR 0027 closes the six silent text/entity gaps. Beyond them, the subset
diverges from what modern frontend and UI developers reach for daily. This
record inventories the gaps with evidence, ranks them by workflow pain,
and commits to an order — so later work can cite one document instead of
re-deriving the list.

Verified against the code at `e2081e8`:

- **Gradients as images are refused:** a `linear-gradient(…)`
  `border-image-source` is deliberately treated as *no image*
  (`cascade.rs:5469` test: "gradient source treated as no image").
  Element backgrounds are solid `background-color` only; there is no
  `background-image` at all.
- **`box-shadow`/`text-shadow`** are not parsed anywhere in
  `style.rs`/`cascade.rs` — no arm, so they are silently ignored.
- **Truncation:** no `text-overflow`, no `white-space` mapping on normal
  blocks (only `pre` builds `TextLayout::no_wrap()`, `build.rs:2141`
  area); long labels wrap or overflow.
- **`text-transform`** has no arm.
- **Pseudo-elements** (`::before`/`::after`) don't exist.
- **`@font-face`** is not parsed: font resolution goes through the
  `FontFamilies`/`FontFaces` resources (`src/fonts.rs`), not CSS.
- **`<table>`** is not in `CONTAINERS` (`build.rs`) and gets no handling —
  an unknown tag walked through, rendering as anonymous text blocks.
- **Form controls:** `checkbox`, `radio`, `range`, `color`, `file`, … are
  known-but-unbuilt — they build no field (`build.rs:3884`); only text
  inputs (ADR 0016/0023) and buttons exist. No `:checked`, no
  `textarea`.
- **Selectors:** a selector holding a sibling combinator (`+`/`~`), an
  attribute selector or an unknown pseudo-class **skips the whole rule**
  (`cascade.rs:4020` doc) — so `:nth-child`, `:not`, `:focus-within`,
  `:empty` are all unreachable.
- **`position: sticky`** is explicitly skipped (`cascade.rs:2633`,
  `style.rs:89`).
- **Animation:** `transition` (shorthand) animates color changes only;
  there is no `@keyframes`, no `transform` property.
- **`@media`** blocks are consumed as unknown (`cascade.rs:4516` shows the
  nested-at-rule test); no responsive mechanism in CSS.
- **CSS nesting** (native nested rules) is unsupported — the load-time
  pre-scan *bounds* rule nesting (bug_0106) but the parser drops nested
  rules.
- **SVG:** no `<svg>` element handling and no SVG asset path.
- **`object-fit`** and background sizing (`cover`/`contain`): `<img>`
  sizes by CSS `width`/`height` (stretch/preserve-ratio), backgrounds by
  the 9-slice model only.

## Decision

Adopt this order. Each tier is independently shippable; the ranking is
author pain per unit of Bevy risk, lowest risk first. Every item gets its
own vector + oracle check when it lands (ADR 0027's test discipline), and
any item that changes a default or a contract gets its own ADR (this one
only fixes the *order* and the scope lines).

**Tier 1 — text-surface basics (no new Bevy surface):**

1. Text truncation: `white-space` (`normal`/`nowrap`/`pre`/`pre-wrap`) and
   `text-overflow: ellipsis` [INFERENCE on Bevy: needs ellipsis support in
   `TextLayout`/parley — capability-check first; if Bevy can't, ship
   `white-space` alone and keep `ellipsis` refused loudly].
2. `text-transform` (`uppercase`/`lowercase`/`capitalize`/`none`,
   inherited) — applied at run construction.
3. `background-image: url(…)` on blocks/containers/root — the same
   `ImageNode` path `border-image` uses (loader dependency, sizing
   `cover`/`contain`/`stretch`, `VisualBox` per `background-clip`'s two
   values; no gradients yet — see Tier 4).
4. `@font-face` documentation-level fix: keep the resource model, but
   *parse* `@font-face` far enough to (a) skip it with a `debug!` note
   naming the resource alternative and (b) stop a copied web stylesheet
   from failing the whole sheet load.

**Tier 2 — selectors and tables (parser + walk work, no Bevy surface):**

5. `:nth-child`/`:nth-of-type` (odd/even/an+b), `:not()` (compound
   argument), `:focus-within`, `:empty` — per-element match against the
   parent's child list (the walk knows it); the match cache keys must fold
   the position in.
6. Sibling combinators `+`/`~` — needs the walk to thread the previous
   sibling's element+pseudo; same cache discipline as ancestors.
7. `<table>`: either map onto the grid model under the hood
   (`table-row`/`cell` → implicit tracks) or refuse the load with a
   "use CSS grid" error — anything but today's silent text-blob output.
   Decide by measurement when picked up.

**Tier 3 — new Bevy surfaces (each its own capability check):**

8. `position: sticky` — likely needs layout-time positioning (the
   `FixedNode` pattern, scroll-aware).
9. `checkbox`/`radio` (widget-backed, `:checked` mapping to
   `PseudoState`), `textarea` (multi-line `EditableText`).
10. Attribute selectors (`[attr]`, `[attr="v"]`, `[data-*]` prefix forms).

**Tier 4 — engine-adjacent, decide only after profiling:**

11. `@keyframes` + `transition` on layout/transform properties — the
    biggest lift, and the most likely to fight the rebuild model
    (ADR 0021's skip-the-diff contract): an animation wants per-frame
    writes the model exists to avoid. Prototype before committing.
12. SVG: decide the shape first — pre-rasterized assets via `<img>` vs a
    `bevy_svg`-class integration. Not a bevy_markup decision alone.
13. `@media`: the Bevy analogue is viewport size + scale, which the
    cascade can read; defer until Tier 1–3 landed (responsive needs
    truncation and sticky to be useful).
14. `box-shadow`/`text-shadow` and gradients: [INFERENCE] on Bevy 0.20
    surface (check `BoxShadow` and whether a generated-texture approach
    is acceptable); gradients-as-data fit the crate (generate the texture
    in the loader) but are pure scope — last.

Accessibility mapping (`aria-*`/role → `AccessibilityNode`) is tracked
here as a standing reminder, not ordered: it spans every element kind and
deserves its own ADR when picked up.

## Alternatives considered

- **"Full CSS parity" as the goal**: rejected — bevy_markup's contract is
  the browser-feel subset a game UI needs (README); parity work (grid
  subgradients, container queries, `:has()`) buys nothing inside Bevy's
  layout engine and would bloat the cascade's hot paths.
- **Ad-hoc app-side workarounds** (Tera context for responsive, manual
  truncation systems): the status quo, and exactly the friction this
  roadmap removes; rejected as a *destination*, kept as the interim.
- **Implementing Tier 4 first** (animations before truncation): rejected —
  visible, cheap wins pay for the risky work; animation design without
  the rebuild-model profiling would likely be reverted.

## Consequences

- **Cost of the ADR itself:** none beyond the record; it adds no code and
  changes no contract. Each tier's items carry their own cost lines when
  their ADRs land.
- **Process:** this record is the index of *what is deliberately missing*
  — AGENTS.md's Limits section should link here instead of re-listing
  aspirational orders, so the two can't drift. Update this ADR only per
  §7 (status flips, not body edits): an item that ships moves out via its
  own ADR, and a re-prioritization is a new record.
- **Risk surface:** Tier 1/2 items stay inside the existing pipeline
  (parse → cascade → items → nodes) and the oracles can check them;
  Tier 3+ items touch Bevy surfaces with unknowns, hence the
  capability-check-first rule written into the items themselves.
