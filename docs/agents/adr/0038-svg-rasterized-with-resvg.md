# 38. SVG: rasterize on the CPU with usvg + resvg

| Field | Content |
|---|---|
| `ADR` | `0038` |
| `Title` | SVG: rasterize on the CPU with usvg + resvg |
| `Date` | 2026-11-07 17:19 +0400 |
| `Author` | Claude Opus 5.5 (Anthropic), via Claude Code; direction chosen by Nikita Chashchinskii |
| `Commit` | `5bae780 Fix the Mutants workflow: cap test memory and use 8 shards` + uncommitted (this ADR, revised after the probe); revised again at `d1eae43` + uncommitted step 1 (the recursion-depth check, §5) |
| `Status` | Proposed (steps 1–3 implemented: `.svg` images, `<img>` redrawn at its displayed size, inline `<svg>`) |
| `Related` | ADR 0028 (roadmap item 12: "decide the shape first"), 0002 (library shape, minimal Bevy features), 0036 (in-domain crates replacing maintained code); review_0003 G19 (no remote loads); [probe report](../performance/adr-0038-probe.md) (the evidence for §2, §3 and §5) |

## Context

bevy_markup has no SVG support. ADR 0028 lists it as a tier-4 gap ("no
`<svg>` element handling and no SVG asset path") and leaves the shape
open: pre-rasterized assets via `<img>` versus a `bevy_svg`-class
integration.

What is true today:

- Every image the library draws is a `Handle<Image>` loaded by asset path:
  - `<img src>`: `build.rs` `Styler::image_of` → `self.server.load(resolved)`
    → `InlineImage` in `spawn_spans`. It refuses `://` and `data:` sources.
  - `border-image`, `background-image` and `--name: url(…)` in a sheet:
    `style.rs` `StylesheetLoader` → `load_context.load(path)`, held in
    `Stylesheet::images`. Remote sources are skipped (`template::is_remote`).
  - `*.slice.ron` frames: `nine_slice.rs` → `load_context.load(image_path)`.
- Image *formats* are the app's choice. The library enables no
  `bevy_image` format feature (`Cargo.toml`: `ui_api`, `default_font`,
  `bevy_log`, `bevy_picking`). `bevy_image` 0.20.1 has no SVG feature
  (checked in its `Cargo.toml`), so an `.svg` path loads nothing today.
- The library leaves rendering to the app (ADR 0002, `AGENTS.md`). Most
  of the test suite (vectors, properties, stateful, signals, layout) runs
  headless without a renderer. Only `tests/golden.rs` draws pixels.
- `size_images` (`build.rs`) already resolves an `<img>`'s CSS
  `width`/`height` against its containing block each frame. So the
  displayed logical size of an inline image is known after layout.
- Content may come from mods or users. Every loader so far treats its
  input as hostile: nesting caps, remote refusal, fuzz targets.

The user asked for SVG support and first proposed bevy_vello. After the
comparison below, the decision was to rasterize with usvg + resvg.

## Decision

### 1. Rasterize on the CPU with usvg + resvg, behind an opt-in `svg` feature

Add a cargo feature `svg` that pulls in `resvg` (0.49 at writing; it
re-exports `usvg` and `tiny-skia`) with `default-features = false`:

- No `text` (fontdb, harfrust, skrifa).
- No `system-fonts` / `memmap-fonts`.
- No `raster-images` (gif, webp, jpeg decoders).
- No `svgz` (flate2).

usvg parses and normalizes the document. resvg draws it into a
`tiny_skia::Pixmap`, whose premultiplied RGBA8 becomes a Bevy `Image`
(`Rgba8UnormSrgb`). The output is an ordinary texture, so it goes through
Bevy UI's existing drawing: z-order, clipping, `opacity` and scroll areas
apply unchanged on every backend Bevy supports.

The feature stays off by default, like `text-input`: apps that don't use
SVG pay nothing.

### 2. An `.svg` asset loader that produces `Image`

`src/svg.rs` (new) registers `SvgLoader: AssetLoader<Asset = Image>` for
the `svg` extension:

- Its settings are `SvgLoaderSettings { scale: f32 }`, default `1.0`.
- The pixel size is the document's intrinsic size in CSS px (usvg's
  `Tree::size`: `width`/`height`, else the `viewBox`, else
  `Options::default_size`) × `scale`, rounded up.

