# review_0007 — Code review 2026-11-11 (the crate at `f76b320`: SVG, `<canvas>`, the selector engine)

| field | value |
|---|---|
| ID | review_0007 |
| Date | 2026-11-11 |
| Scope | the crate at `f76b320`, with the weight on what no review has read: the 63 commits since [review_0006](review_0006.md) (`bd99546`) — SVG (ADR 0038, 0040, 0042: `svg.rs`, `svg_bounds.rs`, `svg_boxes.rs`, `svg_fonts.rs`), `<canvas>` (ADR 0041), selector matching with parcel_selectors (ADR 0034), anchored overlays' same-frame placement, HTML Tab order, the `json` filter, the Tera operator bound, `normalize_self_closing`, `pick_inline_boxes` — ~11 000 added lines in `src/` |
| Method | one session: CI's exact command list (`ci.yml`'s env: `-D warnings`, debug info off) at `f76b320`; the new modules read whole against their ADRs and against the upstream code they model (resvg 0.49, tiny-skia 0.12, Bevy 0.20's `ViewportNode`, Tera 2.4's limits); 14 behaviour probes (temporary tests, since removed), unit probes on the SVG glue and pipeline probes on the headless harness, timed in release builds |
| Focus | correctness, security (content as untrusted input), performance, and whether the bounds bound what they claim |
| Findings | `K01`… (stable ids) |
| Discovery agent | Claude Code session, `claude-fable-5-2` |
| Machine | Arch Linux (kernel 7.2.14), AMD Ryzen 7 9800X3D, rustc 1.98.1 |

Nothing is fixed in this session; no code changed.

## Coverage — read this first

**Probed** ([probe], output quoted): K01–K15.

**Read** ([code]): `canvas.rs`, `svg.rs` (the loaders, the inline cache,
the redraw, both walks), `svg_bounds.rs`, `svg_boxes.rs`, `svg_fonts.rs`,
`cascade/selector.rs`, `anchor.rs`, `tooltips.rs`, `lib.rs`'s wiring,
`focus.rs`'s Tab order, `signals.rs` `pick_inline_boxes`, `template.rs`'s
operator scan and `normalize_self_closing`, `build.rs`'s `size_images`,
`image_size` and `inline_svg_of`, the `svg` fuzz harness, `source_lint`.

**Not covered:** `build.rs` and `cascade.rs` beyond those functions (their
18 000 lines changed by ~4 500 since review_0005; I read the parts the new
features touch), `md.rs` and `l10n.rs`'s changes, the windowing follow-ups
after review_0006 (bug_0160–0177), the examples' code, the golden scenes,
the benchmarks. No fuzz runs, no mutation run, no windowed run of an
example.

Evidence levels as before: **[probe]**, **[code]**.

## Summary

CI's whole list is green, which it wasn't at reviews 0003 to 0006.
The records are current, and the new code is careful in the places it
looked: every SVG refusal I tried to get around by its own route held.

The findings are in the places it didn't look. The SVG bounds model what
resvg does with paths, filters, masks and patterns; three things that
also draw are outside the model. Text is one (`svg-text`): its glyphs
cost nothing in the estimate, and a pattern painted on text skips the
tile check that bug_0200 added, so 250 bytes abort the process again.
Dashed strokes are another (about 1 000 times the calibrated cost per
unit). The `<style>` pass is the third (rules × elements, three times
over, before any budget counts). The same shape shows in the selector
engine: ADR 0034's allowlist keeps out what parcel panics on, but
`:is()` nested with descendant combinators costs depth^levels, and 104
bytes of CSS over a 1 KB page took two minutes.

Outside the bounds: `%` image sizes mix physical and logical pixels (a
`width: 50%` image is 100% wide at `UiScale` 2); a canvas with a CSS
frame loses its picture on the first in-place update; hiding a canvas
shrinks its camera's image to 1 × 1; a refused inline `<svg>` is parsed
again on every build, on the main thread.

## CI at `f76b320` (ci.yml's env)

