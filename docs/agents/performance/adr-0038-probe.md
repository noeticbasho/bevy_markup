# ADR 0038 probe: loader conflicts, usvg's resolver, parser and raster limits

| Field | Value |
|---|---|
| Date | 2026-11-07 17:45 +0400 |
| Author | Claude Opus 5.5 (Anthropic), via Claude Code |
| Branch | `main` at `5bae780`; probes in an out-of-tree crate (not committed) |
| Machine | Arch Linux (kernel 7.2.14), AMD Ryzen 7 9800X3D (16 threads), rustc 1.98.1 |
| Versions | bevy 0.20.1 (`bevy_asset` 0.20.1), bevy_resvg 3.0.1, resvg/usvg 0.49.0 (`default-features = false`), roxmltree 0.21.2, tiny-skia 0.12.0 |

ADR 0038 (Proposed) left four claims marked `[INFERENCE]`. This report
checks each one empirically before any code is written.

## 1. Two `.svg` loaders in one app

**Setup.** A headless Bevy 0.20 app (`MinimalPlugins` + `AssetPlugin`)
with:

- a stand-in for our loader (`Asset = Image`, extension `svg`);
- bevy_resvg's real `SvgPlugin`, which registers a crate-private
  `SvgAssetLoader` (`Asset = SvgAsset`, extensions `svg`/`svgz`);
- a `.sheet` loader that calls `load_context.load::<Image>(path)`, as
  `style.rs` does.

It ran in four configurations: both registration orders, each with and
without a second `Image` loader (for `png`, standing in for Bevy's
`ImageLoader`).

| Load | Ours first | Theirs first |
|---|---|---|
| `load::<Image>("a.svg")` | ours | ours |
| `load::<SvgAsset>("b.svg")` | theirs | theirs |
| `load_context.load::<Image>` inside another loader | ours | ours |
| the same path as `Image` and as `SvgAsset` | both load, separately | both load, separately |
| `load_builder().load_untyped("c.svg")` | **theirs** (`SvgAsset`) | **ours** (`Image`) |
| `load::<Doc>("e.svg#doc")` (our labeled sub-asset) | Loaded | **Failed: `MissingLabel`** |

The extra `Image` loader changed nothing. No warning was logged in any
configuration.

**Why.** `bevy_asset` `server/loaders.rs` `AssetLoaders::find`:

- A typed load without a label narrows to the loaders of that asset type,
  then picks among them by extension.
- An untyped load, or any path **with a label**, skips the type and takes
  the *last registered* loader for the extension.

**Consequences.**

- Typed `Handle<Image>` loads are safe next to another `.svg` loader. The
  library makes only typed loads (no `load_untyped`, `load_erased` or
  `load_folder` in `src/` or `examples/`).
- A labeled sub-asset breaks whenever another `.svg` loader is registered
  after ours. The parsed document must be its own asset type behind its
  own loader (typed load, no label).
- An app's own untyped `.svg` loads depend on plugin order; that's
  Bevy's behavior and should be documented.
- bevy_svg 0.19 depends on `bevy ^0.19` (crates.io), so it can't share a
  Bevy 0.20 app today. bevy_vello 0.14 likewise depends on `bevy ^0.19`.

## 2. What usvg's default resolver reads

**Source.** `usvg` 0.49 `parser/image.rs`
`ImageHrefResolver::default_string_resolver` treats every `href` that
isn't a `data:` URL as a file path, joined with `resources_dir` when one
is set (`Options::get_abs_path`), and calls `std::fs::read` when it
exists. It embeds PNG, JPEG, GIF, WebP and **SVG** (the last parsed as a
nested tree). `default_data_resolver` embeds the same formats from
`data:` URLs.

**Measured.** Each case ran under `strace -f -e
trace=openat,connect,stat,statx,newfstatat` with default `Options` and
again with a resolver whose two closures return `None`:

| `href` / construct | Default resolver | Refusing resolver |
|---|---|---|
| absolute path outside any asset dir | `statx` + `openat` + read; PNG embedded | no syscall, no image |
| same, with `resources_dir` set | read anyway (an absolute path ignores the dir) | none |
| relative, `resources_dir = None` | read **relative to the process cwd** | none |
| `../../outside/private.png` with `resources_dir` | read: no containment | none |
| `xlink:href` (absolute) | read | none |
| `data:image/png;base64,…` | embedded | none |
| absolute path to an `.svg` | read, parsed, **rendered** (no `raster-images` needed) | none |
| `file:///…` | not resolved (`path.exists()` is false for a URL) | none |
| `http://127.0.0.1:9/x.png` | no `connect`, no image | none |
| `<use href="/abs/a.svg#r">` | nothing read | nothing read |
| `<style>@import url(/abs/x.css)</style>` | nothing read | nothing read |
| CSS `fill: url(/abs/x.png)` | nothing read | nothing read |
| `<?xml-stylesheet href="/abs/x.css"?>` | nothing read | nothing read |
| external entity `<!ENTITY x SYSTEM "file://…">` | parse error: unknown entity | same |