Because the result is a plain `Image`, every existing path accepts SVG
with no change in `build.rs` or `style.rs`: `<img src="icon.svg">`,
`border-image: url(frame.svg) 8`, `background-image`, `--icon:
url(…)`, and `.slice.ron` images.

CSS-loaded images keep `scale = 1`. That way `border-image-slice`
numbers (image px, `AGENTS.md` "Box properties") and `%` slices mean
the SVG's own px, as in a browser.

Coexistence with another `.svg` loader (measured with bevy_resvg's real
`SvgPlugin`, both registration orders; probe §1):

- Typed `load::<Image>` and `load_context.load::<Image>` always reach
  ours, and bevy_resvg's typed loads reach its own.
- An **untyped** load, or any **labeled** path, takes whichever loader
  was registered *last* (`bevy_asset` `AssetLoaders::find`).

The library makes only typed, unlabeled loads, so it is unaffected. Two
rules follow:

- The loaders never rely on labels (§3).
- The `svg` feature's docs state Bevy's untyped-load rule for apps that
  also use another SVG crate.

bevy_svg 0.19 and bevy_vello 0.14 depend on `bevy ^0.19` and can't share
a Bevy 0.20 app today.

### 3. `<img>` re-rasterizes at its displayed size

A 1× raster blurs at `UiScale` 2, on HiDPI displays, or when CSS sizes
the image up. For `<img>`, the parsed document is its own asset type,
`SvgDocument(Arc<usvg::Tree>)`, behind a second loader for `svg` that
the build loads typed next to the `Image`. It is not a labeled sub-asset
of the `Image` load: a labeled path skips type matching, and it failed
with `MissingLabel` whenever bevy_resvg registered after us (probe §1).
After layout, a system `rasterize_svg_images` (`PostUpdate`, after
`size_images`):

- computes each SVG `<img>`'s physical size: its displayed logical size
  × the target's scale factor (which already includes `UiScale`);
- when that differs from its current raster, draws a raster of that
  size into a cache keyed by (document asset id, pixel size). Entities
  showing one SVG at the same size share a handle; unused rasters drop
  with their handles;
- swaps the drawn handle while keeping the logical box at the size the
  1× image would give.