| command | exit |
|---|---|
| `check --lib` | 0 |
| `check --lib --features text-input` | 0 |
| `check --lib --features fuzzing` | 0 |
| `check --lib --features svg` | 0 |
| `check --lib --features svg-text` | 0 |
| `build --all-targets` | 0 |
| `test` | 0 (521 tests, 2 ignored) |
| `test --features text-input` | 0 |
| `test --features svg --lib --test html_ui --test content_lint` | 0 |
| `test --features svg-text --lib --test html_ui` | 0 |
| `check --examples --features hot-reload,dev-tools,text-input,svg-text` | 0 |
| `test --example hot-reload --features hot-reload` | 0 |
| `test --lib --features fuzzing` | 0 (243) |
| `doc --no-deps` | 0 |

My first `cargo test` run failed `source_lint`: it caught the `println!`s
of a probe file I had left in `src/`. The lint works (but see K16).

## New findings

### SVG

**K01 · high · A pattern painted on SVG text aborts the process** (`svg-text`) [probe]
- `CostWalk::node` charges a text node `NODE_COST` and stops
  (`usvg::Node::Text(_) => self.add(NODE_COST)`, `svg.rs:1690`). resvg
  draws the node's flattened group: one path per glyph, with the text's
  paint. bug_0200's tile charge lives in `CostWalk::path`, which those
  paths never reach. `ExtentWalk` does walk `text.flattened()`, but it
  bounds reach, not tile size.