**Consequences.**

- An SVG from a mod or a user can read any file the process can, and
  have any SVG on disk drawn into the UI. With `raster-images` off,
  bitmaps are read but not drawn. Their bytes still sit in the tree.
- The refusing resolver closes every case observed. Only `<image href>`
  reads anything; `<use>`, CSS and processing instructions never do.

## 3. Parser limits

Each case ran under `timeout 30` and `prlimit --as=8589934592`. The
probe reports its own parse time and peak RSS (`VmHWM`); GNU `time`
confirmed the RSS.

| Input | Size | Result |
|---|---|---|
| billion laughs (9 levels × 10) | 680 B | parse error "possible entity reference loop" (roxmltree caps entity depth at 10, 255 refs each) |
| 1,000 nested `<g>` | 7 KB | parses, 0.7 ms |
| 1,025 nested `<g>` | 7 KB | `NodesLimitReached` (usvg `svgtree/parse.rs`: depth > 1024) |
| 100,000 nested `<g>` | 700 KB | `NodesLimitReached`, 5 ms, no stack overflow |
| 1,000,000 `<rect>` | 28 MB | `NodesLimitReached`, 0.16 s, 369 MB peak |
| `<use>` chain, 10⁷ expanded nodes | 1.5 KB | `NodesLimitReached`, 0.06 s, 75 MB |
| self-referencing `<use>` | 142 B | dropped, renders empty |
| mutually recursive `<use>` | 172 B | `NodesLimitReached` |
| 1,000,000 gradient stops | 37 MB | `NodesLimitReached` |
| 100,000-part selector in `<style>` | 200 KB | 1.8 ms |
| path with 10,000,000 segments | 60 MB | 0.87 s parse + 0.87 s raster, 935 MB |
| 2,000 `data:` SVG images, each 2,000 rects | 150 MB | 2.5 s parse, 2.1 GB |

**Consequences.** usvg's own limits stop structural bombs: depth 1024
and 1,000,000 nodes, counted after `<use>` expansion
(`svgtree/parse.rs` line 417). Memory stays roughly proportional to
input size, so the asset-size limits the app already chooses also bound
parsing. A pre-check like `nesting_over_limit` isn't needed.

## 4. Raster cost

| Input (≤ 45 KB each) | Raster | Time | Peak |
|---|---|---|---|
| `width="100000" height="100000"`, raster at intrinsic size | 10⁵ × 10⁵ | **process abort**: "memory allocation of 40000000000 bytes failed" (`Pixmap::new` aborts on the allocation instead of returning `None`) | — |
| same, rastered at 64 × 64 | 64² | 10 µs | 4 MB |
| `feGaussianBlur stdDeviation=100000`, filter region 20× the box | 2048² | 8.9 s | 905 MB |
| 1,000 chained `feGaussianBlur` | 1024² | > 30 s (killed) | 4.7 GB |
| `feTurbulence numOctaves=1000000` | 2048² | > 30 s (killed) | — |
| `feMorphology radius=100000` | 1024² | > 30 s (killed) | — |
| `feConvolveMatrix order=300` | 1024² | > 30 s (killed) | — |
| 1,000 nested `<g opacity>` | 1024² | 3.7 s | 22 MB |
| 1,000 nested `<g filter>` (`feOffset`) | 1024² | 14.7 s | 29 MB |
| 200 nested masks | 1024² | 2.0 s | 1.0 GB |
| 500 nested clip paths | 1024² | 1.9 s | 2.1 GB |
| patterns nested 8 deep / 0.001-px tile | 1000² / 4096² | 4 ms / 3 µs | small |
| `stroke-dasharray=0.0001` over 10⁹ px | 1000² | 6 µs | small |
| `filterUnits=userSpaceOnUse` region 2·10⁷ | 64² | 0.45 ms | small |

How the costly cases scale (raster side in px; time per raster):