How the swap survives the build (as implemented in step 2). When the
build updates a span it writes the 1× source back into `InlineImage`
(`update_spec`'s `put`, only when it differs). The loop the `AGENTS.md`
gotcha describes ("App-written `Node` fields don't survive") can't
start: the gotcha's loop needs a write that triggers a restyle, and an
`InlineImage` write triggers none.
- `rasterize_svg_images` runs after `size_images` and before
  `UiSystems::Prepare`. Bevy reads `InlineImage` in
  `update_inline_image_boxes` (`UiSystems::Content`), so whenever the
  build writes the source back, the system restores the raster in the
  same frame and no frame shows the 1× image. Its state lives in an
  `SvgRaster` component the spec never writes: the source, the intrinsic
  size, the shown raster, and last frame's wanted size.
- The swap pins `InlineImage::width`/`height` to the box's logical size;
  otherwise Bevy sizes the box from the raster's pixels.
- `size_images` takes the aspect ratio from `SvgRaster::intrinsic`, not
  from the raster's rounded pixel size: at 33.3% of 400 px the raster is
  134 × 34, and its own aspect would move the box from 33.3 to 33.8 px.

Draws run as `AsyncComputeTaskPool` tasks (a budgeted draw can still take
tens of milliseconds, §5); until one finishes, the previous image stays
on screen.
- A size is drawn once it has held for a frame (`SvgRaster::wanted`),
  not on every step of a resize. [INFERENCE: one frame is enough; not
  measured during a real window drag.]
- The cache (`SvgRasters`) keys finished draws, draws in flight and
  sizes the budget refused by (document, pixel size). Entries no span
  uses are dropped every frame.
- `SvgDocument` has its own loader, so an `<img>` SVG is parsed twice
  (once for the 1× image, once for the document), both on IO threads.
- usvg and resvg run under `svg::guarded` (`catch_unwind`). Polling a
  panicked task re-raises the panic in the poller, here the main thread,
  and resvg still panics on some input where overflow checks are on
  (#1185, UPSTREAM U20; probe §8).

Tests, on the layout harness (`html_ui` `svg_display_size`):
- a CSS width of 4× is drawn at 128 × 32 in a 128 × 32 box;
- a restyle never shows the 1× image, checked by a recorder system in
  `Last`, every frame;
- the intrinsic size keeps the loaded image;
- `UiScale` 2 draws 64 × 16 into a 32 × 8 box;
- a `%` width keeps the intrinsic aspect;
- a size the budget refuses keeps the 1× image.

Each of the three mechanisms was removed once and its test failed: the
put-back, the pinned size, the intrinsic aspect.

### 4. Inline `<svg>` is a replaced inline element

Within the same feature, the walk treats `<svg>` like `<img>`: an inline
run (`InlineBox` + `InlineImage`) whose image the build draws. As
implemented in step 3:

- **The source.** `tag.raw()`, the element's span in the rendered HTML,
  goes to usvg, so the walk doesn't need to understand SVG; its children
  are never walked as text. `prepare_inline` adds what HTML lets the
  author omit and XML needs: `xmlns`, and `xmlns:xlink` when the text
  uses `xlink:`. It also sets `color` to the element's computed CSS color
  (`rgba(…)`), unless the author wrote one.
- **Well-formed XML only.** usvg parses XML. An unquoted attribute, an
  unclosed tag or `&nbsp;` makes the element show nothing (debug log).
  Finding this exposed bug_0185: `tl` read `<rect/>` as an unclosed
  element named `rect/` and swallowed the document after it. All `tl`
  input is now normalized first (UPSTREAM U21).
- **`currentColor`** follows the cascade, so `svg { color: … }`, `:hover`
  and themes recolor icons through the existing restyle. The color is
  part of the cache key (the text), so each color is its own drawing.
- **Sizing:** CSS `width`/`height` as for `<img>` (`size_images`), else
  the document's own size (usvg's `Tree::size`: `width`/`height`, else
  the `viewBox`). Browsers default a size-less inline SVG to 300 × 150,
  or stretch a `viewBox`-only one to the container; bevy_markup uses the
  `viewBox` size. The intrinsic size comes from the parsed tree when the
  build collects the element (`ImageRun::intrinsic`), so the box is sized
  in that build.
- **When it's drawn.** `InlineSvgs` (a `Mutex`-guarded cache keyed by
  the prepared text) reserves an image handle.
  - Text up to 64 KiB is parsed during the build. A document estimated
    under `INLINE_SYNC_COST` (2²⁰ units, ~1–4 ms) is drawn then too, and
    `finish_inline_svgs` (after `Build`, before `size_images`) inserts it
    the same frame. `a_recolor_shows_no_blank_frame` fails with 6 blank
    frames when every draw goes to a task.
  - Larger or costlier documents draw on the `AsyncComputeTaskPool`.
    Their insertion is followed by a `Modified` event, which restyles the
    UI as an `<img>` load does.
- **Sharing and lifetime.** Elements with the same text share one image.
  Entries no `InlineImage` has shown for 120 frames are dropped.
  `finish_inline_svgs` gives each parsed document an `SvgDocument`, and
  `rasterize_svg_images` finds it through `InlineSvgs::document_of`, so
  inline SVG is redrawn at its displayed size like an `<img>` (§3).
- **Without the feature** an `<svg>` is walked through as before (an
  unknown inline element whose text shows).

Deliberate limit: page CSS doesn't reach inside the SVG. A rule like
`.icon path { fill: red }` doesn't apply. Only the inherited `color` (via
`currentColor`) and the box size cross the boundary. Styles inside the
SVG's own `<style>` and attributes do apply (usvg handles them).

`<svg>` written as raw HTML in Markdown passes through the conversion
unchanged (`md.rs`: raw HTML passes through), so it works in `.md`
templates too. A blank line inside it ends CommonMark's raw HTML block.

Tests (`html_ui` `inline_svg`, `svg::tests`):
- in text, with a `<text>` child that must not leak;
- `currentColor` and a restyle, plus a no-blank-frame recolor;
- in a container, CSS-sized;
- `xlink:` without its namespace;
- malformed and hostile documents skipped;
- in Markdown;
- the `UiScale` redraw.

Falsified: without the cascade color, three tests fail; with the
children walked, six; without the collect-time intrinsic size, one.

### 7. CSS-loaded SVGs at the scale factor (added 2026-11-07)