- **Probe** (bug_0200's document with the `<rect>` replaced by text; 64 ×
  64, Bevy's default font registered):

  ```xml
  <pattern id='p' width='1e4' height='1e4' patternUnits='userSpaceOnUse'>
    <rect width='1' height='1'/></pattern>
  <text y='8' font-size='8' fill='url(#p)' transform='scale(100)'>A</text>
  ```

  | document | estimate (limit 134 217 728) | draw |
  |---|---|---|
  | the `<rect>` (bug_0200) | 2 000 000 110 904 | refused |
  | the `<text>` | 8 384 | `memory allocation of 4000000000000 bytes failed`, SIGABRT |

- Reachable from any `.svg` file and any inline `<svg>` in an app built
  with `svg-text`. `guarded` can't catch an abort.
- **Fix direction:** walk `text.flattened()` in `CostWalk` as `ExtentWalk`
  does (that also fixes K04).

**K02 · medium (DoS) · Dashed strokes cost about 1 000 times their estimate** [probe]
- The estimate charges a path its segments and its bounding box once per
  paint. `stroke-dasharray` appears nowhere in `src/`. tiny-skia dashes
  at draw time, up to 1 000 000 dashes per contour (`dash.rs`
  `MAX_DASH_COUNT`), each stroked on its own.
- **Probe** (release; a 4000 × 100 document of
  `<path d='M0 y H4000' stroke='black' stroke-width='1'
  stroke-dasharray='0.0041'/>`, ~490 000 dashes each):

  | paths | bytes | estimate | of the budget | draw |
  |---|---|---|---|---|
  | 4 | 396 | 832 512 | 0.62% | 132 ms |
  | 16 | 1 368 | 929 856 | 0.69% | 520 ms |
  | 16, undashed | | | | 1.8 ms |

  One path adds ~8 100 units and ~32 ms: 4 µs per unit against the
  calibrated 3.8 ns. The budget admits ~16 000 such paths (a 1.4 MB
  file): about 9 minutes of drawing on an asset or compute thread, which
  nothing can stop.
- The probe report (`adr-0038-probe.md` §3) tested only a dash so small
  that tiny-skia gives up (`0.0001` over 10⁹ px: "small").
- **Fix direction:** charge `min(length / interval, 10⁶)` dashes per
  contour (the length from the path's bounds is an upper bound per
  segment), or refuse dash intervals below a fraction of a pixel.

**K03 · medium (DoS) · Glyph outlines aren't in the cost estimate** (`svg-text`) [probe]
- The cause is K01's. Without a pattern, the drawing is merely unbounded.
- **Probe** (release; a 4096 × 4096 document, one `<text font-size='3000'>`
  of N `M`s, each placed at `x='0'`):

  | glyphs | bytes | estimate | draw |
  |---|---|---|---|
  | 50 | 267 | 33 554 560 (25%) | 94 ms |
  | 200 | 717 | 33 554 560 | 224 ms |
  | 800 | 2 517 | 33 554 560 | 710 ms |

  800 `<rect>`s of one glyph's box are refused (estimate 136 157 992).
  The work bound charges text 16 units per byte, so it admits ~524 000
  glyphs: about 7 minutes at 0.85 ms each.
- The `svg` fuzz target can't find K01 or K03: `fuzz::svg` draws with
  `SvgFonts::default()` (no fonts, text draws nothing), and `fuzzing`
  doesn't imply `svg-text` (K16).

**K04 · medium (DoS) · `<style>` rules × elements is quadratic, and outside every budget** [probe]
- `Graph::new` matches every kept rule against every element
  (`svg_bounds.rs:387`). usvg's `resolve_css` does the same, and
  `svg_boxes::open` a third time when the document has a filter, mask or
  pattern. A rule that matches nothing adds no edge and no declaration,
  so `MAX_REFERENCES` never counts it; elements cost 16 units of work
  each whatever the sheet holds.
- **Probe** (release; R rules `qN{r:1}`, 4R `<g/>` elements, `svg::parse`):

  | rules | elements | bytes | parse |
  |---|---|---|---|
  | 2 000 | 8 000 | 50 974 | 90 ms |
  | 4 000 | 16 000 | 102 974 | 363 ms |
  | 8 000 | 32 000 | 206 974 | 1.50 s |

  4× per doubling. A 1 MB file is ~37 s, inside `MAX_SVG_WORK` (32 000
  elements are 6% of it). ADR 0040 promises ~0.5 s at the limit.
- **Fix direction:** add rules × elements to `Bounds::work` (it is known
  before the loop runs), or cap the product.

**K05 · medium · A refused inline `<svg>` is parsed again on every build, on the main thread** [probe]
- `InlineSvgs::image` inserts a cache entry only on success. Each early
  `return None` (a parse error, `TooExpensive`, a failed draw:
  `svg.rs:857-884`) leaves nothing behind, so the next build converts the
  document again. A build is any content update or restyle of that UI: a
  `:hover` flip, a context change.
- The parse is synchronous for any text up to `INLINE_SYNC_BYTES` (64
  KB). Only the *draw* is gated by cost (`INLINE_SYNC_COST`); the
  conversion may take the whole of `MAX_SVG_WORK`.
- **Probe** (release; a 1 248-byte inline document: a `<use>` fan-out of
  10⁵ rects, inside the work budget, and an `feTurbulence` the cost
  estimate refuses; three calls of `InlineSvgs::image`):

  | call | result | time |
  |---|---|---|
  | 1 | `None` | 552 ms |
  | 2 | `None` | 499 ms |
  | 3 | `None` | 480 ms |

  The same document without the filter: `Some`, then 1.5 µs per call.
- So one refused icon makes every hover in its UI a half-second frame,
  and an accepted one may stall its first build as long.
- **Fix direction:** cache the refusal under the same key (and age it out
  like a success); send a document whose `Bounds::work` is over a small
  threshold to the task, as large text already goes.

**K06 · medium (DoS) · Nothing bounds the rasters' sum** [probe]
- `MAX_SVG_SIDE` and `MAX_SVG_COST` bound one drawing. `SvgRasters` keeps
  one per (document, displayed size) in use, and `InlineSvgs` one per
  distinct text for 120 frames. A page chooses how many.
- **Probe** (`svg`; one 16 × 16 `icon.svg`, N `<img>`s with
  `style='width: 1500px'`, `1501px`, …; the bytes of `Assets<Image>`):

  | `<img>`s | page | image bytes |
  |---|---|---|
  | 4 | 175 B | 36.1 MB |
  | 8 | 343 B | 72.3 MB |
  | 16 | 679 B | 145.4 MB |

  9 MB each here; 64 MB each at 4096 px, so 100 tags (4 KB of template)
  are 6.4 GB, each also a concurrent task holding a pixmap and a copy.
  `canvas.rs` refuses a single 100 000 px canvas for this reason.
- **Fix direction:** a byte budget over `SvgRasters` and the inline
  cache; past it, a span keeps its 1× image (it's what a non-SVG image
  gets).

**K07 · low · The word `color` in any attribute value turns `currentColor` off** [probe]
- `prepare_inline`'s `has` splits the start tag on whitespace and
  compares each piece up to `=` (`svg.rs:800-804`). A quoted value with
  spaces splits too, and a bare word in its middle reads as an attribute.

  | start tag | `color` added |
  |---|---|
  | `<svg viewBox="0 0 1 1">` | yes |
  | `<svg class="icon color accent" …>` | **no** |
  | `<svg aria-label="pick a color here" …>` | **no** |
  | `<svg class="x xmlns y" …>` | yes, but **no `xmlns`** (usvg needs the namespace) |

- The icon then ignores the cascade: black where the page says gold, no
  `:hover` recolor. Middle words only (`class="color x"` and `"x color"`
  keep their quote).
- **Fix direction:** read the attributes with a scan that knows quotes
  (`normalize_self_closing` has one).

**K08 · low (perf) · Idle frames cost O(spans × distinct inline documents)** [probe]
- `rasterize_svg_images` asks `InlineSvgs::document_of` for every inline
  span every frame, before it looks at the span's `SvgRaster`; that locks
  the cache and scans all its entries (`svg.rs:925-935`, `:1182`).
  `finish_inline_svgs` also rebuilds a set of every `InlineImage` each
  frame.
- **Probe** (release, layout harness; N paragraphs each with its own
  inline `<svg>`, 100 idle frames, no build):

  | distinct documents | 100 frames | text-only page |
  |---|---|---|
  | 250 | 123 ms | 117 ms |
  | 500 | 140 ms | 121 ms |
  | 1 000 | 197 ms | 129 ms |
  | 2 000 | 384 ms | 144 ms |

  The extra is 0.05, 0.19, 0.68, 2.4 ms per frame. A page of per-row
  sparklines is the realistic case.
- **Fix direction:** a second map, image id → document; skip the lookup
  when the span's `SvgRaster::source` is still its image.

**K09 · low · A draw in flight outlives its document's reload** [code]
- On `AssetEvent::Modified` `rasterize_svg_images` drops the document's
  `ready` and `refused` entries but not `drawing`
  (`svg.rs:1140-1143`). A task started on the old tree finishes into
  `ready` under the same key, and the span shows the old drawing until
  its size changes. Hot reload and the `svg-text` font reload only.

### Selectors

**K10 · high (DoS) · Nested `:is()` with descendant combinators costs depth^levels** [probe]
- parcel's matcher is linear for a plain descendant chain: a failed
  walk to the root returns `NotMatchedGlobally` and ends the match. An
  `:is()` argument is matched by a call of its own, whose failure is
  local: the outer loop moves to the next ancestor and calls it again.
  Each level of `:is(… div) div` multiplies by the remaining depth.
- ADR 0034's allowlist admits descendant combinators inside `:is()`,
  `:where()` and `:not()`. The paren guard (`MAX_CSS_PAREN_NESTING`, 8)
  allows seven levels; `MAX_DOM_DEPTH` allows 100.
- **Probe** (release, `TestUi::new`, `settle()`; N nested `<div>`s around
  one `<p>`; the rule is `S p { color: red }` with `S` =
  `.never div` wrapped in `:is(…) div` L times):

  | depth | levels | CSS | HTML | settle |
  |---|---|---|---|---|
  | 80 | 2 | 68 B | 888 B | 8 ms |
  | 80 | 3 | 77 B | 888 B | 24 ms |
  | 80 | 4 | 86 B | 888 B | 266 ms |
  | 95 | 4 | 86 B | 1 053 B | 623 ms |
  | 95 | 5 | 95 B | 1 053 B | 9.3 s |
  | 95 | 6 | 104 B | 1 053 B | **120.9 s** |

  About 14× per level at depth 95; seven levels would be near half an
  hour. The build runs on the main thread: the app is frozen.
- The testbench's `selector-bombs` nests `:not()` six deep (the rule is
  dropped) and has no nested `:is()`.
