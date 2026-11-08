# 41. `<canvas>` shows a render target the app registers

| Field | Content |
|---|---|
| `ADR` | `0041` |
| `Title` | `<canvas>` shows a render target the app registers |
| `Date` | 2026-11-09 01:53 +0400 |
| `Author` | Claude Opus 5.5 (Anthropic), via Claude Code |
| `Commit` | `0daf648 Fix bugs 188-191: bound usvg's conversion work before it runs (ADR 0040)` + uncommitted: the U24/U26–U28 SVG guards (unrelated) |
| `Status` | Proposed |
| `Related` | ADR 0028 (authoring gaps roadmap); ADR 0038 (SVG: `<img>`-like replaced elements, `MAX_SVG_SIDE`); ADR 0031/0032 (windowed build); `anchor.rs` `HtmlWorldAnchorView` (the visibility report this copies) |

## Context

Games put live renders inside their UI: a minimap, a character preview in
an inventory, a portrait in a dialogue box, a rotating item model in a shop.
In Bevy, a camera renders into an `Image` (`RenderTarget::Image`) and the UI
shows that image. bevy_markup has no way to place such an image from a
template:

- `<canvas>` isn't handled. It's not in `CONTAINERS` (`build.rs`), so it's
  walked through like any unknown inline tag (`is_walked_through`). It gets
  no entity, so an `is="…"` definition on it never runs and
  `HtmlElements` can't find it.
- `<img src>` takes asset paths only (`ImageRun`, `image_of`). A render
  target is a runtime `Handle<Image>` with no path.
- The workaround is a `<div is="minimap">` whose definition inserts Bevy's
  `ViewportNode` (`bevy_ui::widget::ViewportNode`). It works, but the
  template can't size it like an image (no intrinsic size, so the box is
  0×0 unless CSS sizes it). Every app also rewrites the same binding code,
  and nothing bounds the texture it allocates (below).

Bevy 0.20 already has the pieces, all in `bevy_ui` (the crate's `ui_api`
feature; no new Bevy feature):

- `ViewportNode { camera: Option<Entity> }` (`bevy_ui` `widget/viewport.rs`)
  shows a camera's `RenderTarget::Image` in a node. Rendering it is
  `bevy_ui_render`'s `extract_viewport_nodes`, which is the app's choice, as
  all rendering is.
- `update_viewport_render_target_size` resizes the target image to the
  node's physical size whenever `ComputedNode` changes, so HiDPI and layout
  changes get a sharp image with no app code.
- `viewport_picking` (behind `bevy_picking`, which bevy_markup enables)
  forwards pointer input over the node to a custom `PointerId`, positioned in
  the camera's viewport. The app's own picking backend (mesh picking, say)
  then hits the 3D scene through the UI.
- `Image::new_target_texture(w, h, format, view_format)` (`bevy_image`)
  builds an image with the usages a render target needs.