`border-image` and `background-image` SVGs were drawn at 1× and blurred
on HiDPI or under `UiScale`. The redraw system now also takes
`ImageNode`s the build owns: `CssFrame` entities, and roots whose
`CssRoot` owns the image. An app's own `ImageNode` is left alone.

- **Sliced or tiled:** drawn at intrinsic size × the node's scale factor
  (`1 / ComputedNode::inverse_scale_factor`).
- **Stretched:** drawn at the node's physical size.

Bevy's slicer (`bevy_ui_render` `compute_texture_slices`) reads the slice
insets in image px twice: as `inset / image_size` to cut the texture, and
as `inset × min(node / image, max_corner_scale)` to size the corners on
screen. Tiles are drawn at `image × stretch_value`. So for a raster `s`×
larger, `scale_mode` multiplies the insets by `s` and divides
`max_corner_scale` and every tile `stretch_value` by `s`. That cuts the
same regions and draws them at the same logical size, with `s`× the
pixels. Only `NodeImageMode::Auto` sizes a node from its image
(`update_image_content_size_system`), and the build never uses it, so
layout is unaffected.

As for `<img>`, the build re-inserts the 1× `ImageNode` on every update;
`SvgFrame` keeps the 1× mode, and the system restores the raster and the
scaled mode the same frame.

Tests (`html_ui` `svg_css_images`, `UiScale` 2):
- a 40×20 frame with 25% slices draws at 80×40, insets 10/5 → 20/10,
  corner cap 0.5, and no frame shows the 1× image across a restyle;
- a tiled background draws 2× with `stretch_value` 0.5;
- a stretched background draws at 200×100 for a 100×50 node;
- scale factor 1 is untouched.

Without the mode scaling two tests fail; without the put-back, one.
`.slice.ron` frames (`NineSliceFrame`, app nodes) are still 1×.

### 9. Testbench scenarios and profiling (added 2026-11-07)

With `--features svg`, the testbench has three SVG scenarios:
- **perf, `svg-icons`:** N rows with an inline `<svg>` whose
  `currentColor` follows the row's state.
- **perf, `svg-images`:** N `<img>` rows of one SVG file at a CSS width
  that steps every 30 frames: redraw bursts.
- **security, `svg-bombs`:** the probe report's hostile constructs as
  `<img>` files and inline.

Two new checks: `svg-drawn` (every inline image of the main UI shows a
drawn image) and `svg-refused` (no hostile SVG produced an image, and
every `<img>` one failed its load).

The SVG systems and hot internals carry `#[hotpath::measure]`:
`rasterize_svg_images`, `finish_inline_svgs`, `sync_svg_fonts`,
`InlineSvgs::image`, `parse_with`, `recursion_depth`, `estimate_cost` and
`draw`.

First run (release, `--size 500 --no-render --exit-after 600`):
- every check passed, worst frames 16.7–21.8 ms against the 250 ms
  budget;
- in `svg-icons`, `rasterize_svg_images` and `finish_inline_svgs`
  averaged about 20 µs per frame each (≈0.1% of the run).

The first `svg-bombs` run found bug_0186: an inline SVG over the render
budget was refused only on its draw task, after the build had already
sized its box. It's now refused while the build collects it.

### 8. Benchmarks (added 2026-11-07)

`benches/svg.rs` (criterion) and `benches/svg-divan.rs` (divan,
allocation counts) measure the loader's stages through doc-hidden glue in
`bevy_markup::fuzz` (`svg_depth`, `svg_parse`, `svg_cost`, `svg_draw`,
`svg_prepare_inline`; hence `--features fuzzing`). They also measure
frame workloads on the shared headless app: inline icons idle, inline
icons recolored every frame (each a new drawing), and `<img>` SVGs idle
after their redraw. Both run nightly next to the pipeline suites.