- **Fix direction:** in `Selector::from_parsed`, allow a combinator
  inside at most one level of `:is()`/`:where()`/`:not()` (nested lists
  of compounds stay free), or bound levels × combinators. Either is a
  few lines in `nested`.

### Images

**K11 · medium · `%` image sizes are wrong at any scale factor but 1** [probe]
- `size_images` resolves a percentage against `block.size` minus the
  insets: `ComputedNode`'s **physical** pixels. The result goes into
  `InlineBox::size` and `InlineImage::width`/`height`, which are
  **logical** (`rasterize_svg_images` divides the same box by
  `inverse_scale_factor`). review_0006 J03 was this mix, in the windowed
  build.
- **Probe** (layout harness; `p { width: 200px } img { width: 50% }`,
  `frame.png` 32 × 24):

  | `UiScale` | inline box (logical) | expected |
  |---|---|---|
  | 1 | 100 × 75 | 100 × 75 |
  | 2 | **200 × 150** | 100 × 75 |
  | 0.5 | **50 × 37.5** | 100 × 75 |

  The same on a HiDPI display at `UiScale` 1. Every test of `%` images
  runs at scale 1.
- **Fix direction:** multiply `content` by `block.inverse_scale_factor`.

### `<canvas>`

**K12 · medium · A canvas with a CSS frame or background loses its picture, or its frame** [probe]
- An image canvas is an `ImageNode` on the element (`CanvasImage`). CSS
  `border-image`/`background-image` is an `ImageNode` on the same element
  (`CssFrame`). `sync_canvases` overwrites the frame when it binds;
  `update_spec` re-inserts the frame on every update (`ImageNode` has no
  `PartialEq`), and `sync_canvases` then sees `has_image` and an
  unchanged `showing`, so it doesn't bind again.