One hazard in that path: the resize is unbounded. `Image::resize` grows
`data` to `w × h × pixel size` when the image has CPU data, and
`new_target_texture` fills it with zeroes, so it always has data. A canvas
styled `width: 100000px; height: 100000px` asks for a 40 GB allocation,
which aborts the process. A texture past the GPU's limit (8192 or 16384 per
side, typically) fails wgpu validation [INFERENCE: not run]. Templates are
data (mods, servers, localized content), and ADR 0038 bounds SVG rasters
for the same reason (`MAX_SVG_SIDE` = 4096: "tiny-skia aborts on a failed
allocation").

## Decision

### 1. `<canvas>` is a replaced element with its own node

`<canvas>` becomes an item, like `<img>` directly in a container (the
anonymous-block branch near `build.rs` `is_walked_through`). It gets a leaf
`Node`, an `HtmlElement` (id, classes, dataset), `Focusable` and
`ElementSignals` like any element (`data-on-click`, `tabindex`, `is=` all
work), and it never walks its children. HTML's fallback content inside
`<canvas>` is for browsers without canvas support, which never applies
here.

Size, as in HTML: the `width`/`height` attributes (CSS px, default 300×150)
are the intrinsic size. CSS `width`/`height` override it, with one side
keeping the aspect ratio (the `ImageRun::box_size` rules). For an image
source (§2) with no attributes, the image's own size is the intrinsic size.

Deviation: HTML's `<canvas>` is inline, but here it's always its own box.
`ViewportNode` needs a `Node` entity, and an inline box inside a block's
`Text` (`InlineBox` + `InlineImage`) isn't one. A `<canvas>` inside a text
block (`<p>Map: <canvas …></p>`) splits the block around it, the way a
container inside a block already does (the mixed-content rule). It's logged
at `debug`.

### 2. The app binds names to sources: `HtmlCanvases`

The template names a source and the app registers it:

```html
<canvas data-canvas="minimap" width="256" height="256"></canvas>
```

```rust
let image = images.add(Image::new_target_texture(256, 256, TextureFormat::Bgra8UnormSrgb, None));
let camera = commands.spawn((Camera3d::default(), RenderTarget::Image(image.into()), …)).id();
canvases.insert("minimap", CanvasSource::Camera(camera));
```

`HtmlCanvases` (a resource, in the prelude) maps a name to one of:

- **`CanvasSource::Camera(Entity)`**: the element gets
  `ViewportNode::new(camera)`. The box size drives the resolution (§4), and
  pointer input reaches the camera's scene. This is for live renders the
  player interacts with: a rotatable preview, a clickable minimap.
- **`CanvasSource::Image(Handle<Image>)`**: the element gets an `ImageNode`,
  stretched to the box. The resolution is whatever the app made, and
  bevy_markup never resizes it. This is for fixed-size renders (a 128²
  minimap camera the app sizes itself), CPU-drawn images and video frames.
  It matches HTML's model, where the bitmap size is set apart from the CSS
  size.

The library creates no cameras and no images. Cameras belong to apps
(AGENTS.md conventions), and the library depends on no renderer. A name with
no registration shows an empty box and a one-time `debug` line, like an
undefined `is=` name.

Why a name and not an `id`: ids are per document, while one name can be
shown by many UIs (one at a time for a camera, §5), and the template author
and the app share the name as a contract, like `data-on-*` signal names.
`data-*` is the crate's attribute namespace for app hooks (`data-on-*`,
`data-tooltip`, `data-l10n-id`).

### 3. Bound outside the spec, by one system

The source isn't part of `NodeSpec`. The build only marks the element with
`HtmlCanvas { name, intrinsic }`. A system `sync_canvases` (`PostUpdate`,
after `Build`, before `UiSystems::Prepare`, beside `rasterize_svg_images`)
inserts or replaces the `ViewportNode`/`ImageNode` when an `HtmlCanvas` is
added or changed, or `HtmlCanvases` changes. That means:

- registering, replacing or removing a source never re-renders or rebuilds
  the UI;
- a kept element keeps its binding across updates. `update_spec` doesn't
  own `ViewportNode`/`ImageNode` on a canvas: CSS `border-image` doesn't
  apply to it, so they're added to the components it leaves alone.

### 4. The resolution is bounded: `MAX_CANVAS_SIDE`

For a camera source, `sync_canvases` caps the node's `max_width`/
`max_height` so its physical size stays at most `MAX_CANVAS_SIDE` per side
(4096, the `MAX_SVG_SIDE` value: 64 MB at 4 bytes per pixel). Layout then
never produces a box that would make Bevy's resize allocate more. The cap is
in logical px (side × `ComputedNode::inverse_scale_factor`), recomputed when
the scale factor changes, and it's taken back if the source becomes an
image. An image source is never resized, so it needs no cap: the app chose
its size.

### 5. One camera, one viewport

`ViewportNode` resizes the camera's image to its own node. Two viewports on
one camera would resize it back and forth every layout change. So per
camera, only the first visible canvas in UI order gets the `ViewportNode`.
Every other canvas naming that camera shows its image stretched, as an
image source. One `warn` per name.