First run (release, best of criterion's samples, Ryzen 7 9800X3D):

| Stage | icon | illustration (40 groups) | drop-shadowed 512² | 30-pattern chain |
|---|---|---|---|---|
| depth check (incl. the XML parse) | 0.78 µs | 21.5 µs | 1.2 µs | 14.7 µs |
| full parse | 3.5 µs | 135 µs | 4.2 µs | 57 µs |
| cost estimate | 11 ns | 0.68 µs | 19 ns | 0.23 µs |
| draw 1× | 7.9 µs | 2.9 ms | 16 ms | 53 µs |
| draw 2× | 28 µs | 9.9 ms | 67 ms | 143 µs |

Notes on the table:
- The depth check's own share of a parse is less than its column: in the
  real path its XML parse is the one usvg reuses.
- Drawing dominates whenever there are filters or many layers, which is
  what the render budget bounds.

| Frame workload | 120 | 600 |
|---|---|---|
| inline icons, idle | 0.14 ms | 0.27 ms |
| inline icons, all recolored per frame | 1.2 ms | 4.8 ms |
| `<img>` SVGs, idle after redraw | 0.15 ms | 0.30 ms |

### 6. SVG `<text>`: the `svg-text` feature (added 2026-11-07)

usvg converts `<text>` to outlines while it parses, so the fonts must be
known then. The feature turns on resvg's text stack (fontdb, harfrust,
skrifa); `svg` alone stays lean for icon-only apps.

- **Which fonts.** `svg_fonts::SvgFontSet` loads into a fontdb database
  every face of `FontFamilies` given as a font file (`FontSource::Handle`:
  its `Font::data` blob, not copied), plus Bevy's default font (its
  default handle).
- **How `font-family` resolves.** A custom `usvg::FontResolver` resolves
  the SVG's `font-family` as the page's CSS does: registered names in any
  case, generics through `set_generic`, `FontFaces::face`'s fallback
  (bold-italic → italic → bold → regular), and an unknown family → the
  default font. Names inside the font files are never used. Unit test:
  three copies of FiraMono as distinct faces, each glyph's resolved
  `fontdb::ID` (`Text::layouted`) checked (fails when the resolver
  ignores the family list).
- **Fonts that arrive later.** `sync_svg_fonts` rebuilds the set when
  its signature changes: families × face handles × loaded. It then
  reloads every `.svg` path whose source has `<text>` (the loaders note
  them), and the redraw cache drops rasters of a `Modified`
  `SvgDocument`. It also signals a `FontFamilies` change, which restyles
  the UIs: inline text is keyed by the set's generation, so the build
  draws it again. The next run sees the same signature, so it doesn't
  loop (tested). `a_font_that_arrives_later_redraws_the_text` fails
  without the reload and without the generation key.
- **Limit.** System font families (`FontSource::Family`, `Generic`,
  `system_fonts`) have no bytes in Bevy's assets, so SVG text naming them
  uses the default font.

### 5. Safety limits

SVG is untrusted input, like every other loader input here. The probe
(§2–§4) measured what each limit has to stop.

- **No external resources.** `Options::image_href_resolver` is replaced
  by one whose two closures return `None`, and `resources_dir` stays
  `None`. usvg's default string resolver `std::fs::read`s any path:
  - absolute paths;
  - relative ones against the process cwd, or against `resources_dir`
    with no `..` containment.

  It embeds bitmaps and renders SVGs from them, `raster-images` or not.
  Under `strace`, the refusing resolver made every case read nothing.
  `<use>`, CSS `@import`/`url()`, `xml-stylesheet` and external entities
  never read files with either resolver. `data:` URLs are refused too in
  the first version: a `data:` SVG fan-out costs parse time in
  proportion to its size (probe §3), and nothing needs it yet.
- **Pixel cap, checked before allocating.** No raster exceeds
  `MAX_SVG_PIXELS` (16,777,216: 4096 × 4096). Larger requests scale down
  in proportion, with a one-time warn. The cap must be applied before
  `Pixmap::new`: a 100,000 × 100,000 request **aborted the process**
  (allocation failure) instead of returning `None`.
- **A render budget from the parsed tree.** The pixel cap doesn't
  bound filters. Files under 45 KB took 2–15 s, or more than 30 s and up
  to 4.7 GB, inside it (probe §4). `resvg::render` can't be cancelled,
  so the loader estimates the cost from the `usvg::Tree` before
  rendering. The estimate is the raster area × the sum over the tree of:
  - each filter primitive's weight, scaled by its filter region
    relative to the element's box: `feTurbulence` × `numOctaves`,
    `feMorphology` × radius², `feConvolveMatrix` × order², others × 1;
  - one unit per isolated group (opacity, filter, mask, clip path),
    times its nesting depth.

  Those are the scaling laws measured in probe §4. A document over
  `MAX_SVG_COST` fails its load with an error naming the cause. The
  constant is set during implementation from a benchmark, aiming at a
  worst-case raster in the tens of milliseconds at the pixel cap
  [INFERENCE: the cost model's weights hold across primitives; a
  property test checks estimate vs measured time].
