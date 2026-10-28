# 37. Anchored overlays with Bevy's Popover

| Field | Content |
|---|---|
| `ADR` | `0037` |
| `Title` | Anchored overlays with Bevy's Popover |
| `Date` | 2026-10-29 02:00 +0400 |
| `Author` | Claude Opus 5.5 (Anthropic), via Claude Code |
| `Commit` | `4b13c76 Decode character references with htmlize (ADR 0036); reject html5ever (ADR 0035)` |
| `Status` | Rejected (2026-10-29, after evaluation: a Popover must be a child of its anchor, and a child node collapses a text block's layout) |
| `Related` | ADR 0035, 0036 (the same cleanup); `src/anchor.rs`, `src/tooltips.rs` |

## Context

bevy_markup positions overlays itself. `HtmlAnchor` (on any UI root,
typically an `HtmlUi`) keeps the overlay beside another entity: each
frame `place_anchored` reads the element's laid-out rect (the previous
frame's layout), sets the overlay's insets for `Right`/`Left`/`Above`/
`Below`, clamps it inside the element camera's viewport, gives it the
element's `UiTargetCamera`, and despawns it with the element. Tooltips
(`data-tooltip`) spawn an `HtmlUi` with an `HtmlAnchor` to the nearest
ancestor that has a box (a link span anchors to its text block).
`HtmlWorldAnchor` does the same for a point in the 3D world.

Bevy 0.20's `bevy_ui_widgets::Popover` does anchored placement upstream:
a `Popover` node is positioned relative to **its parent**, in
`PostUpdate` after layout (the same frame), choosing among a list of
`PopoverPlacement`s (side, align, gap) the one least clipped by the
window, and setting `position_type: Absolute` and its `UiTransform`.

The cleanup asked whether `HtmlAnchor` and the tooltips could use it.

## Decision (rejected)

Make `HtmlAnchor` a thin wrapper: reparent the overlay under its element,
insert `Popover` (the placement as its side with `Start` alignment, the
mirror side as a fallback) plus `OverrideClip` (so an
`overflow: hidden`/`scroll` ancestor doesn't clip it) and a
`GlobalZIndex`; delete `place_anchored`. Tooltips spawn as children of
their anchor.

## Evaluation

What would have worked:

- **Reconciliation** keeps app-nested `HtmlUi` roots under an element
  (`update_children` keeps them after the pipeline's children), so a
  child overlay survives rebuilds, and dies with a replaced element as it
  does today.
- **Clipping**: `OverrideClip` drops the inherited clip rect
  (`bevy_ui` `update_clipping`).
- **Gains**: same-frame placement (no one-frame trail), and flipping to
  the less clipped side instead of clamping over the element.

What sinks it:

- **A child node collapses a text block.** A block of text (`<p>`, a
  heading, a list item) is one entity carrying `Text`, laid out as a
  measured leaf. A throwaway layout test put an absolutely positioned
  50×20 node under such a block: the block went from 320×39 to **320×0**
  (taffy stops measuring a node that has a layout child). Tooltip anchors
  are text blocks in the common case — every link span anchors to its
  paragraph — so the overlay would erase the text it points at.
- Working around it means either anchoring to a non-text ancestor (the
  wrong rect, so not Popover's placement any more) or giving every text
  block a wrapper node (a layout-model change across the build, the
  windowed build's height estimates and the oracle's model, for a
  ~150-line placement function).
- `HtmlWorldAnchor` has no Bevy counterpart and stays either way, so the
  module wouldn't go.

## Revisit when

- Bevy's `Popover` can target an entity other than its parent (an
  explicit anchor, like CSS anchor positioning), or
- bevy_markup's text blocks gain a wrapper node for another reason.

## Worth taking from Popover without adopting it

Not done here; candidates if overlays need them:

- **Fallback placements**: try the mirror side when the preferred one is
  clipped, instead of only clamping (a tooltip at the right edge flips
  left rather than covering its element).
- **Same-frame placement**: run after `ui_layout_system` and correct the
  overlay's transform as Popover does, removing the one-frame trail.
