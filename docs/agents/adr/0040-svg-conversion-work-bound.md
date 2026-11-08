# 40. Bound usvg's conversion work before it runs

| Field | Content |
|---|---|
| `ADR` | `0040` |
| `Title` | Bound usvg's conversion work before it runs |
| `Date` | 2026-11-08 15:51 +0400 |
| `Author` | Claude Opus 5.5 (Anthropic), via Claude Code |
| `Commit` | `ff99cc1 Explain swapped gamepad face buttons in the examples' docs` + uncommitted: this change |
| `Status` | Accepted |
| `Related` | ADR 0038 §5 (the safety limits this extends); bug_0188, bug_0189, bug_0190, bug_0191; UPSTREAM U17, U22, U23, U24, U25 |

## Context

ADR 0038 §5 put two bounds around an SVG load: a static recursion-depth
check before usvg converts the document (stack), and a render-cost estimate
from the converted `usvg::Tree` before resvg draws it (draw time). Nothing
bounded the conversion itself. usvg's only size limit counts the nodes of
its internal tree after `<use>` copies: a million.

The nightly `svg` fuzz run on 2026-11-08 (run 42471256114, on `ff99cc1`)
found an input that the loader refused, correctly, as too expensive to
draw, but only after 8.8 s on the CI runner (bug_0188). Stage timings of
that input (ASan build, local): depth check 0.5 ms, usvg conversion 1.7 s,
cost estimate 21 ms. The input is a `<use>` fan-out: ten `<use>`s per
level, about 10⁵ rects from 1.6 KB, under usvg's million-node limit.
Conversion is linear in what it builds (about 18 µs per node under ASan),
so the cure is a bound on what it builds.

Reading usvg 0.49, svgtypes 0.16 and kurbo 0.13 found three more ways for a
small document to make the conversion unbounded, each confirmed with a
generated input:

- **Markers** (bug_0189). `marker::convert` converts the marker's whole
  content once per vertex of each path that carries it. Marker properties
  are inherited (`find_attribute`), and markers nest (only re-entering the
  same marker is blocked). A 44 KB file, a 1000-rect marker on a
  4000-vertex path, didn't finish in 120 s.
- **Arcs** (bug_0190). svgtypes' `SimplifyingPathParser` splits an
  elliptical arc with kurbo's `to_cubic_beziers(0.1, …)`. The tolerance is
  an absolute 0.1 user units, so the cubic count grows as `(11.16·r)^⅙`.
  The simplifier buffers all of them and drains the buffer with
  `Vec::remove(0)`, which is quadratic. One 40-byte arc (`A1e30 1e30 0 1 1
  1e30 0`) took 4.3 s; at `r = 3e38` it ran past 100 s. Circles, ellipses
  and rounded rects go through tiny-skia's `arc_to` (linear, no buffer),
  and `r = 3e38` still cost 0.3 s per shape.
- **Inherited paint servers.** An `objectBoundingBox` pattern is cloned per
  shape it paints, and a gradient's stops are copied per shape. Through an
  inherited `fill`, one reference reaches every shape below it. Probing
  this found bug_0191: a pattern whose own content inherits a `fill`
  naming the pattern makes usvg recurse until the stack overflows, which
  aborts the process. The depth check never saw it, because an inherited
  reference wasn't an edge of its graph.

## Decision

### 1. One model, before usvg runs: `svg_bounds::bounds`

The depth check moves from `svg.rs` into `src/svg_bounds.rs` and becomes
`bounds(document, max_references) -> Bounds { depth, work }`. It builds one
graph over the document's elements, as before (parent → child, element →
referenced element, `<style>` rules matched by simplecss as usvg matches
them). Then it adds what was missing:

- **Inherited references are edges.** A topological walk carries each
  element's inherited paint servers and markers (`linearGradient`,
  `radialGradient`, `pattern`, `marker` named anywhere in its own
  declarations, unioned with its parent's) down the inheritance edges:
  children, and a `<use>`'s referenced content, which usvg copies under the
  `<use>`. Each shape and text element gets an edge to each one. The cycle
  check then refuses a pattern or marker that inherits itself (bug_0191),
  and the depth counts the hop.
- **Instances.** A second topological walk counts each element's instances:
  the number of paths to it from the root, each edge weighted by how many
  times it instantiates its target. A child or reference edge is 1 (a
  `<use>` copy; an `objectBoundingBox` mask, clip path or filter converted
  per element). An edge from a shape to a marker is the shape's segments
  + 2, for its start, every vertex and its end. An edge to a paint server
  is 0: usvg converts the server once and caches it, and a shape clones
  the server's content shallowly (nested servers stay shared `Arc`s). So
  the shape's own work gains twice the server's content instead, for fill
  and stroke. Counting the server transitively made 32 nested patterns
  look like 3³² work; reality is linear.
- **Work** is Σ instances × own work. An element is 16 units, a gradient
  stop 1, a path segment 4, a byte of text 16, and each path arc adds
  (cubics)²/64 for the simplifier's drain.

Segments are counted, never generated:
- `d` is walked with svgtypes' raw `PathParser`, the parser the simplifier
  runs on, tracking the current point as the simplifier does. Each arc's
  cubics are computed with kurbo's own arithmetic (`from_svg_arc`, then
  `append_iter`'s subdivision count), with no generation.