- **Parse limits.** roxmltree rejects entity loops (depth 10, 255
  references per entity). usvg rejects trees over 1,000,000 nodes
  counted after `<use>` expansion. That stopped a 10⁷-node `<use>` bomb
  in 0.06 s. Memory stays proportional to input size.
- **A static recursion-depth check before usvg converts** (revised during
  step 1, probe §6). On a 2 MB thread (Bevy's task-pool default), usvg
  0.49's converter overflows the stack at a few KB of input:
  - 113 `feImage` hops, 121 chained patterns or 251 `<use>` hops in debug;
  - ~689 nested `<g>` in debug, below its own 1024-level check.

  Upstream's `max_depth` (#1223, unreleased) doesn't count references
  (UPSTREAM U17). So `svg::parse`:
  1. parses the XML once with roxmltree, configured as usvg configures it;
  2. bounds the recursion with `svg::recursion_depth`;
  3. hands that same document to `usvg::Tree::from_xmltree`.

  The bound is the longest path over parent → child and element →
  referenced-element edges:
  - weights are 1 per element, 2 per `<svg>`, +6 per reference;
  - a reference is any `url(#id)` or `#id` in an attribute, or in a
    `<style>` rule matched with simplecss as usvg matches it;
  - cycles are refused, since usvg expands `<use>` cycles to its depth
    limit;
  - the limit is 256, about 800 KB in debug.

  One parse means the check and usvg can't disagree about the document
  (a DTD entity that expands into markup counts). A test draws the
  deepest accepted document of every construct, and of mixes, on a 2 MB
  debug thread. roxmltree 0.21.1 also recursed per level (~14 KB each in
  debug); 0.21.2 fixed it, and the crate takes it as a version floor
  (UPSTREAM U18).

  The check is sound only while it uses usvg's own parser and selector
  engine:
  - roxmltree comes from usvg's re-export (`usvg::roxmltree`), so the
    versions can't differ (`Tree::from_xmltree` would also fail to
    compile);
  - simplecss is private in usvg, so
    `usvg_and_the_depth_check_share_one_simplecss_and_roxmltree` fails if
    `Cargo.lock` holds two (falsified with a second copy). Both ask for
    `0.2`, so an app's build always resolves one; a split arrives only
    with a resvg bump here.

  The same coupling for the CSS pre-scan is a compile-time check
  (ADR 0039).
- **Remote paths.** `.svg` assets go through the existing `is_remote`
  refusal like any other image.
- **Fuzzing.** A fuzz target `svg` over the loader's parse, budget and
  raster. Seed it with the probe's cases, minimized. It also runs each
  input through the inline-`<svg>` preparation (`prepare_inline`) and
  draws that; the `html` target doesn't build, so it never reaches inline
  `<svg>`.

### 6. Verification

- Loader unit tests: intrinsic size from `width`/`height` vs `viewBox`,
  `scale`, the pixel cap, a refused `<image href>`.
- Headless vectors (`tests/html_ui.rs`): CPU rasterization needs no
  renderer, so `TestUi` can assert the loaded image's size and the
  inline box. A layout vector `layout_svg` checks `<img src=x.svg>` and
  inline `<svg>` rects against Chromium via the existing layout oracle
  (`scripts/browser_oracle.py`). The intrinsic-sizing rules
  (`width`/`height`/`viewBox`, one CSS dimension preserving the ratio)
  are exactly what a browser can arbitrate.
- A golden scene `tests/golden/svg/`: committed SVGs with fills,
  strokes, a gradient, `currentColor` and an SVG `border-image`.
- `content_lint`'s CSS `url()` check already covers `.svg` URLs.

### Implementation order

1. Feature, loader, safety limits, fuzz target. This makes `<img>` and
   CSS `url()` SVGs work at 1×.
2. Display-size re-rasterization for `<img>` (§3).
3. Inline `<svg>` (§4).

Each step ships on its own with its vectors, a CHANGELOG line and the
`AGENTS.md` updates (Public API row for `SvgLoaderSettings`, Layout,
Limits).

## Alternatives considered

