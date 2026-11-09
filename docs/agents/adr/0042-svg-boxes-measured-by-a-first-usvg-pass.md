# 42. usvg's object bounding boxes are measured by a first conversion

| Field | Content |
|---|---|
| `ADR` | `0042` |
| `Title` | usvg's object bounding boxes are measured by a first conversion |
| `Date` | 2026-11-09 16:00 +0400 |
| `Author` | Claude Opus 5.5 (Anthropic), via Claude Code; the approach is the user's ("call the `usvg` code for the bounding box and check it … before applying any filters") |
| `Commit` | `8184761 Fix bug 196: refuse feMorphology radii that overflow f32; file bug 197` + uncommitted: this change |
| `Status` | Accepted |
| `Related` | ADR 0038 (SVG images), ADR 0040 (bounding usvg's work before it runs); bug_0196, bug_0197; UPSTREAM U30, U31 |

## Context

usvg 0.49 maps every `objectBoundingBox` region onto the element's object
bounding box with tiny-skia-path 0.12's `NonZeroRect::bbox_transform`,
which multiplies and unwraps `NonZeroRect::from_xywh` (UPSTREAM U31). Five
call sites: a `filter` element's region (`parser/filter.rs:227`, default
−10%/120%), a CSS filter function's region (`:72`, always), `feImage` and
primitive subregions (`:411`, `:428`, the latter multiplying the filter
region by the primitive's fractions), a mask region (`parser/mask.rs:55`)
and a pattern tile (`parser/paint_server.rs:966`). `feMorphology` unwraps
`radius × box` the same way (U30). A finite box whose product overflows
`f32` panics in every build: `<rect width='3e38' height='8'
filter='url(#f)'/>` with any filter (bug_0197). `svg::guarded` makes that a
failed load; the `svg` fuzz target aborts.

The box is `Group::calculate_object_bbox` over the already converted
children (`converter.rs:859`), computed right before the clip path, mask
and filter conversions that use it. It depends on the geometry (paths, arcs,
text outlines through fonts, `<use>` copies, markers placed per vertex and
scaled by stroke width, nested viewports) and on every transform below the
element: `scale(1e20)` in a filtered group grows the box without any large
number. ADR 0040's bounds run before usvg on the XML; none of them tracks
coordinates.

## Decision

1. **usvg computes the boxes, in a first conversion that can't reach
   `bbox_transform`** (`src/svg_boxes.rs` `largest_box`). It converts a
   rewritten copy of the document with the same `usvg::Options` and reads
   every group's `bounding_box()` (subroots included); the largest
   coordinate magnitude is the measure. The copy:
   - inlines each element's CSS into its `style` attribute in usvg's order
     (`<style>` rules sorted by specificity, matched with usvg's
     `XmlNode`, then the element's own `style`), and drops `<style>`;
   - starts each `style` with `filter`, `mask`, `clip-path`: `none`,
     `fill: #000`, `stroke: none`, all `!important` (usvg keeps the first
     important declaration, `svgtree/parse.rs`): no filter, mask or pattern
     is converted, and no shape is dropped for a fill naming a pattern that
     isn't converted;
   - renames `defs`, `mask`, `clipPath` and `pattern` to `g` (with
     `display: inline !important`), so content usvg converts only through a
     reference is converted in place. A group's object box doesn't include
     the transforms above it, so the place doesn't matter;
   - keeps SVG-namespace elements and SVG/XLink/XML attributes only, as
     usvg does, without the DTD (entities are already expanded).

   It converts a superset of the real document's elements. A group usvg
   doesn't keep in the copy (its filter gone, no transform) folds into its
   parent's box in the same coordinates.
2. **The factors come from the XML** (`svg_bounds` `Bounds::regions`): the
   largest `x`, `y`, `width`, `height` of any filter, mask, pattern or
   primitive, and any morphology radius, as a px length (`%` against the
   inherited viewport, which is never less than the 1/100 a box unit
   uses), at least 2 (a filter function's region). None of them is a
   presentation attribute, so CSS can't set them.
3. **`svg::check_regions`** refuses with `SvgError::RegionOverflow` when
   `(regions + 1) × 2 × largest + regions` (a region after
   `bbox_transform`, or in user space), times `regions + 1` again (a
   subregion), exceeds `f32::MAX / 2`.
4. **Only when needed:** the first pass runs when the document has a
   `filter`, `mask` or `pattern` element or a `filter` declaration
   (`Bounds::boxes`). Gradients use `Transform::from_bbox`, which doesn't
   unwrap.
5. bug_0196's document-wide rule (a radius above 1 whenever any filter
   uses `objectBoundingBox` primitive units) is dropped: the radius is now a
   factor. `check_values` still refuses a radius that isn't finite as
   `f32`.

## Alternatives considered

- **Mirror usvg's geometry in `svg_bounds`** (an upper bound per element:
  path extents, arc radii, shape lengths, text as font size × length,
  `<use>` and viewport transforms, markers × stroke width, multiplied up
  the tree). Coarse is fine for safety, but it reimplements usvg's
  conversion rules (units, text layout, marker placement, `switch`) and
  drifts from them silently. The user preferred usvg's own numbers.
- **Inject a user stylesheet** (`Options::style_sheet`) instead of
  rewriting. It neutralizes filters, masks and paints without touching the
  XML, but it can't make usvg convert mask, clip-path and pattern content,
  where a filter of its own can panic the same way (the test's mask and
  pattern cases). Renaming needs the copy anyway; with a copy, inlined CSS
  keeps selectors that name the renamed tags (`mask g g`) working.
- **Rewrite the units attributes** (`filterUnits` etc. to
  `userSpaceOnUse`) in a copy and keep everything else. The defaults are
  `objectBoundingBox`, so every element needs an attribute added, and a
  region in user units changes which elements usvg keeps (a near-zero
  region drops one), which can hide a box.
- **Cap every number in the document.** Transforms multiply across
  nesting, so a cap on numbers doesn't bound products.
- **Leave it to `svg::guarded`.** A failed load either way for the app, but
  the fuzz target aborts on it, and U31 panics in release builds too.

## Consequences

- Each of the ten test inputs in `svg::tests::regions_over_huge_boxes_are_refused`
  panics usvg when converted directly, and is refused. They cover the five call sites plus a box grown by
  `scale(1e20)`, filters inside mask and pattern content, and a `<style>`
  transform reaching renamed content. Disabling the CSS inlining, or the
  renaming, fails the test. A drawing scaled down by a `viewBox` of 10⁷
  still draws.
- Cost: documents with a filter, mask or pattern convert twice, plus a
  serialization. `benches/svg-divan.rs` `parse` (release, median of 100):
  `shadowed` 4.4 → 11.4 µs, `deep` 60 → 161 µs; `icon` and `illustration`
  (none of those) unchanged. The first pass converts at most what the work
  bound (ADR 0040) already allows, plus each `<defs>` element once.
- Over-refusal: a box within ~1/20 of `f32::MAX` with ordinary factors, or
  factors whose worst case overflows although the actual products don't.
  Nobody draws such documents.
- Open: the first pass trusts that renaming doesn't remove what the real
  conversion keeps. A mask's or pattern's own attributes that usvg reads on
  `g` differently (none known) would show up as a missed box, which
  `svg::guarded` still catches. Drop the pass once usvg checks (U31's PR
  sketch).
