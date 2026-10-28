# Architecture Decision Records

Short, dated writeups of significant decisions in bevy_markup: the
alternatives considered, why one was picked, and what it cost. They are
separate from [`AGENTS.md`](../../../AGENTS.md), which describes the code's
*current* state and is rewritten as it changes. An ADR stays as written; a
later one supersedes it. Format and rules:
[`docs/agents/skills/adr.md`](../skills/adr.md).

0001–0012 were written retrospectively on 2026-10-06 from the git history
(`59f8f32..d2c9b63`), the `AGENTS.md` text at each commit, the bug ledger,
and the omp session that designed 0007–0012 together with the project owner.
Each record says who made the decision and marks reasoning nobody recorded
as `[INFERENCE]`.

## Index

| # | Title | Status |
|---|-------|--------|
| [0001](./0001-html-tera-fluent-css-onto-bevy-ui.md) | UI as HTML templates (Tera) + Fluent + CSS, built into native Bevy UI entities | Accepted |
| [0002](./0002-library-shape-and-public-api.md) | A minimal-dependency Bevy library with a component/resource API | Accepted |
| [0003](./0003-nine-slice-frames-as-css-border-image.md) | 9-slice frames as CSS `border-image`, not a custom asset format | Accepted |
| [0004](./0004-verification-strategy.md) | Verify against reference implementations, then attack with every kind of generated input | Accepted |
| [0005](./0005-update-decisions-as-a-pure-state-machine.md) | When to rebuild or restyle: a pure state machine, with restyles in place | Accepted (content-rebuild part superseded by [0009](./0009-in-place-update-model.md)) |
| [0006](./0006-declarative-interaction-signals-and-pseudo-classes.md) | Declarative interaction: `data-on-*` signals as messages, `:hover`/`:active` as restyles | Accepted (extended by [0011](./0011-signal-sources-dataset-and-routing.md)) |
| [0007](./0007-css-layout-and-the-root-rule.md) | Layout through CSS — flex, grid, positioning — and the template styling its own root | Accepted |
| [0008](./0008-focus-and-navigation-in-the-library.md) | Browser-style focus and directional navigation in the library; input stays the app's | Accepted |
| [0009](./0009-in-place-update-model.md) | Update in place: skip identical renders, reconcile by key, write only what changed | Accepted |
| [0010](./0010-overlays-anchors-and-tooltips.md) | Overlays: anchored to elements and world points, with tooltips built in | Accepted |
| [0011](./0011-signal-sources-dataset-and-routing.md) | Signals: what produced them, browser button semantics, per-feature `data-*`, routing by name | Accepted |
| [0012](./0012-template-composition-and-examples.md) | Template composition: one Tera set per template, Tera 2 components as the widget library | Accepted |
| [0013](./0013-own-the-fluent-bundle-asset.md) | Own the Fluent bundle asset instead of bevy_fluent | Accepted |
| [0014](./0014-markdown-via-pulldown-cmark.md) | Markdown as a source syntax via pulldown-cmark | Accepted |
| [0015](./0015-font-fallback-lists.md) | Font fallback lists via Bevy 0.20's `FontSource` | Accepted |
| [0016](./0016-text-input.md) | Text input as `<input type="text">` via Bevy 0.20's `TextInput` | Accepted |
| [0017](./0017-keyed-sub-document-updates.md) | Keyed sub-document updates: skip unchanged sub-trees when content changes | Rejected on measurement |
| [0018](./0018-benchmarks-with-criterion.md) | Benchmarking with Criterion | Accepted |
| [0019](./0019-benchmarks-with-divan.md) | Benchmarking with Divan (the second harness; allocation counts) | Accepted |
| [0020](./0020-convert-markdown-at-template-load.md) | Convert Markdown at template load, not per render | Accepted |
| [0021](./0021-per-element-dirty-tracking.md) | Per-element dirty tracking: dependency maps at load, slot-local updates per frame | Accepted (fully implemented); amended by 0033 |
| [0022](./0022-text-inputs-without-maxlength-or-placeholder.md) | Text inputs carry no `maxlength` or `placeholder` | Superseded in part (its `maxlength` half) by [0023](./0023-maxlength-maps-to-max-characters.md) |
| [0023](./0023-maxlength-maps-to-max-characters.md) | `maxlength` maps to the widget's `max_characters` | Accepted |
| [0024](./0024-tera-block-indentation-is-not-markdown.md) | Indentation inside Tera blocks is not Markdown | Accepted |
| [0025](./0025-html-container-indentation-is-not-markdown.md) | Indentation inside raw HTML containers is not Markdown | Accepted |
| [0026](./0026-disable-indented-code-blocks.md) | Disable indented code blocks in Markdown (fenced code only) | Proposed |
| [0027](./0027-text-properties-lists-overflow-entities.md) | Add the missing text properties, list numbering, scroll-if-needed and full entity decoding | Accepted (fully implemented) |
| [0028](./0028-modern-authoring-gaps-roadmap.md) | Modern authoring gaps: what to add next (roadmap) | Accepted (roadmap adopted; text surface, background-image, font-face skip, selectors, siblings, tables, attributes shipped — sticky/checkbox/textarea deferred as documented limits; SVG, @media, shadows/gradients and keyframes still open) |
| [0029](./0029-restyle-reuse.md) | Restyle reuse: dependency-tracked re-cascade instead of full restyles | Accepted |
| [0030](./0030-compounds-match-whole-in-every-position.md) | Compounds match whole in every position | Accepted |
| [0031](./0031-windowed-build-spec-level-culling.md) | Windowed build: spec-level viewport culling with height-preserving spacers | Accepted; amended by 0032 |
| [0032](./0032-windowed-build-revised.md) | Windowed build, revised: one unit system, nested culling, scroll reuse and slack | Accepted; amended by 0033 |
| [0033](./0033-sibling-alignment-and-height-estimates.md) | Sibling alignment and height estimates for content changes | Accepted |
| [0034](./0034-match-selectors-with-parcel-selectors.md) | Match selectors with lightningcss's selector engine (parcel_selectors) | Accepted |
| [0035](./0035-replace-tl-with-html5ever.md) | Replace tl with html5ever | Rejected |
| [0036](./0036-decode-character-references-with-htmlize.md) | Decode character references with htmlize | Accepted |
| [0037](./0037-anchored-overlays-with-bevy-popover.md) | Anchored overlays with Bevy's Popover | Rejected |

Add new records to this index in the same commit that adds the file.