- **bevy_vello (with vello_svg).** This was the user's first proposal.
  Its newest release, 0.14, targets Bevy 0.19, and nothing supports Bevy
  0.20 (README compatibility table, read 2026-11-07). Vello renders with
  GPU compute shaders in its own pass, which brings these problems:
  - It needs WebGPU on the web; there's no WebGL2 path.
  - It needs a renderer, so the headless test layers can't see it.
  - It draws outside Bevy UI's z-order and clipping. [INFERENCE: not
    prototyped.]
  - The library would depend on rendering, which ADR 0002 leaves to the
    app.

  Its README calls SVG support "limited", and vello_svg parses with usvg
  anyway, so it adds a renderer without adding SVG coverage. Worth
  revisiting only for animated vector content (Lottie via velato), as a
  separate backend behind its own feature.
- **bevy_svg (0.19).** It draws SVGs as meshes for sprites and 3D: its
  features enable `bevy_sprite` (`2d`) and `bevy_pbr` (`3d`). It doesn't
  produce UI images. It tessellates with lyon (`lyon_tessellation` among
  its crates.io dependencies) and depends on `bevy ^0.19`, so it can't
  share a Bevy 0.20 app today.
- **bevy_resvg (3.0.1).** Same engine, and it supports Bevy 0.20. Read
  in source (probe §5):
  - Its loader is crate-private and produces `SvgAsset(Arc<Tree>)`, not
    `Image`, and its rasterizer is crate-private too. The public surface
    is its plugin and its `Svg` / `UiSvg` components.
  - It needs Bevy's `bevy_sprite_render` (a renderer dependency) and
    `resvg ^0.48` with default features (system fonts, raster decoders).
  - It parses with usvg's default resolver, which reads files (§5).

  There is nothing to reuse behind the library's `Handle<Image>` paths.
- **Status quo: apps pre-rasterize to PNG.** No dependency, but no
  sharp scaling, no `currentColor` theming and no inline SVG. Keeping
  this means closing ADR 0028 item 12 as "won't do".
- **For the stack overflows (§5), weighed during step 1:**
  - *A dedicated loader thread with a large stack.* This was chosen first,
    then dropped for the static check, which the user proposed. The
    thread needs a stack sized from the input (up to ~500 B of debug
    stack per input byte, measured), so it reserves hundreds of MB for
    large files. It doesn't exist on wasm, and it's a workaround to
    remove later. The static check costs one linear pass over a
    document that is parsed once anyway, and works on every target.
  - *Patch usvg in a fork via `[patch.crates-io]`*, like the
    fluent-syntax fork. `[patch]` doesn't reach crates that depend on
    bevy_markup, so their apps would stay exposed unless they copied the
    entry.
  - *Another XML parser for the check* (quick-xml, xml-rs). It would be
    no more robust than roxmltree 0.21.2, which is iterative and caps
    entities. It would open parser differentials: a parser that doesn't
    expand DTD entities into markup sees a flat document where usvg sees
    600 levels.
  - *Wait for upstream.* A usvg release that counts references in
    `max_depth` makes the check removable (a PR sketch is in U17), but
    there's no such release yet.
- **Hand-write a rasterizer or a narrower SVG subset.** This is the
  kind of code ADR 0036 replaced with a maintained crate, and SVG is far
  larger than the entity table was.

## Consequences

- SVG icons, frames and backgrounds work through the paths authors
  already use, in every renderer Bevy supports, and testable headlessly.
- New dependency tree under `svg` only: resvg, usvg, tiny-skia,
  roxmltree, kurbo, svgtypes and their dependencies. Compile time and
  binary size need measuring in step 1 (cold build of `--features svg`
  vs without, `cargo bloat` or the release `.rlib` sizes).
- CPU cost at load and on each re-rasterization. Measure a large icon
  sheet and a full-window SVG background at 4K in step 2 (criterion,
  next to the existing benches). Raster memory grows with the number of
  distinct display sizes. Loaders run on Bevy's `IoTaskPool`, so a slow
  SVG holds an IO thread and delays other loads, not frames.
- The render budget rejects some legitimate heavy SVGs (deep filter
  stacks at large sizes). Each rejection is a load error naming the
  cause, and `MAX_SVG_COST` is one constant to revisit.
- Static SVG only: no SMIL animation, no scripts. `<text>` inside SVG
  renders nothing without the `text` feature.
- Open:
  - SVG `<text>` with system font families (§6: font files only).
  - Page CSS reaching inside inline `<svg>` (usvg's
    `Options::style_sheet` could carry the matching rules).
  - Whether `svg` should join a future default feature set.