| Parameter | Values → time |
|---|---|
| chained blurs at 256² | 10 → 26 ms, 100 → 242 ms (linear in count) |
| 10 chained blurs | 256² → 26 ms, 1024² → 405 ms (linear in area) |
| `stdDeviation` at 1024² | 10 → 61 ms, 1000 → 59 ms (flat: box-blur approximation) |
| `numOctaves` at 256² | 4 → 15 ms, 40 → 119 ms, 400 → 1.1 s (linear) |
| `feMorphology radius` at 256² | 5 → 5 ms, 50 → 312 ms, 500 → 3.6 s (≈ radius², easing toward linear) |
| `feConvolveMatrix order` at 256² | 3 → 4 ms, 9 → 16 ms, 27 → 122 ms (≈ order²) |
| nested `<g opacity>` | 10 → 37 ms, 100 → 366 ms at 1024² (linear in depth × area) |
| nested `<g filter>` at 1024² | 10 → 147 ms, 100 → 1.5 s |
| nested masks at 1024² | 10 → 110 ms, 50 → 505 ms |
| nested clip paths at 1024² | 10 → 45 ms, 100 → 379 ms |

**Consequences.**

- The pixel cap is necessary: an uncapped size aborts the process, not
  just the load.
- The pixel cap is **not sufficient**, which disproves ADR 0038's
  inference. A small file within the cap takes 2–15 s, or more than 30 s
  and gigabytes, through filter primitive count, `numOctaves`, the
  morphology radius, the convolution order, the filter region, and the
  nesting depth of isolated groups, masks and clip paths. Cost is close
  to pixels × (sum of per-primitive work) × nesting depth.
- Once started, a render can't be interrupted: `resvg::render` has no
  cancellation hook. So a budget has to be computed from the parsed tree
  before rendering.
- Bevy runs loaders on the `IoTaskPool` (`bevy_asset`
  `server/mod.rs`), so a slow SVG doesn't stall frames, but it holds an
  IO thread and delays other loads. Re-rasterizing for display size
  (ADR 0038 §3) must run off the main thread too.

## 5. bevy_resvg as the integration

From its 3.0.1 source:

- `SvgAssetLoader` is `pub(crate)` and produces `SvgAsset(Arc<Tree>)`,
  not `Image`.
- `rasterise` is `pub(crate)`.
- The public surface is `SvgPlugin` and the `Svg` / `UiSvg` /
  `SvgSize` / `SvgColor` components.
- It needs Bevy's `bevy_sprite_render` and `bevy_ui` features (a
  renderer dependency) and `resvg >=0.42, ^0.48` with default features
  (system fonts, raster decoders).
- Its loader parses with the options from its settings, whose
  resolver is usvg's default: `OptionsDef` has no resolver field and its
  conversion fills the rest with `..Default::default()`
  (`settings/options/macros.rs`; read in source, not run). An app using
  it inherits §2's file reads.

There's nothing to reuse behind the library's `Handle<Image>` paths, so
the ADR's own-loader decision stands.

## 6. Stack depth on a 2 MB thread (found while implementing step 1)

Bevy's task-pool threads, which run asset loaders, get the default 2 MB
stack (`bevy_tasks` `TaskPoolBuilder::stack_size` unset). The step-1 unit
tests run on 2 MB test threads and aborted with a stack overflow. Every
case below was located with gdb (`bt`, frames grouped by function) and
sized by bisection in a probe that parses on a 2 MB thread.

**roxmltree 0.21.1** (bevy_markup's lockfile; usvg accepts `^0.21.1`):
`tokenizer::parse_element` / `parse_content` recurse per nesting level,
~13.8 KB per level in debug (2.08 MB over 151 levels, from frame
addresses). 1,000 nested `<g>` overflowed. 0.21.2 (2026-10-24) parses
iteratively: fixed upstream, so the crate takes a version floor
(UPSTREAM U18).

**usvg 0.49 converter**, with roxmltree 0.21.2. It recurses per nested
element and per reference followed into a target's content
(`paint_server::convert_pattern` → `convert_children` → … →
`convert_path` → `paint_server::convert`). Overflow on a 2 MB thread,
parse stage (`Tree::from_str`) alone:

| Construct (each level references or nests the previous) | Debug | Release | Debug stack per level |
|---|---|---|---|
| `<g>` nesting | 689 | none below usvg's 1024 limit | 3.0 KB |
| nested `<svg>` | 370 | 704 | 5.7 KB |
| `<use>` chain | 251 | none up to 4096 (node limit first) | 8.4 KB |
| `<pattern>` filled with the previous | 121 | 333 | 17 KB |
| `<marker>` on a path in the previous | 123 | 336 | 17 KB |
| `<filter><feImage href>` | 113 | 440 | 18.5 KB |
| `<mask>` | 293 | 710 | 7.2 KB |
| `<clipPath clip-path>` | 984 | 1566 | 2.1 KB |
| `href` inheritance (pattern, gradient, filter) | none up to 65,536 | none | — |

The `<g>` case escaped §3: its doubling search went from 512 straight to
1024, where usvg's nesting check fails first. Upstream's PR #1223 (merged
2026-11-05, unreleased) makes that check configurable and counts `<use>`
instances, but not converter references (UPSTREAM U17).

