# 43. A canvas shows its source on a child node

| Field | Content |
|---|---|
| `ADR` | `0043` |
| `Title` | A canvas shows its source on a child node |
| `Date` | 2026-11-11 16:46 +0400 |
| `Author` | Claude Opus 5.5 (Anthropic), via Claude Code |
| `Commit` | `3ce6efe File review_0007's findings as open bug reports bug_0204-0221` + uncommitted: the review_0007 fixes |
| `Status` | Accepted |
| `Related` | ADR 0041 (amends its §3 and §4); bug_0215, bug_0216, bug_0217, bug_0218; review_0007 K12–K15; UPSTREAM U38 |

## Context

ADR 0041 §3 put a canvas's `ViewportNode` or `ImageNode` on the
`<canvas>` element itself, on the premise that "CSS `border-image`
doesn't apply to it". It does: `border-image` and `background-image` are
the element's `ImageNode` too (`CssFrame`), so the two replaced each
other. Binding replaced the frame, and the next in-place update
(`update_spec` re-inserts a `CssFrame`'s `ImageNode`, which has no
`PartialEq`) replaced the picture while `HtmlCanvas::showing` still said
`Image` (review_0007 K12, bug_0215). A camera canvas kept its
`ViewportNode`, but Bevy drew the frame's center over the same node.

Three more defects lived in §3–§4's owner choice: a hidden canvas still
owned its camera, and Bevy resized the image to 1 × 1 for its 0 × 0 box
(bug_0216); an oversized canvas first in document order owned the camera
and showed nothing (bug_0217); and a camera whose target image wasn't in
`Assets<Image>` panicked in Bevy's resize (bug_0218, U38).

## Decision

1. **The picture is a child.** `sync_canvases` shows the source on a
   `CanvasPicture` child (in the prelude) that fills the canvas's padding
   box (absolute, 100% × 100%): a `ViewportNode` or a stretched
   `ImageNode` goes there, never on the element. The element keeps
   whatever CSS gives it: frame, background, border, radius.
2. **Updates keep the child** like an app-nested `HtmlUi`: the build's
   `Tree` query reads `(Has<HtmlUi>, Has<CanvasPicture>)` where it read
   `Has<HtmlUi>`, so `update_children` leaves it out of the pairing and
   keeps it, and `refresh_cached_handles` skips it.
3. **`bind` owns the child's life**: spawned on demand, despawned when
   the canvas shows nothing; a viewport goes on it only once it's laid
   out at a non-zero size (a fresh child has none in the frame it's
   spawned, and Bevy would resize the image to 1 × 1).
4. **Owners** are chosen among canvases that are visible and within
   `MAX_CANVAS_SIDE`; with none, the camera has no viewport and nothing
   resizes its image. A camera is bound only while its target image is
   in `Assets<Image>`.

## Alternatives considered

- **Refuse CSS images on a canvas** (review_0007's second direction):
  document it and drop a `border-image`/`background-image` declaration on
  `<canvas>` with a warning. One check, no new entity, but it takes away
  the obvious way to frame a portrait, which the `canvas` example's
  readers would write first.
- **Draw the frame on the child instead** (the source on the element):
  the frame is CSS-owned and restyled by `update_spec`, which only
  writes to the element's own entity; moving CSS output off the element
  would touch every frame path, for one tag.

## Consequences

- A framed canvas works through bindings and updates
  (`tests/canvas.rs` `a_framed_canvas_keeps_its_frame_and_its_picture`).
- One more entity per bound canvas, and the first bind of a camera
  canvas waits one frame for the child's layout.
- Apps that read `ViewportNode`/`ImageNode` off the canvas element must
  read the `CanvasPicture` child instead. The canvas feature is
  unreleased, so no released contract changes.
- Pointer input: picking hits the child; events bubble to the element
  (its `data-on-*` hooks), `:hover` walks ancestors, and Bevy's
  `viewport_picking` forwards from the child's `ViewportNode`
  (`pointer_input_reaches_the_cameras_scene` still passes).