- **Probe** (`canvas.framed { border-image: url(frame.png) 4 stretch;
  border-width: 4px }`, `CanvasSource::Image`):

  | moment | the element's `ImageNode` |
  |---|---|
  | before binding | `frame.png`, sliced |
  | bound | the canvas image, stretched (the frame is gone) |
  | after an in-place update (a class change) | `frame.png`, sliced (the picture is gone; `showing` still says `Image`) |

- A framed portrait is the first thing the `canvas` example's reader
  will write. A camera canvas keeps its `ViewportNode` but shares the
  element with the frame, whose center Bevy always draws.
- **Fix direction:** show the source on a child node the canvas owns
  (the frame stays on the element), or document that a canvas takes no
  CSS image and drop the declaration with a warning.

**K13 · medium · Hiding a camera's only canvas resizes its image to 1 × 1** [probe]
- With no visible canvas, the first one in document order still owns
  the camera (`.unwrap_or(&list[0])`, `canvas.rs:282`) and keeps its
  `ViewportNode`. `display: none` lays it out at 0 × 0, and Bevy's
  `update_viewport_render_target_size` resizes to `size.max(UVec2::ONE)`.
- **Probe:** a 200 × 100 canvas, then `class='hidden'`
  (`display: none`): the image is `(200, 100)`, then `(1, 1)`;
  `shown("map")` is false.
- `HtmlCanvases::shown`'s docs offer the opposite: "A camera keeps
  rendering while nothing shows it; turn it off … where the last frame
  needn't be kept." The last frame is destroyed either way, and an app
  using the image elsewhere (a material, a minimap) gets one pixel.
- **Fix direction:** an invisible canvas owns nothing (`camera: None`
  while no candidate is visible).

**K14 · low · An oversized canvas keeps the camera from one that fits** [probe]
- The size gate runs after the owner is chosen. A canvas over
  `MAX_CANVAS_SIDE` that comes first in the document is the owner and
  shows nothing; a fitting canvas of the same camera shows the image
  stretched, and the camera is never resized.
- **Probe:** `<canvas width='5000' height='100'>` then `<canvas
  width='60' height='30'>`, one camera with an 8 × 8 image: the first
  shows `Nothing`, the second `Image`, the image stays 8 × 8.
- **Fix direction:** leave oversized canvases out of the candidates.

**K15 · low · A camera whose target image isn't in `Assets<Image>` panics in Bevy** [probe]
- Bevy's resize ends in `images.get_mut(image_handle).unwrap()`
  (`bevy_ui` `viewport.rs:188`). `sync_canvases` gates the size but not
  the image's presence.
- **Probe:** `CanvasSource::Camera` over a `RenderTarget::Image` of a
  reserved handle: `called Option::unwrap() on a None value` on the
  compute pool, the first frame.
- App code, not content: a target that loads later, or one removed while
  its canvas lives. Worth an UPSTREAM entry beside U33, and a check
  (`targets` + `Assets<Image>::contains`) before the `ViewportNode` goes
  on.
- Smaller, [code]: `warned` and `unregistered` (`Local` sets) never drop
  despawned entities or old names; the "canvas inside text" warning
  compares the raw tag name, so `<CANVAS>` skips it (`build.rs:6994`).

### Templates

**K16 · low · The limits don't say what they defend; template output has none** [probe]/[code]
- Tera caps component recursion (20) and `range` (100 000), not output:
  two nested loops over `range` in 92 bytes render n² characters.
  Probed linear in the output (1 200² = 1.44 MB: 62 ms to settle), so
  100 000² is 10 GB of `String`.