**The check.** `svg::recursion_depth` runs on the roxmltree document
before usvg converts it (the same document: `Tree::from_xmltree`). It
takes the longest path over parent → child edges and element →
referenced-element edges:
- Edge weights: 1 per element, 2 per `<svg>`, +6 per reference.
- References are `url(#id)` or `#id` in any attribute, and in `<style>`
  rules matched by simplecss as usvg matches them.
- Cycles are refused.

The limit is 256. Calibration against the table: 3.0 KB per element, and
a pattern hop (path → reference → pattern → content, 8 units) costs
≤ 17 KB, so 256 units stays near 800 KB in debug.

**Verified.** `accepted_depths_fit_a_two_megabyte_stack` builds, for each
construct and for 24 pseudo-random mixes, the deepest document the check
accepts, and draws it on a 2 MB thread in the debug test build. Accepted
depths: `<g>` 253 levels, `<svg>` 126, `<use>`/pattern/mask/marker/CSS
mask 31, `feImage` 15, mixes 31–36.
- *Falsified:* with the limit raised to 4096 the test aborts with a stack
  overflow.
- *Headroom:* the same documents pass on a 1 MB stack and overflow at
  768 KB, so 2 MB leaves at least 2× in debug (release frames are about 3×
  smaller).

## 7. Cost-estimate calibration

`fuzz::svg_estimate` against `resvg::render` (best of three, release
build), 31 document families × 256², 1024², 2048² (the families of §4,
plus gradients, strokes, patterns, lighting, displacement, 20,000 small
paths, a 200,000-segment path). Time per estimated unit:

| | ns per unit |
|---|---|
| plain fill (overestimated: a solid rect is a fast path) | 0.02–0.08 |
| paths, clip chains, morphology, lighting | 0.1–1.2 |
| gradients, masks, opacity nests, blurs, convolution | 1.1–1.9 |
| drop shadow, displacement, turbulence | 2.0–2.7 |
| blur over a 20× filter region (clamped to 25× the canvas) | 3.7–3.8 |

The first run had morphology at 3.3–3.75 ns/unit for radius 40
(time grows faster than linearly in the radius). Its weight became
`2·rx·ry + 8·(rx + ry)`, and those rows now read 0.40–0.65.

At `MAX_SVG_COST` = 2²⁷ (134,217,728 units) the slowest accepted draw is
≈ 0.5 s on this machine (Ryzen 7 9800X3D), on an IO thread, not a frame.
Ordinary content stays far below the limit:
- a 1024² panel with a drop shadow: 36M units;
- a 4096² gradient: 50M units.

Debug builds draw roughly 10–50× slower.

The cargo-fuzz `svg` target draws within a 256 px side and a
proportional budget, and asserts each input finishes within 5 s: a miss
of the estimate by more than ~1000× fails it. First run: 144,475 inputs
in 180 s, no failures.

## 8. Panics in the renderer (step 2)

A step-2 test drew a 16 × 16 SVG with `feTurbulence numOctaves="100"`
and saw no image: resvg 0.49 panicked (`filter/turbulence.rs:234`,
`attempt to add with overflow`) on Bevy's IO thread. That's upstream
#1185 (UPSTREAM U20). Release builds wrap instead of panicking.

**Loaders are already safe.** `bevy_asset` 0.20.1 runs every loader under
`catch_unwind` (`server/mod.rs:1494`) and reports `AssetLoaderPanic`, so
the load fails rather than hanging. Removing bevy_markup's own guard
leaves `an_svg_that_panics_the_renderer_fails_its_load` passing, which
confirms this.

**Redraw tasks are not.** A probe spawned a panicking task on the
`AsyncComputeTaskPool` and polled it with `bevy::tasks::futures::check_ready`
under `catch_unwind`. The poller panicked: a task's panic propagates to
whoever polls it. Unguarded, a redraw would crash the app's main thread
inside `rasterize_svg_images`. `svg::guarded` wraps usvg's parse and
resvg's draw, so the task returns `SvgError::Panicked`.
`a_renderer_panic_is_an_error` fails without the guard (checked).