### 6. Visibility is reported, not acted on

A camera behind a canvas that isn't shown (the UI despawned, `display:
none`, culled by the windowed build, an unloaded tab) keeps rendering.
Turning it off is app policy: some apps want the last frame kept. So
`HtmlCanvases::shown(name)` reports whether any canvas with that name was
visible (`InheritedVisibility`, laid out at a non-zero size) at the last
`sync_canvases` run. The app toggles `Camera::is_active` from it, as it
reads `HtmlWorldAnchorView::on_screen` today.

## Alternatives considered

- **Keep the `is=` workaround and document it.** It needs no library code,
  but `<canvas>` itself still has no entity, so the workaround needs a
  `<div>` with CSS sizing. Every app re-solves the one-viewport-per-camera
  rule, and nothing bounds the resize: any template can crash any app that
  copies the recipe.
- **`<img src="canvas://minimap">`.** It reuses `<img>`'s inline sizing.
  But an inline image isn't a node, so `ViewportNode` can't sit on it: no
  live resize and no pointer forwarding. It also mixes the asset-path
  contract with a runtime registry.
- **The library spawns the camera and image** (`<canvas data-scene="…">`
  plus a scene description). It would need a renderer dependency and would
  own policy (format, MSAA, clear color, render layers, order) that
  belongs to the app. That breaks the crate's scope rule (AGENTS.md: "Panels,
  layouts, selectors, … cameras … belong to apps").
- **Our own resize and forwarding** instead of `ViewportNode`. It would
  allow capping in the resize itself. But it duplicates about 150 lines of
  Bevy (`viewport_picking`, `update_viewport_render_target_size`) that
  track Bevy's picking internals, which change between releases. Capping the
  node's size bounds the same allocation from the outside.
- **Binding by `id`**, with the registry keyed by element id. Ids collide
  across UIs, and an id is also a styling and focus hook. Renaming one for
  CSS would silently unbind the canvas.
- **A component instead of a resource** (`HtmlCanvasSource` on the camera,
  naming itself). Equivalent, and arguably more ECS-like. Rejected for now
  because the image source has no entity to carry it. The resource can
  later be filled from such a component without breaking the template
  contract.

## Consequences

- A template can place a live render with one tag. The app's code is the
  camera and image (which it writes anyway) plus one `insert`. Pointer
  input reaches the scene through the UI (a camera source), with no app
  glue.
- A template can't allocate more than 64 MB per canvas through a camera
  source, nor make two viewports fight over one camera.
- Costs: one item kind in `build.rs` (§1, a leaf like `<img>`'s anonymous
  block), one system, one resource and its prelude entries, and the cap
  bookkeeping. No new dependency or Bevy feature: `ViewportNode` is in
  `bevy_ui`, `RenderTarget` in `bevy_camera`, which `anchor.rs` already
  uses.
- Testing: the headless harness (`TestUi`, no renderer) can check
  everything but the pixels: the item and its dump line (`canvas=minimap
  camera|image`), the intrinsic and CSS sizes (layout harness), the cap
  under a huge CSS size and a scale factor, the single-viewport rule, and
  rebinding without a rebuild (build counters, as `quiet` does). A golden
  scene can show a camera's clear color in a canvas (`tests/golden.rs`
  already renders offscreen with full Bevy). Pointer forwarding needs
  `TestUi::with_pointer` plus a picking backend on the inner camera
  (a stub backend that reports the forwarded positions is enough).
- Open:
  - `<canvas>` inside a text block splits the block (§1). An inline canvas
    would need Bevy's inline boxes to be nodes.
  - No `object-fit` for image sources: they always stretch, like
    `border-image`'s center.
  - The fuzz harness has no assets or cameras; it covers only the item and
    sizing (an unregistered name).
  - An example (a 3D unit preview in the `world` or `menu` example) and a
    testbench scenario (many canvases, churn, huge CSS sizes) come with
    the implementation.
  - The ADR 0028 roadmap gets a `<canvas>` row when this is accepted.