- Whether that matters depends on a decision the repository doesn't
  record. The crate bounds Tera operators (bug_0182), CSS nesting, DOM
  depth, grid tracks and SVG as if templates and stylesheets were
  hostile, and the fuzz targets enforce it; README promises it only for
  SVG files. A template is a program, and K10, K06 and this item are
  only findings if templates and CSS may come from mods.
- **Fix direction:** an ADR stating the threat model per input (FTL and
  SVG untrusted; CSS, templates: decide), then either a rendered-size
  cap or a sentence in the docs.

### Tests and records

**K17 · low (tests) · Gaps in the guards** [code]
- `tests/source_lint.rs` reads `src/*.rs` with `read_dir`, not
  recursively: `src/cascade/selector.rs` is never linted. A debug print
  or a file write there passes (review_0004 H05's class).
- SVG text is unfuzzed: `fuzzing` implies `svg`, not `svg-text`, and the
  harness draws without fonts. K01 is a one-token variant of bug_0200's
  document, which the property test found on its first run.
- `hostile_magnitudes_never_panic` and the calibration generate no
  `stroke-dasharray` (K02).
- No test lays out an inline image at a scale factor other than 1
  (K11); review_0006's J03 fix added a scale-2 camera to the harness for
  the windowed tests only.

**K18 · low (records) · Small disagreements** [code]
- AGENTS.md's Layout omits `src/md.rs`, `src/fuzz.rs`,
  `tests/source_lint.rs` and `tests/selector_model.rs`.
- WORKSPACE.md and AIWG.md name the repository `nchashch/bevy_markup`;
  `Cargo.toml` and the `origin` remote say `noeticbasho/bevy_markup`.
- `svg.rs`'s module docs and README say a costly inline document is
  drawn "on a background task"; its parse isn't (K05).

## Checked, no finding

- **Plain descendant and child chains** (`a b > c d …` over a deep
  document): linear by parcel's `NotMatchedGlobally` [code]; K10 needs
  the nested call.
- **The refusals themselves:** the depth and work bounds, the cycle
  check through inherited paint, the extent walk, the region check. I
  found no route past one by the thing it models.
- **`place_anchored`:** units (physical → logical through the element's
  `inverse_scale_factor`), the far-edge insets, the clamp, the subtree
  translation [code].
- **`pick_inline_boxes`:** it reads its own messages back, and skips
  them (a span has no `TextLayoutInfo`); no loop [code].
- **Tooltips over their own element** (a clamped tooltip under the
  pointer would despawn itself every other frame): the docs prescribe
  `pointer-events: none`, and the example has it.
- **`normalize_self_closing`:** linear, quote-aware; unquoted values
  keep their slash as HTML reads them [code].
- **Tera component recursion:** capped at 20 by Tera 2.4.
- **`sync_canvases` vs Bevy's resize:** ordered, with the sync point
  between them; the gate's units are Bevy's (physical).

## A note on the SVG approach

`UPSTREAM.md` holds 17 entries for the SVG stack (usvg, resvg, tiny-skia,
roxmltree, svgtypes; between U17 and U36), all from the few days of the
SVG work, most answered by one more refusal modelled on the renderer's
internals. K01–K04 are four more of the same
kind, found by reading for an afternoon. The bounds are well made, but
their job is to enumerate everything a renderer that was never hardened
can do with hostile input, and the list has no end that this crate
controls: a resvg patch release can move it.

Two cheaper lines of defence exist. An element allowlist for untrusted
documents (shapes, groups, gradients, transforms, `<use>`; no filters,
masks, patterns, markers, text, dashes) leaves almost nothing for the
cost model to get wrong, and icons — the stated use — need no more. And
`svg-text` could stay out of the untrusted path until text is in the
model. Worth an ADR before the next refusal.

## Triage

1. K01: an abort from a small file, in a shipped feature. One line
   (`text.flattened()` in `CostWalk`) closes it and K03.
2. K10: a frozen app from 104 bytes of CSS, if CSS is untrusted (K16
   decides).
3. K11, K12, K13: wrong output in ordinary use (HiDPI, a framed
   portrait, a hidden portrait).
4. K05, then K02, K04, K06: stalls and memory.
5. K16's ADR and the SVG note: both decide how much of 2 and 4 is owed.
6. The rest.