- Rect, circle and ellipse radii resolve as usvg's `convert_length`
  resolves them: absolute units at 96 dpi, `em`/`ex` against the inherited
  `font-size` (default 12), `%` against the largest inherited viewport
  side (default 100). Each is a maximum over the declarations that could
  apply, so the result can only be too large.

One deviation from exact counting is deliberate. The simplifier starts
each segment where kurbo's last cubic ended, a rounding error away from the
exact endpoint. So an arc whose endpoints coincide counts a whole turn, and
a count landing on a whole number counts one more.
`path_segments_match_the_simplifier` checks the result against svgtypes run
for real: never fewer, at most one more for a single arc.

### 2. `MAX_SVG_WORK`, checked in `svg::parse_within`

`parse_unguarded` checks depth, then work, then converts. A document over
`MAX_SVG_WORK` (2²³) fails with `SvgError::TooLarge { work, max }`.
`Limits` gains `work`, so `rasterize_with` parses within `limits.work`.
The fuzz harness uses `SVG_FUZZ_WORK` (1/16 of the limit), as it already
scales the render cost: the target times each input, so a slow input means
the model missed a cost. `svgtypes` becomes a direct dependency of the
`svg` feature. It's the version usvg resolves: one copy in `Cargo.lock`,
checked by the lockfile test that already covers simplecss and roxmltree.

### 3. Calibration

Release build, without ASan (cargo-fuzz `-s none`): each class of input,
work vs real conversion time.

| input | work | conversion | ns per unit |
|---|---|---|---|
| `<use>` fan-out, 10⁵ rects | 15.1M | 716 ms | 47 |
| one path, 2·10⁵ line segments | 0.80M | 48 ms | 60 |
| 20 circles, `r = 1e30` | 12.0M | 219 ms | 18 |
| one path, 40 arcs at `r = 1e27` (quadratic drain) | 976M | 11.7 s | 12 |
| 20-rect marker × 5000 vertices | 3.7M | 158 ms | 43 |
| inherited 20-rect pattern × 2000 shapes | 3.0M | 12 ms | 4 |
| inherited 50-stop gradient × 5000 shapes | 0.84M | 19 ms | 23 |
| 100 KB of text in 1000 `tspan`s (no `svg-text`) | 1.6M | 3.3 ms | 2 |

The nightly's input comes to 18.9M, refused at about 2.3× the limit.

The first calibration counted a segment as 1 unit, which made line paths
240 ns per unit against an element's 54. A segment is now 4. One unit is
then at most ~60 ns, so `MAX_SVG_WORK` = 2²³ allows ~0.5 s of conversion in
release, like `MAX_SVG_COST`'s ~0.5 s of drawing. Text is weighted
blindly: this build has no `svg-text`, and shaping is costlier than
anything measured here.

What fits: `ordinary_documents_fit_the_work_budget` checks a map (200
gradient-filled paths of 200 segments), a diagram (1000 arrow-marked
lines and curves), 2000 `<use>` icons and 1000 rounded shapes with `%`/`em`
radii, each under a quarter of the limit. Of the table's inputs, the
fan-out, the circles, the arcs and the nightly's input are refused; the
check itself took 0.1–0.7 ms on them, and 24 ms on the 2·10⁵-segment path
(release).

## Alternatives considered

- **Bounding only `<use>` copies.** It fixes the fuzz find and leaves
  markers (worse per byte) and arcs (one 40-byte arc hung) open, along with
  the self-inheriting pattern, which aborts the process.
- **Counting segments by running svgtypes' simplifier with an early exit.**
  Exact, but one `next()` generates a whole arc into the buffer. That's
  millions of cubics, or an unbounded loop where kurbo's count is
  infinite, before the first one returns.
- **Refusing large numbers outright** (any coordinate or radius over some
  magnitude). Simple, but `em` and `%` chains multiply past any per-value
  limit, and it refuses legitimate large coordinate spaces whose arcs are
  cheap.
- **Time-boxing the conversion.** usvg's conversion can't be interrupted
  (no cancellation hook), and a thread that's abandoned keeps the CPU.
- **Treating every shape as carrying every marker or paint server** (no
  inheritance walk). Much simpler, but it refuses a 10⁵-segment map that
  happens to share a document with one small arrowhead marker.
- **Modelling paint servers like markers (transitive instances).** Simpler,
  but exponential in nested patterns, where usvg's work is linear (it
  failed the depth tests' 32 nested patterns as 3³² work).

## Consequences

- A document whose conversion would take more than ~0.5 s in release fails
  to load with an error naming the estimate, instead of stalling a loader
  thread for seconds, minutes or forever. One that would recurse through
  an inherited self-reference fails as a cycle, instead of aborting.
- Over-approximation costs: a cached gradient counts per shape, an
  inherited property counts even where a shape overrides it, and a marker
  inheriting itself is refused although usvg skips it (as the CSS spelling
  `path { marker-end: url(#a) }` already was, through an explicit edge).
- The check is linear in elements and references, and the inheritance walk
  stores at most `MAX_REFERENCES` inherited entries.
- The model is tied to usvg 0.49, svgtypes 0.16 and kurbo 0.13: the arc
  formula, the simplifier's tracking and what usvg caches. A resvg bump
  needs `path_segments_match_the_simplifier` (which compares against the
  real simplifier) and the calibration table rechecked.
- Open: tiny-skia 0.12's scan converter has a `debug_assert!` that fails on
  huge coordinates (UPSTREAM U24). Release builds skip it, `svg::guarded`
  turns it into an error in debug builds, and it remains reachable by
  fuzzing (like U20).
