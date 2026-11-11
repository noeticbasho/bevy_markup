# Upstream issues

Defects in dependencies and tools that bevy_markup works around locally, tracked so
they can be reported upstream (issue and/or pull request) and the local
workarounds removed once upstream releases a fix. bevy_markup's own bugs are in
`INDEX.md`; entries here link to a bug report when one exists.

Status per entry: **unreported** → **reported** (issue link) → **PR open**
(link) → **fixed upstream** (version) → **workaround removed** (commit).
Before reporting, re-check the latest upstream release and default branch:
the issue may already be fixed or reported.

## Summary

| # | Project | Issue | Impact on bevy_markup | Local workaround | Status |
|---|---|---|---|---|---|
| U1 | fluent-syntax (fluent-rs) | Invalid unicode escape before a multi-byte character panics ([bug_0005](bug_0005.md)) | panic from any FTL asset | `ftl_check::bad_escape` at load (ADR 0045); the fork no longer wired in | fix on fork; upstream PR pending |
| U2 | fluent-syntax (fluent-rs) | Unbounded expression nesting overflows the stack ([bug_0015](bug_0015.md)) | stack overflow (abort) from a few KB of FTL | `ftl_check` nesting limit at load (ADR 0045); the fork no longer wired in | fix on fork; upstream PR pending |
| U3 | honggfuzz-rs | Bundled honggfuzz doesn't build against current binutils | honggfuzz driver doesn't build | vendored patch | unreported |
| U4 | fuzzcheck | Coverage sensor misreads LLVM 21+ `__llvm_prf_data` | fuzzcheck driver gets no usable coverage | vendored patch | unreported |
| U5 | fuzzcheck | `observing_only_files_from_current_dir` drops absolute paths | fuzzcheck observes no project code | vendored patch | unreported |
| U6 | fuzzcheck | `fuzzcheck_mutators_derive` 0.13.0 uses let-chains with edition 2021 | fuzzcheck driver doesn't build | vendored, edition bumped | unreported |
| U7 | cargo-mutants | `--re` / `--exclude-re` don't filter struct-field-deletion mutants | targeted reruns run unrelated mutants (~5× slower) | none (documented) | unreported |
| U8 | cargo-fuzz | Prebuilt (musl) binary defaults `--target` to musl, which ASan rejects | CI fuzz jobs failed to build | `--target "$host"` in `scripts/fuzz-libfuzzer.sh` | unreported |
| U9 | Bevy 0.19 | `UiPlugin`'s `viewport_picking` panics without the picking plugins | headless UI layout tests need extra plugins | add `DefaultPickingPlugins` in `tests/common` | discuss first |
| U10 | lightningcss | Bare `grid-auto-flow: dense` fails to parse | the declaration is dropped (CSS means `row dense`) | none (documented; write `row dense`) | unreported |
| U11 | tl | A value-less attribute eats the next attribute's first character ([bug_0019](bug_0019.md)) | `autofocus data-on-click` lost the click hook and focusability | switched to `astral-tl` 0.8.0 (astral-sh's maintained fork, fixed there) | fixed in astral-tl; y21/tl 0.7.8 affected |
| U12 | bevy_picking / bevy_ui | Text-section hits resolve `Pickable` on the span entity, not the text node ([bug_0020](bug_0020.md)) | `pointer-events: none` on a text-bearing element leaked clicks | `Pickable::IGNORE` on the spans too (`src/build.rs`) | unreported |
| U13 | pulldown-cmark | The offset iterator panics on a list item holding only a link reference definition before a whitespace-only line ([bug_0178](bug_0178.md)) | panic from any `.md` template | minimum version raised to 0.13.5 (`Cargo.toml`) | fixed upstream (0.13.5, #1021); a residual un-hardened `unwrap()` in `OffsetIter` is latent only (not reachable in 0.13.5 per ~18M fuzz execs + instrumented replay), left unreported |
| U14 | lightningcss | Nested `light-dark(` and `calc(` around a `var()` parse in exponential time ([bug_0179](bug_0179.md)) | a few hundred bytes of CSS stall the load for minutes to hours | nesting pre-scan bounds `(…)` at 8 (`MAX_CSS_PAREN_NESTING`) | reported upstream by others (#1382, #1389); PR #1383 open for #1382 |
| U15 | lightningcss | The parser recurses once per block with no bound ([bug_0092](bug_0092.md), [bug_0179](bug_0179.md)) | stack overflow (abort) from a few KB of nested blocks | nesting pre-scan bounds all blocks at 32 (`MAX_CSS_NESTING`) | reported upstream by others (#1297, #1155, #1092) |
| U16 | tera | The compiler recurses once per operator of an expression; the 2.4.0 cap (256 operations, Keats/tera#1035) still overflows a 2 MB thread in debug builds from 136 ([bug_0182](bug_0182.md)) | stack overflow (abort) at template load in unoptimized builds | a tag over 64 operators fails the load before Tera compiles it (`tera_tag_over_limit`, `src/template.rs`) | unreported (follow-up to the closed #1035) |
| U17 | usvg (resvg) | Converter recursion through references and nesting is unbounded | stack overflow (abort) from a few KB of SVG on a 2 MB thread (`svg` feature) | static recursion-depth check before conversion (`svg_bounds::bounds`) | unreported; resvg #1223 (unreleased) bounds XML nesting and `<use>` only |
| U18 | roxmltree | 0.21.1's tokenizer recursed once per nesting level | stack overflow at ~150 levels in debug on a 2 MB thread (`svg` feature) | version floor `roxmltree >= 0.21.2` | fixed upstream (0.21.2) |
| U19 | usvg (resvg) | The default `ImageHrefResolver` reads any file path | an SVG reads any file the process can and embeds it (`svg` feature) | a resolver that resolves nothing | unreported (by design; worth a docs warning) |
| U20 | resvg | `feTurbulence` overflows `i32` after enough octaves or on a huge `baseFrequency` (#1185, open; #937 a similar multiply overflow) | panic where overflow checks are on (debug builds, fuzzing): a failed load, or a crash through a redraw task (`svg` feature) | `svg::guarded` (catch_unwind around usvg/resvg); the `svg` fuzz target lets it unwind (`fuzz::is_known_upstream_debug_panic`) | reported upstream (#1185), open; the `baseFrequency` trigger unreported |
| U21 | astral-tl | `/` is a name character: `<b/>` is an unclosed element named `b/` ([bug_0185](bug_0185.md)) | a `<br/>` merged the rest of the document into one block | `template::normalize_self_closing` before every `tl` parse | unreported |
| U22 | svgtypes / kurbo | An arc splits into `(11·r)^⅙` cubics at kurbo's absolute 0.1 tolerance, and svgtypes' simplifier drains them with `Vec::remove(0)` ([bug_0190](bug_0190.md)) | one 40-byte arc converts for seconds or never finishes (`svg` feature) | counted before conversion in the work bound (`svg_bounds::bounds`, ADR 0040) | unreported |
| U23 | usvg (resvg) | A pattern whose content inherits a paint naming itself recurses until the stack overflows ([bug_0191](bug_0191.md)) | stack overflow (abort) from 230 bytes (`svg` feature) | inherited references are edges of the cycle check (`svg_bounds::bounds`) | unreported |
| U24 | tiny-skia | Anti-aliased fills shift the path's *unclipped* rows by 2 in an `i32`, which wraps past 2²⁹ ([bug_0194](bug_0194.md)) | panic in any build on paths far off the canvas (debug: `walk_edges` assertion; release: U28) — a failed load (`svg` feature) | refused before drawing (`svg::check_extent`, `MAX_SVG_EXTENT`, `SvgError::TooFarOut`) | unreported |
| U25 | usvg (resvg) | No bound on conversion work: `<use>` copies under the million-node limit, markers per vertex ([bug_0188](bug_0188.md), [bug_0189](bug_0189.md)) | seconds to minutes of conversion from a few KB (`svg` feature) | the work bound (`svg_bounds::bounds`, `MAX_SVG_WORK`, ADR 0040) | unreported (by design; a conversion budget option would replace the model) |
| U26 | usvg (resvg) | `feTurbulence` with a `baseFrequency` past `f32::MAX` unwraps `PositiveF32::new(inf)` ([bug_0192](bug_0192.md)) | panic in any build: a failed load (`svg` feature) | refused before usvg runs (`svg_bounds::check_values`, `SvgError::OutOfRange`) | unreported |
| U27 | tiny-skia | The cubic edge clipper's bisection can't reach its 0.25 px tolerance between ~2²¹ and 2²² px and unwraps `NormalizedF32Exclusive::new(1.0)` ([bug_0194](bug_0194.md)) | panic in any build: a failed load (`svg` feature) | refused before drawing (`svg::check_extent`) | unreported |
| U28 | tiny-skia | Release builds of U24: the wrapped row range writes outside the canvas, caught by an `unwrap` in `alpha_runs.rs` or a slice in `pipeline/mod.rs` ([bug_0194](bug_0194.md)) | panic in release builds: a failed load (`svg` feature) | as U24 | unreported (report with U24) |
| U29 | usvg (resvg) | `feConvolveMatrix` multiplies its `order` sides as `u32` unchecked ([bug_0195](bug_0195.md)) | panic where overflow checks are on (debug builds, fuzzing): a failed load; release wraps (the cost estimate then refuses the kernel) (`svg` feature) | refused before usvg runs (`svg_bounds::check_values`, `SvgError::OutOfRange`) | unreported |
| U30 | usvg (resvg) | `feMorphology` unwraps `PositiveF32::new(radius × scale)`, `None` when the product isn't finite ([bug_0196](bug_0196.md)) | panic in any build: a failed load (`svg` feature) | non-finite radii refused before usvg runs (`svg_bounds::check_values`, `SvgError::OutOfRange`); radius × box by ADR 0042 | unreported |
| U31 | tiny-skia-path (via usvg) | `NonZeroRect::bbox_transform` unwraps `from_xywh` of an overflowed product ([bug_0197](bug_0197.md)) | panic in any build on a filter over a box wider than ~2.8·10³⁸: a failed load (`svg` feature) | boxes measured by a first usvg pass, overflowing regions refused (ADR 0042, `svg_boxes`, `SvgError::RegionOverflow`) | unreported |
| U32 | resvg | The box blur adds 2 to an `i32` box width that saturates at `i32::MAX` for a deviation of ~1.4·10⁹ px ([bug_0198](bug_0198.md)) | panic where overflow checks are on (debug builds, fuzzing): a failed draw; release wraps to a negative box (`svg` feature) | refused before drawing past `MAX_SVG_BLUR` (`svg::check_extent`, `SvgError::TooBlurred`) | unreported |
| U33 | Bevy (bevy_picking 0.20) | `Location::is_in_viewport` returns false without a `PrimaryWindow`, even for an image render target: ray-based picking (mesh, sprite) hits nothing in a windowless app | any render-to-texture-only app loses mesh picking (no bevy_markup code affected); the examples' `--offscreen` mode did | the harness's `--offscreen` mode spawns a primary `Window` entity with no native window (`examples/shared/harness.rs`) | unreported |
| U34 | Bevy (bevy_ui 0.20) | The UI picking backend hit-tests a text node's runs (`run_geometry`) but never `TextLayoutInfo::inline_boxes`: a pointer over an inline image hits its text block ([bug_0199](bug_0199.md)) | an image or SVG inside a link can't be clicked through the icon | `signals::pick_inline_boxes` adds the image's hit from the UI backend's hit on its text block | unreported |
| U35 | tiny-skia-path 0.12 (via resvg 0.49) | `NonZeroRect::to_int_rect` and `IntSize::to_int_rect(x, y)` unwrap `IntRect::from_xywh`, `None` once `x + width` passes `i32::MAX`; resvg converts filter regions, subregions and offsets with them ([bug_0201](bug_0201.md)) | panic in any build: a failed draw (`svg` feature) | regions, subregions and offsets past `MAX_SVG_FILTER_REGION` refused before drawing (`svg::check_extent`) | unreported |
| U36 | resvg 0.49 (tiny-skia 0.12) | `render_pattern_pixmap` allocates a pattern's tile at its drawn size with no limit; `Pixmap::new` allocates with `vec![0; len]`, which aborts on failure ([bug_0200](bug_0200.md)) | process abort (`svg` feature) | the render-cost estimate charges each tile (`svg::estimate_cost`) | unreported |
| U37 | parley 0.11 (via bevy_text 0.20) | An inline box's whole height counts as ascent and the line height is the tallest item's, so a box taller than the text's ascent gets negative leading ([bug_0203](bug_0203.md)) | an `<img>`/inline `<svg>` taller than the ascent overlaps the line above; the line is short by the text's descent | none | fixed upstream (parley 0.12.0); waiting for Bevy to depend on it |
| U38 | Bevy (bevy_ui 0.20.1) | `update_viewport_render_target_size` ends in `images.get_mut(image_handle).unwrap()`: a `ViewportNode` camera whose `RenderTarget::Image` isn't in `Assets<Image>` panics ([bug_0218](bug_0218.md)) | a canvas camera over a not-yet-loaded or removed target image panics on the compute pool | `sync_canvases` binds a camera only while its target image is in `Assets<Image>` (bug_0218) | unreported |
| U39 | Tera 2.4.0 | A render's work and memory have no limit (only `range` ≤ 10⁵ and component recursion ≤ 20): nested loops run for hours, `set_global` doubling in a loop or unrolled `set` doubling ask for terabytes and abort ([bug_0220](bug_0220.md)) | a template stalls or aborts the app | `tera_check`: a static check at load bounds steps, built values and copies of context data (ADR 0044); output capped by `template::MAX_RENDERED_BYTES` | unreported |
| U40 | simplecss 0.2.2 (via usvg 0.49 and bevy_markup's `svg_bounds`) | `Selector::matches` tries every ancestor for a descendant combinator and recurses with no memo: a selector with k descendant combinators costs up to Σ_{j≤k} C(depth, j) calls per element when its leftmost compound never matches ([bug_0229](bug_0229.md)) | a 555-byte SVG `<style>` parses for 36 s (`svg` feature) | `svg_bounds::match_work` charges every rule by that count (and its class/attribute tests) before anything matches | unreported |
| U41 | usvg 0.49 text layout (`text` feature) | `ByteIndex::code_point_at` counts characters from the chunk's start, and `resolve_relative_shifts` (and the rotate and text-on-path code) calls it per cluster: every text chunk lays out in time quadratic in its characters ([bug_0232](bug_0232.md)) | a 100 000-character `<text>` converts for 3.9 s (13.5 s with multi-byte characters) inside `MAX_SVG_WORK` (`svg-text`) | `svg_bounds` charges a `<text>` by its characters² | unreported |
| U42 | simplecss 0.2.2 (via usvg 0.49 and bevy_markup's `svg_bounds`) | `StyleSheet::parse_more` builds an error, positioned by counting lines from the text's start (`Stream::gen_text_pos`), at every declaration's end, unitless number and `#` colour, then drops it: parsing is quadratic in the sheet's length ([bug_0231](bug_0231.md)) | 40 000 rules (760 KB) parse for 0.96 s per pass, three passes per SVG (`svg` feature) | `svg_bounds::style_scan_bytes` charges the offsets of those places before simplecss parses | unreported |
| U43 | usvg 0.49 text layout (`text` feature) | `process_chunk` reshapes the whole chunk once per span and merges each span's glyphs into the first run with `glyphs.iter().position(…)` and `Vec::remove`/`insert`: spans × characters ([bug_0232](bug_0232.md)) | 4 000 one-character `<tspan>`s convert for 2.0 s (`svg-text`) | `svg_bounds` charges a `<text>` by its spans × characters | unreported |

Both are fixed in the fork [nchashch/fluent-rs](https://github.com/nchashch/fluent-rs):
branch `fix/fuzzing-bugs` (on upstream `main`, 0.12; for the PR) and branch
`fix/fuzzing-bugs-0.11` (on tag `fluent-syntax@0.11.1`; what bevy_markup
patches in, since fluent 0.16 / fluent-bundle 0.15 needs `^0.11`). One
commit on each, same
diff: `fluent-syntax/src/parser/{core,errors,expression,helper}.rs` and the
new test file `fluent-syntax/tests/robustness.rs`. Upstream's `cargo fmt`,
CI clippy command and workspace tests pass on both.

## U1 — fluent-syntax: invalid unicode escape before a multi-byte character panics

- **Project:** [projectfluent/fluent-rs](https://github.com/projectfluent/fluent-rs),
  crate `fluent-syntax`. Affected: 0.11.1 (pinned by fluent 0.16 via
  fluent-bundle 0.15) and 0.12.0 (latest checked, 2026-10-05).
- **Problem:** `skip_unicode_escape_sequence` reports an invalid `\u`/`\U`
  escape with the character that ended it, sliced as `ptr + 1`, one byte.
  If that character is multi-byte, `Slice::slice` (`&self[range]`) panics.
  It's the only unsafe slice: the parser's other `ptr - 1` slices follow an
  ASCII byte, and replaying ~4,900 ftl fuzz inputs with a boundary clamp
  removed found no other panic. (An earlier "second variant", a long
  multi-byte error context, was wrong: that input parses to an ordinary
  error.)
- **Reproduction:** `FluentResource::try_new("u={\"\\U\u{fffd}".to_owned())`
  panics with "byte index 7 is not a char boundary".
- **Fix (fork):** take the whole offending character
  (`chars().next().map(char::len_utf8)`) instead of one byte. The first
  local fix (vendored, now removed) clamped every slice to a char boundary.
- **Workaround now (2026-11-11, ADR 0045):** `ftl_check::bad_escape` refuses the file at load, before fluent-syntax parses it; the fork is no longer wired in (`[patch]` removed from every manifest), so dependents need nothing. Drop the check's part once a released fluent-syntax that `fluent` uses carries the fix.
- **Tests:** fork `robustness.rs` `invalid_unicode_escape_before_multibyte_char`
  (includes the exact error text); bevy_markup `src/fuzz.rs`
  `broken_unicode_escape_after_multibyte_char_does_not_panic`.

## U2 — fluent-syntax: unbounded expression nesting overflows the stack

- **Project:** as U1. Affected: 0.11.1 and 0.12.0 (latest checked,
  2026-10-05).
- **Problem:** inline expressions nest recursively with no limit, along two
  cycles: placeables (placeable → expression → inline expression or select
  variant → placeable) and call arguments (inline expression → call
  arguments → inline expression, `F(F(F(…)))`, no braces needed). Measured
  overflow depths for placeables: debug 200–400 levels on a 2 MB stack
  (800–1600 on 8 MB), release 1600–3200 on 2 MB (6400–12800 on 8 MB); calls
  overflow at 100–400 levels in debug on 2 MB. A few KB of FTL abort any
  process that parses it, e.g. a Bevy app loading a translation on a worker
  thread.
- **Reproduction:**
  `FluentResource::try_new(format!("x = {{ {}1{} }}\n", "F(".repeat(100_000), ")".repeat(100_000)))`
  (or the same with `{`/`}` around `"a"`) aborts with a stack overflow.
- **Fix (fork):** both cycles pass through `get_inline_expression` once per
  level; a depth counter there returns the new `ErrorKind::NestingTooDeep`
  past `MAX_NESTING_DEPTH` = 100, and the entry becomes Junk. The first
  local fix (vendored, now removed) counted placeables only and missed the
  call cycle.
- **Workaround now (2026-11-11, ADR 0045):** `ftl_check`'s port of the parser refuses nesting past `MAX_FTL_NESTING` (64) at load, before fluent-syntax parses it; the fork is no longer wired in (`[patch]` removed from every manifest), so dependents need nothing. Drop the check's part once a released fluent-syntax that `fluent` uses carries the fix.
- **Tests:** fork `robustness.rs` (100,000 nested placeables and calls on a
  2 MiB stack; the boundaries 100/101 braces and 99/100 calls, derived by
  hand; a too-deep entry leaves the next message intact); bevy_markup
  `src/fuzz.rs` `deeply_nested_expressions_are_an_error_not_a_stack_overflow`,
  `nesting_up_to_the_limit_still_works`.
- **For the PR:** the limit (100) and the new `ErrorKind` variant (a public
  enum without `#[non_exhaustive]`, so technically breaking for exhaustive
  matches) are maintainer decisions. fluent-bundle's resolver already caps
  placeables per message (`MAX_PLACEABLES`).

## U3 — honggfuzz-rs: bundled honggfuzz fails against current binutils

- **Project:** [rust-fuzz/honggfuzz-rs](https://github.com/rust-fuzz/honggfuzz-rs),
  crate `honggfuzz` 0.5.62 (latest on crates.io, 2026-10-04). The C code
  comes from upstream honggfuzz (google/honggfuzz).
- **Problem:** `honggfuzz/linux/bfd.c` compares `bfd_find_nearest_line`'s
  result with `TRUE`; current binutils' `bfd.h` no longer defines it (it
  uses C `bool`), so the build fails.
- **Local fix:** `honggfuzz/vendor/honggfuzz/honggfuzz/linux/bfd.c`: `== TRUE`
  → `== true` (two lines, 170 and 176).
- **Upstream:** check whether google/honggfuzz already fixed `bfd.c`. If so,
  honggfuzz-rs only needs to update its bundled copy; else PR to
  google/honggfuzz first.

## U4 — fuzzcheck: coverage sensor misreads LLVM 21+ profile data

- **Project:** [loiclec/fuzzcheck-rs](https://github.com/loiclec/fuzzcheck-rs),
  crate `fuzzcheck` 0.13.0 (latest on crates.io). Vendored here from git
  master (edition 2024). Check maintenance status before investing in a PR.
- **Problem:** the coverage sensor parses `__llvm_prf_data` with the old
  record layout. With LLVM 21+ (rustc 1.100 nightly) records are 9 × u64 =
  72 bytes and NumCounters sits at offset 0x38 (verified against a
  `-C instrument-coverage` section dump), so counter counts and offsets are
  garbage.
- **Local fix:** `fuzzcheck/vendor/fuzzcheck/src/code_coverage_sensor/llvm_coverage.rs`
  (`PATCH(bevy_markup)`): read the relative counter pointer as a signed offset,
  track `counter_offset` per record, read NumCounters at the new offset.
  It also prints a `DIAG prf_data` line (a debugging leftover to drop
  before upstreaming).
- **Upstream PR sketch:** detect the profile format version
  (`__llvm_prf_data` layout differs by LLVM version) rather than hard-coding
  the new layout.

## U5 — fuzzcheck: current-dir filter rejects absolute paths

- **Project:** as U4.
- **Problem:** `CodeCoverageSensor::observing_only_files_from_current_dir`
  keeps only files whose path `is_relative()`. Cargo passes absolute source
  paths, so no project file passes the filter and the sensor observes
  nothing.
- **Local fix:** `fuzzcheck/vendor/fuzzcheck/src/code_coverage_sensor/mod.rs`:
  also keep `file.starts_with(current_dir)`.

## U6 — fuzzcheck: derive crate doesn't compile on current Rust

- **Project:** as U4, crate `fuzzcheck_mutators_derive` 0.13.0.
- **Problem:** it uses let-chains but declares edition 2021, where they
  don't compile (let-chains are edition-2024 only).
- **Local fix:** vendored with `edition = "2024"` (`fuzzcheck/Cargo.toml`
  `[patch.crates-io]`). Upstream git master may already be on edition 2024;
  then the request is just a crates.io release.

## U7 — cargo-mutants: regex filters skip field-deletion mutants

- **Project:** [sourcefrog/cargo-mutants](https://github.com/sourcefrog/cargo-mutants),
  27.1.0.
- **Problem:** `--re` and `--exclude-re` select mutants by name, but
  "delete field `x` from struct `Y` expression" mutants are always included,
  even when their name matches neither filter. Reproduction in bevy_markup: a regex
  matching no mutant at all, e.g.
  `cargo mutants --list --re '^src/nonexistent\.rs:'`, still lists every
  struct-field deletion in the crate (31 on 2026-10-04).
- **Local workaround:** none (documented in AGENTS.md, Testing TODO 6);
  targeted reruns take ~10 min instead of ~2.

## U8 — cargo-fuzz: prebuilt binary defaults to a musl target

- **Project:** [rust-fuzz/cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz),
  0.13.2 as installed by `taiki-e/install-action` (via cargo-binstall, a
  musl build).
- **Problem:** cargo-fuzz's default `--target` is the triple cargo-fuzz
  itself was compiled for. The prebuilt binary is
  `x86_64-unknown-linux-musl`, so fuzz targets are built for musl, where
  AddressSanitizer is unsupported ("sanitizer is incompatible with
  statically linked libc").
- **Local workaround:** `scripts/fuzz-libfuzzer.sh` passes
  `--target "$(rustc +nightly -vV | sed -n 's/^host: //p')"`.
- **Upstream suggestion:** default to the active rustc's host triple instead
  of the build triple.

## U9 — Bevy 0.19: UI picking systems require picking resources (discuss first)

- **Project:** [bevyengine/bevy](https://github.com/bevyengine/bevy), 0.19.1.
- **Problem:** `UiPlugin` adds `bevy_ui::widget::viewport::viewport_picking`,
  which takes `Res<HoverMap>` / `Res<PointerState>`. Without
  `DefaultPickingPlugins` (e.g. a headless app with `UiPlugin` only) the
  first update panics on the missing resource. It's arguably intended
  (`UiPlugin` assumes picking); worth asking whether the system should be
  conditional or the dependency explicit.
- **Local workaround:** `tests/common/mod.rs` (`TestUi::with_layout`) adds
  `DefaultPickingPlugins` and `TextureAtlasPlugin`.

## U10 — lightningcss: bare `grid-auto-flow: dense` doesn't parse

- **Project:** [parcel-bundler/lightningcss](https://github.com/parcel-bundler/lightningcss),
  1.0.0-alpha.72 (the version bevy_markup uses; re-check the latest).
- **Problem:** `<GridAutoFlow as Parse>::parse` (`src/properties/grid.rs`),
  on a leading `dense`, `try_parse`s a following `row`/`column` and
  propagates its error with `?`, so `dense` alone (valid CSS: `[ row |
  column ] || dense`, = `row dense`) is an invalid declaration. `dense row`,
  `row dense`, `column dense` parse.
- **Reproduction:** `StyleSheet::parse("p { grid-auto-flow: dense }", …)`
  yields no `Property::GridAutoFlow` declaration.
- **Local workaround:** none; documented in `src/style.rs` (write
  `row dense`), the example uses `row dense`; `cascade::tests::
  grid_container_properties_map_to_bevy_values` notes it at its `dense row`
  case.
- **Upstream fix sketch:** make the trailing direction optional (`.ok()`
  instead of `?`) and keep `Row` as the default.

## U11 — tl: a value-less attribute eats the next attribute's first character

- **Project:** [y21/tl](https://github.com/y21/tl) 0.7.8 (latest release,
  checked 2026-10-05). Fixed in [astral-sh/astral-tl](https://github.com/astral-sh/astral-tl)
  0.8.0 (crates.io `astral-tl`, library name `tl`).
- **Defect:** `parser::base::parse_attributes` advances one byte after every
  attribute (to step over a quoted value's closing quote), but for a
  value-less attribute `parse_attribute` has already skipped the whitespace
  and stands on the next attribute's first byte: `<div autofocus
  data-on-click="x">` parses as `autofocus` + `ata-on-click="x"`.
- **Workaround:** bevy_markup depends on `astral-tl` (`tl = { package =
  "astral-tl", … }` in `Cargo.toml`); API-compatible, full suite passes.
  Regression test: `focus_navigation_scope_and_styles` (`autofocus` first).
- **Status:** not reported to y21/tl (fixed in the maintained fork; y21/tl
  has had no release since 0.7.8).

## U12 — bevy_picking: text-section hits resolve `Pickable` on the span, not the text node

- **Project:** [bevyengine/bevy](https://github.com/bevyengine/bevy) 0.19.1
  (`bevy_ui` 0.19.1 picking backend + `bevy_picking` 0.19.1 hover map).
- **Problem:** for a `Text` node, `bevy_ui`'s `ui_picking` reports the *text
  span entity* as the hit target and attaches the text **node**'s `Pickable`
  to the hit. `bevy_picking`'s `build_hover_map` then resolves `Pickable`
  with a query on the **target** entity — the span, which normally has no
  `Pickable` — so the node's `should_block_lower: false` /
  `is_hoverable: false` never apply: the span hovers and blocks by default,
  and the event bubbles up from it. Concretely, `pointer-events: none` on a
  UI element didn't make its text unclickable (bevy_markup
  [bug_0020](bug_0020.md)).
- **Reproduction:** the backend pushes the hit with the node's `Pickable`;
  put `Pickable::IGNORE` on a text node under a click point and observe that
  the click still reaches the node's ancestors (observers) instead of
  falling through to what's behind. bevy_markup's
  `pointer_events_none_silences_an_element` (`tests/signals.rs`) is the
  end-to-end form; it failed before the local fix and passes with spans
  carrying `Pickable::IGNORE`.
- **Local workaround:** `src/build.rs` puts `Pickable::IGNORE` on every span
  of an ignored block (`spawn_spec`), and takes it back on restyle
  (`apply_spec`).
- **Upstream question (discuss first):** should `build_hover_map` use the
  `Pickable` recorded on the hit (the text node's) rather than re-querying
  the target entity, or should the backend report the text node as the
  target? Either fixes text under `pointer-events: none`; the second also
  changes which entity `Pointer<Over>` observers see.

## U13 — pulldown-cmark: the offset iterator panics on a definition-only list item before a blank line

- **Project:** [pulldown-cmark/pulldown-cmark](https://github.com/pulldown-cmark/pulldown-cmark).
  Affected: 0.13.4 (bevy_markup's lock until bug_0178) and the 0.13 branch
  before `078feb520e`; fixed in **0.13.5** (checked 2026-10-26; upstream
  `main` at `61d95cedd7`, 2026-10-23, does not panic either).
- **Problem:** `Parser::into_offset_iter()` panics with
  `called Option::unwrap() on a None value` at `src/parse.rs:2199`
  (0.13.4) when a list item contains only a link reference definition and
  the next line holds only whitespace such as a vertical tab or form feed.
  The tight paragraph left behind has no children, and the code that skips
  tight paragraphs (`tree.push(); tree.cur().unwrap()`) assumes one. The
  plain iterator (`Parser::next`) does not panic on these inputs.
- **Reproduction:**
  `Parser::new_ext("- [a]: b\n\u{0B}", Options::empty()).into_offset_iter().count()`
  panics on 0.13.4. bevy_markup reaches it from `.md` templates
  (`md::dedent_blocks` → `code_ranges`); found by the honggfuzz `html`
  target (bug_0178).
- **Upstream fix:** PR [#1021](https://github.com/pulldown-cmark/pulldown-cmark/pull/1021)
  ("Correctly parse all-spaces line between refdef and paragraph", for
  issue [#997](https://github.com/pulldown-cmark/pulldown-cmark/issues/997)),
  cherry-picked into the 0.13 branch as `078feb520e` and released in 0.13.5.
  Bisected: `96201943db` (just before) panics, `078feb520e` does not. Issue
  [#1084](https://github.com/pulldown-cmark/pulldown-cmark/issues/1084) is
  another input reaching the same `unwrap()` (tasklists + strikethrough),
  also fixed for its input.
- **Local workaround:** none needed; `Cargo.toml` requires
  `pulldown-cmark` ≥ 0.13.5 and all five lockfiles are on 0.13.5. The panic
  is genuinely fixed in 0.13.5 (bug_0178 resolved): the parse change means
  the empty tight paragraph is no longer produced, so neither iterator
  panics on the inputs above.
- **Residual asymmetry (latent, not reachable; no report warranted):** in
  0.13.5 the two iterators diverge where they skip a tight paragraph.
  `Parser::next` (plain, `src/parse.rs:2431`) was hardened — it binds
  `tree.cur()` to `maybe_cur_ix` and guards it with `if let Some(..)`, so an
  empty tight paragraph emits nothing. `OffsetIter::next`
  (`src/parse.rs:2267`, the path bevy_markup's `code_ranges` uses) still
  does `self.tree.push(); self.tree.cur().unwrap()`. So the unwrap survives,
  but the parser no longer reaches it: the hardening is defense-in-depth for
  a state the parse fixes removed. It would become a live panic again only
  if a future parse change reintroduced an empty tight paragraph, and then
  only through the offset iterator.
- **Why "not reachable" and not just "not found":** three searches on
  0.13.5, all against `Parser::new_ext(data, Options::all()).into_offset_iter()`
  (2026-10-26):
  - **Coverage-guided fuzzing** (cargo-fuzz / libFuzzer), seeded with the
    0.13.4 trigger and the near-miss cases (`issue_1095` `"- [n]:Z\r\n\t\t"`
    and `issue_1084`): two runs, ~6.6M and ~11.7M executions (~18.3M total),
    ~6240 edges covered, no panic.
  - **Instrumented replay:** a vendored 0.13.5 with that exact
    `.cur().unwrap()` replaced by a probe logging when `cur()` is `None`;
    the 40,933-input fuzz corpus replayed through it reached the
    empty-tight-paragraph branch **0 times**.
  - Hand-built refdef / tight-list / trailing-whitespace combinations and a
    token-soup random search: nothing.
  If a future upstream change reopens this, the fix is to make
  `OffsetIter::next` match the plain iterator's `if let Some(..)` guard;
  left unreported because it is not reachable today.

## U14 — lightningcss: nested `light-dark(` and `calc(var(…))` parse in exponential time

- **Project:** [parcel-bundler/lightningcss](https://github.com/parcel-bundler/lightningcss).
  Affected: 1.0.0-alpha.72 (bevy_markup's lock; the latest release,
  checked 2026-10-28) and `master`.
- **Problem:** parse time doubles per nesting level. `light-dark()`
  arguments are parsed as a color first; without a comma that fails after
  the full parse, and the generic function path parses them again
  (#1382). A typed property with nested `calc()` around a `var()` does the
  same (#1389). Measurements: bug_0179 (depth 18: 36 ms / 554 ms).
- **Reproduction:** `StyleSheet::parse(&format!("a{{color:{}", "light-dark(".repeat(24)), ParserOptions::default())`
  takes ~2 s in release; `.x{width:` + 22 nested `calc(… - 1px)` around
  `var(--x)` ~19 s (#1389).
- **Upstream:** issue [#1382](https://github.com/parcel-bundler/lightningcss/issues/1382)
  with fix PR [#1383](https://github.com/parcel-bundler/lightningcss/pull/1383)
  (open, mergeable, 2026-10-22); issue [#1389](https://github.com/parcel-bundler/lightningcss/issues/1389)
  (open, no PR). Both filed by others from fuzzing.
- **Local workaround:** `nesting_over_limit` rejects text nesting `(…)`
  deeper than 8 before lightningcss sees it (sheets fail their load,
  `style=` attributes and substituted values are dropped). Revisit the
  bound (not the block bound, U15) once both are fixed in a release.

## U15 — lightningcss: unbounded parser recursion

- **Project:** [parcel-bundler/lightningcss](https://github.com/parcel-bundler/lightningcss).
  Affected: 1.0.0-alpha.72 and `master`.
- **Problem:** the parser recurses once per block (`(`, `[`, `{`, nested
  rules) with no depth limit. ~500 nested blocks overflow a 2 MB thread in
  a debug build, ~3000 nested `calc(` or `light-dark(` in release
  (bug_0179); nested rules too (bug_0106).
- **Upstream:** issues [#1297](https://github.com/parcel-bundler/lightningcss/issues/1297)
  (selector, math-function and rule-nesting recursion),
  [#1155](https://github.com/parcel-bundler/lightningcss/issues/1155),
  [#1092](https://github.com/parcel-bundler/lightningcss/issues/1092); all
  open, no PR (checked 2026-10-28).
- **Local workaround:** `nesting_over_limit` bounds all blocks at 32
  (bug_0092, bug_0106). The css fuzz harness applies it too (bug_0179),
  so fuzzing doesn't spend time on inputs every app rejects.

## U16 — tera: expression compilation recurses once per operator

- **Project:** [Keats/tera](https://github.com/Keats/tera). Affected:
  2.4.0 (bevy_markup's lock; the latest release, checked 2026-11-03).
- **Problem:** `Compiler::compile_expr` walks the left-deep AST of an
  operator chain recursively, one frame per operator. Issue
  [#1035](https://github.com/Keats/tera/issues/1035) (closed 2026-09-09)
  reported the unbounded case; 2.4.0 bounds it with
  `MAX_EXPRESSION_COMPLEXITY` = 256, measured against an 8 MB main
  thread. On a 2 MB thread a debug build overflows from 136 operators;
  release holds up to the cap (bug_0182).
- **Reproduction:** in a debug build on a 2 MB thread,
  `Tera::new().add_raw_template("t", &format!("{{{{ n{} }}}}", " - n".repeat(136)))`
  aborts.
- **Upstream:** not reported. A PR sketch: make `compile_expr` iterative
  along the left spine of binary operations and filters, or count depth
  in the compiler and lower the cap to what a 2 MB debug thread holds.
- **Local workaround:** `template::tera_tag_over_limit` rejects a tag of
  more than 64 operators before Tera compiles it; drop it once Tera's
  compiler holds its cap on a 2 MB debug thread.

## U17 — usvg: converter recursion through references and nesting is unbounded

- **Project:** [linebender/resvg](https://github.com/linebender/resvg)
  (`usvg` crate). Affected: 0.49.0 (the latest release, checked
  2026-11-07); `main` after PR
  [#1223](https://github.com/linebender/resvg/pull/1223) (merged
  2026-11-05, unreleased) too, which makes the XML nesting limit
  configurable (`Options::max_depth`, default 1024) and counts `<use>`
  instances, but not references followed by the converter.
- **Problem:** `parser/converter.rs` recurses once per nested element
  and once per reference it follows into a target's content
  (`paint_server::convert_pattern` → `convert_children` → … →
  `convert_path` → back into `paint_server::convert`; likewise masks,
  clip paths, markers, `feImage`). On a 2 MB thread (Bevy's task-pool
  default) a debug build overflows at 121 chained patterns, 113 `feImage`
  hops, 123 markers, 251 `<use>` hops, 293 masks, 370 nested `<svg>`,
  ~689 nested `<g>` (below the 1024 nesting check); release at about 3×
  those (probe report `docs/agents/performance/adr-0038-probe.md` §6).
  A few KB of SVG abort the process.
- **Reproduction:** on a 2 MB thread, `usvg::Tree::from_str` of
  `<svg xmlns="http://www.w3.org/2000/svg">` followed by 130 patterns,
  each `<pattern id="pN" width="8" height="8"
  patternUnits="userSpaceOnUse"><rect width="8" height="8"
  fill="url(#pN-1)"/></pattern>`, and a rect filled with the last.
- **Upstream:** not reported. A PR sketch: carry a depth in the
  converter's `State` (already threaded through every recursive call),
  increment it on each element and each followed reference, and fail
  with the existing `NodesLimitReached` past `Options::max_depth` — the
  option #1223 introduced, so callers on small stacks set one number.
- **Local workaround:** `svg::parse` parses the XML with roxmltree,
  bounds the recursion with `svg_bounds::bounds` (longest path over
  nesting and `url(#…)`/`href` edges, `<style>` rules matched with
  simplecss as usvg matches them, cycles refused) and only then calls
  `Tree::from_xmltree` on the same document. Keep it until a usvg
  release bounds the converter's recursion; then set `max_depth` and
  drop the check (keep `accepted_depths_fit_a_two_megabyte_stack`).

## U18 — roxmltree: the tokenizer recursed once per nesting level

- **Project:** [RazrFalcon/roxmltree](https://github.com/RazrFalcon/roxmltree).
  Affected: 0.21.1 and earlier; fixed in 0.21.2 (2026-10-24: "Element
  content is now parsed iteratively instead of recursing once per
  nesting level").
- **Problem:** `tokenizer::parse_element` / `parse_content` recursed per
  level, ~14 KB of stack each in a debug build: a 2 MB thread overflowed
  at ~150 nested elements. usvg 0.49 accepts `roxmltree ^0.21.1`, so an
  older lockfile keeps the bad version (bevy_markup's did).
- **Local workaround:** `roxmltree = "0.21.2"` in `Cargo.toml` (the `svg`
  feature), a version floor. Status: fixed upstream; the floor stays as
  long as usvg allows 0.21.1.

## U19 — usvg: the default image resolver reads any file path

- **Project:** linebender/resvg (`usvg`). Affected: 0.49.0.
- **Problem:** `ImageHrefResolver::default_string_resolver` treats every
  non-`data:` `href` as a file path (absolute, cwd-relative without
  `resources_dir`, `..` without containment) and `std::fs::read`s it,
  embedding bitmaps and parsing SVGs (probe report §2, under `strace`).
  Reasonable for a command-line converter; a hazard for libraries that
  render untrusted SVG, and the docs don't warn.
- **Upstream:** not reported. Suggest a docs warning, or a resolver that
  confines paths to `resources_dir`.
- **Local workaround:** `svg::options` installs a resolver whose closures
  return `None`.

## U20 — resvg: `feTurbulence` overflows `i32` after enough octaves

- **Project:** linebender/resvg. Affected: 0.49.0. Reported upstream as
  [#1185](https://github.com/linebender/resvg/issues/1185) ("Panic
  attempt to multiply with overflow in filter/turbulence.rs", open); #937
  (a multiply overflow in `lib.rs`) is open too.
- **Problem:** `filter/turbulence.rs` `noise2` computes
  `bx0 = (x + 4096) as i32` and `bx1 = bx0 + 1`; each octave doubles `x`,
  so after enough octaves the cast saturates and `+ 1` overflows. A panic
  where overflow checks are on; release builds wrap (the lattice index is
  masked, so the noise is merely wrong).
- **Reproduction:** draw `<filter id="f"><feTurbulence baseFrequency="0.05"
  numOctaves="100"/></filter><rect width="16" height="16"
  filter="url(#f)"/>` in a debug build. A huge finite frequency reaches
  the same line in one octave: `baseFrequency='1e20'` on a 64 × 64 image
  (found 2026-11-08; about 3e7 is enough there). Not in #1185 yet: add it
  when reporting.
- **Impact on bevy_markup:** Bevy turns a panicking asset loader into a
  failed load, but the `<img>` redraws run as `AsyncComputeTaskPool`
  tasks, and polling a panicked task re-raises the panic in the poller,
  the app's main thread (probe report §8).
- **Local workaround:** `svg::guarded` wraps usvg's parse and resvg's
  draw in `catch_unwind`: the panic becomes `SvgError::Panicked` (the load
  fails, or the size isn't redrawn). Keep it while resvg can panic on
  input; it costs nothing on the normal path. The `svg` fuzz target
  (debug assertions on) lets this panic unwind into `guarded` instead of
  aborting (`fuzz::is_known_upstream_debug_panic`, matched by file and
  message in `resvg-0.49.0/src/filter/turbulence.rs`), so it no longer
  turns the nightly red; `fuzz::svg_tests` fails once a resvg release
  stops panicking, as a reminder to drop the excuse.

## U21 — astral-tl: `/` is a name character, so `<b/>` never closes

- **Project:** [astral-sh/astral-tl](https://github.com/astral-sh/astral-tl)
  0.8.1 (crates.io `astral-tl`, library `tl`; the latest release, checked
  2026-11-07). y21/tl 0.7.8 has the same `IDENT_CHARS` table.
- **Problem:** `util.rs` `IDENT_CHARS` includes `/`, and `parse_tag`
  reads a tag name with `read_ident`: `<b/>` is an element named `b/`,
  the self-closing check finds no `/`, and the element stays open over
  the rest of the document. The same goes for a value-less attribute
  before `/>` (bug_0185).
- **Reproduction:** `tl::parse("<p>A <br/> B</p><p>C</p>")`: the second
  `p` is a child of `br/`.
- **Upstream:** not reported. A PR sketch: end `read_ident` for tag and
  attribute names at `/` (keep it for unquoted values), or check for
  `/>` after the name.
- **Local workaround:** `template::normalize_self_closing` (a space
  before such a `/>`) in `HtmlDocument::parse` and `walk_translation`.

## U22 — svgtypes / kurbo: arc subdivision grows with the radius, and the drain is quadratic

- **Project:** RazrFalcon/svgtypes 0.16.2 (`SimplifyingPathParser`) with
  linebender/kurbo 0.13.1 (`Arc::to_cubic_beziers`); usvg 0.49 builds
  shapes' arcs through tiny-skia's `PathBuilder::arc_to` with the same
  kurbo call.
- **Problem:** the tolerance is an absolute 0.1 user units, and
  `append_iter` makes `ceil(max((1.1163·r/0.1)^⅙, 4) · sweep/2π)` cubics:
  1.5·10⁵ for one arc at `r = 1e30`, ~7·10¹¹ at `r = 1e70` (past ~1e77,
  `rx·ry` squared can overflow and the count be NaN, so 0). The sweep is
  computed from where the previous arc's last cubic ended, a rounding
  error off the written point, and a large radius amplifies that error:
  `A1e92 1e9 0 1 1 0 0` after `A1e9 1e9 0 1 1 1e9 0` is ~3·10¹⁵ cubics,
  one from the exact point (bug_0193). The simplifier
  pushes every cubic into `buffer` and returns them with
  `self.buffer.remove(0)`, which moves the rest each time: quadratic.
- **Reproduction:** `<path d='M0 0 A1e30 1e30 0 1 1 1e30 0'/>` converts
  for 4.3 s (ASan); `A3e38 3e38 0 1 1 3e38 0` runs past 100 s.
- **Local workaround:** `svg_bounds::path_segments` counts each arc's
  cubics with kurbo's arithmetic (walking svgtypes' raw `PathParser`), and
  the work bound refuses the document before usvg runs (ADR 0040).
- **PR sketch:** svgtypes: drain with a `VecDeque` (or an index), and cap
  the cubics per arc (a curve is never visibly better past a few hundred).
  kurbo/usvg: a tolerance relative to the radius, or to the output scale.

## U23 — usvg: a pattern inheriting a paint naming itself recurses forever

- **Project:** linebender/resvg, crate usvg. Affected: 0.49.0.
- **Problem:** converting a pattern converts its content. A shape inside
  that inherits `fill`/`stroke` naming the pattern from an ancestor of the
  pattern converts the pattern again, without end: the paint cache is
  filled only after the content converts. svgtree's recursive-pattern
  check looks at the content's own attributes, not inherited ones.
- **Reproduction:** `<g fill='url(#p)'><pattern id='p' width='10'
  height='10' patternUnits='userSpaceOnUse'><rect width='5'
  height='5'/></pattern><rect width='64' height='64'/></g>` overflows the
  stack.
- **Local workaround:** inherited paint servers are edges of
  `svg_bounds::bounds`' graph, so the document is a refused cycle.
- **PR sketch:** carry the patterns being converted in `State` (as
  `parent_markers` does for markers) and resolve a nested reference to
  one of them as `none`.

## U24 — tiny-skia: the anti-aliased row range wraps on paths far off the canvas

- **Project:** linebender/tiny-skia 0.12.0.
- **Problem:** `scan/path.rs` (around line 164) takes the path's integer
  bounds before clipping them to the canvas (the code's own `TODO:
  SkScanClipper`; Skia clips first) and, for anti-aliasing, shifts them
  left by 2 (`start_y <<= shift_edges_up`). Past |y| ≥ 2²⁹ the `i32`
  wraps and the edge walk starts at a garbage row. Debug builds then fail
  `walk_edges`' `debug_assert!(edges[curr_idx].last_y >= curr_y as i32)`;
  release builds write outside the canvas (U28). Lines alone trigger it;
  non-anti-aliased fills don't.
- **Reproduction:** a 64 × 64 SVG with `<path d='M603553400 -786566100
  L-232962910 303603170 L554380750 -829689350 Z'/>`, or `<path d='M0 0
  A1e9 1e9 0 1 1 1e9 0 A1e9 1e9 0 1 1 0 0'/>` (seed `004a6ba5…` in
  `fuzz/seeds/svg/`). Through tiny-skia alone: fill that triangle at
  identity with `anti_alias = true` (halving the coordinates still panics,
  quartering them doesn't).
- **Impact on bevy_markup:** before the workaround, a failed load
  (`svg::guarded` catches the panic). An earlier version of this entry
  said release builds were unaffected; a release replay showed U28
  ([bug_0194](bug_0194.md)).
- **Local workaround:** `svg::check_extent` refuses a drawing that reaches
  more than `MAX_SVG_EXTENT` (2¹⁹ px) outside the image, measured on
  control points with strokes padded, through every transform resvg
  applies (`SvgError::TooFarOut`). Drop it, and the limit, once tiny-skia
  clips first.
- **PR sketch:** clip the integer bounds to the clip rect (Skia's
  `SkScanClipper`) before the shift, or compute the shift in `i64` and
  reject what doesn't fit.

## U25 — usvg: no bound on conversion work

- **Project:** linebender/resvg, crate usvg. Affected: 0.49.0.
- **Problem:** usvg's only size limit counts its tree's nodes after `<use>`
  copies (a million). Conversion is ~7 µs per node in release, so a fan-out
  under the limit converts for seconds (bug_0188). Markers are converted per
  path vertex and aren't counted at all (bug_0189). Not a defect in usvg's
  terms: it has no notion of a budget.
- **Local workaround:** `svg_bounds::bounds` estimates the conversion's work
  from the XML (instances over the reference graph, inherited markers and
  paint servers, path segments), refused over `MAX_SVG_WORK` (ADR 0040).
- **PR sketch:** an `Options::max_work`-style budget counted by the
  converter itself (nodes built plus segments), failing with an error past
  it. With it, the model here could go.

## U26 — usvg: an infinite `baseFrequency` panics

- **Project:** linebender/resvg, crate usvg. Affected: 0.49.0.
- **Problem:** `parser/filter.rs` `convert_turbulence` parses
  `baseFrequency` as a number list, converts to `f32` (a value past
  `f32::MAX` becomes `inf`), checks only `is_sign_positive()` and calls
  `PositiveF32::new(x).unwrap()`, which is `None` for infinity. An
  unconditional `unwrap`: release builds panic too.
- **Reproduction:** `<filter id='f'><feTurbulence
  baseFrequency='1e40'/></filter><rect width='8' height='8'
  filter='url(#f)'/>`. Found by a local `svg` fuzz run on 2026-11-08
  (artifact `crash-05abaafb…`, a 557-byte filter chain).
- **Impact on bevy_markup:** before the workaround, the load failed with
  `SvgError::Panicked` (`guarded`), and libFuzzer counted it a crash
  ([bug_0192](bug_0192.md)).
- **Local workaround:** `svg_bounds::check_values` refuses an
  `feTurbulence` whose `baseFrequency` holds a non-finite `f32` before usvg
  runs (`SvgError::OutOfRange`). A number past `f64` fails svgtypes' parse
  and usvg ignores the attribute, so that isn't refused. Drop the check
  once usvg handles it.
- **PR sketch:** treat a non-finite frequency like a negative one (keep the
  default), or use `PositiveF32::new(x).unwrap_or_default()`.

## U27 — tiny-skia: the cubic clipper unwraps a `t` of 1.0

- **Project:** linebender/tiny-skia 0.12.0.
- **Problem:** `edge_clipper.rs` (around line 568) `mono_cubic_closest_t`
  bisects for the `t` where a monotonic cubic crosses a clip edge, until
  the point is within 0.25 px. Between ~2²¹ and 2²² px the f32 spacing is
  0.25 px or coarser, so the search never gets within tolerance and `t`
  walks up to exactly 1.0. `NormalizedF32Exclusive::new(1.0).unwrap()` then
  panics, in every build. Above 2²² `too_big_for_reliable_float_math`
  draws such a cubic as a line, so only that band is affected.
- **Reproduction:** `svg::tests::paths_far_outside_the_image_are_refused`'s
  cubic (three cubics around 2·10⁶ px, `crispEdges`, stroke width 3), or
  artifact `crash-13d520f9…` (a seed in `fuzz/seeds/svg/`).
- **Local workaround:** as U24 (`MAX_SVG_EXTENT` is below 2²¹).
- **PR sketch:** stop the bisection after a fixed number of steps and
  clamp `t` into the exclusive range, or use an inclusive type there.

## U28 — tiny-skia: release builds write outside the canvas (U24)

- **Project:** linebender/tiny-skia 0.12.0.
- **Problem:** with U24's wrapped row range and no debug assertion, the
  scan converter writes coverage outside the canvas. Later code catches
  it: `alpha_runs.rs` (around line 189) unwraps a `None`, or
  `pipeline/mod.rs` (around line 205) slices out of range.
- **Reproduction:** U24's inputs in a release build (`cargo fuzz run -O -s
  none … svg <seed>`); artifact `crash-c7219239…` (a seed).
- **Local workaround:** as U24.
- **PR sketch:** fixed by U24's fix; report together.

## U29 — usvg: `feConvolveMatrix` `order` overflows `u32`

- **Project:** linebender/resvg, crate usvg. Affected: 0.49.0.
- **Problem:** `parser/filter.rs` `convert_convolve_matrix` reads `order`
  as one or two numbers saturated to `i32`, keeps them when positive, and
  multiplies them as `u32` unchecked: `order_x * order_y` when a
  `kernelMatrix` is present (line 674), and `columns * rows` in
  `tree/filter.rs` `ConvolveMatrixData::new` (line 459). Debug builds
  panic (`attempt to multiply with overflow`). Release builds wrap. From
  reading the code: `order='65536'` with no `kernelMatrix` wraps to 0, which
  equals the empty matrix's length, so usvg accepts a 65536 × 65536 kernel
  with no data. resvg would then index that empty matrix; bevy_markup's
  cost estimate multiplies the sides in `f64` and refuses it first.
- **Reproduction:** `<filter id='f'><feConvolveMatrix
  order='65536'/></filter><rect width='8' height='8' filter='url(#f)'/>`
  (seed `ddf57483…` in `fuzz/seeds/svg/`). Found by a local `svg` fuzz run
  on 2026-11-09 (three artifacts, all filter chains whose `order` was
  mangled into a huge number).
- **Local workaround:** `svg_bounds::check_values` refuses an `order` whose
  product (read as usvg reads it) doesn't fit in `u32`
  (`SvgError::OutOfRange`). Drop it once usvg checks.
- **PR sketch:** `checked_mul` in both places, treating an overflow like a
  mismatched `kernelMatrix` (the primitive is dropped); or cap the order at
  a sane size, since a large kernel is quadratic per pixel anyway.

## U30 — usvg: `feMorphology` radius × scale unwrapped as `PositiveF32`

- **Project:** linebender/resvg, crate usvg. Affected: 0.49.0.
- **Problem:** `parser/filter.rs` `convert_morphology` (lines 1028–1029)
  reads `radius` as `f32`s and, when both are non-negative, unwraps
  `PositiveF32::new(rx * scale.width())` (and `ry`). `PositiveF32` refuses
  infinity, so a radius past `f32::MAX` (`1e49` parses to infinity) panics
  in every build. Under `primitiveUnits="objectBoundingBox"` the scale is
  the element's bounding box, so a finite radius over a huge box panics
  too (radius 2, box width 2·10³⁸).
- **Reproduction:** `<filter id='f'><feMorphology
  radius='1e49'/></filter><rect width='8' height='8' filter='url(#f)'/>`
  (seed `67bfa887…` in `fuzz/seeds/svg/`). Found by a local `svg` fuzz run
  on 2026-11-09.
- **Local workaround:** `svg_bounds::check_values` refuses a radius that
  isn't finite as `f32` (`SvgError::OutOfRange`); a finite radius counts in
  the region factors of U31's workaround (ADR 0042). Drop both once usvg
  checks.
- **PR sketch:** treat a non-finite product like a negative radius (the
  primitive's default, or a transparent result as the spec says for a
  disabled effect) instead of unwrapping; `PositiveF32::new(…).unwrap_or(…)`.

## U31 — tiny-skia-path: `NonZeroRect::bbox_transform` unwraps an overflowed rect

- **Project:** linebender/tiny-skia, crate tiny-skia-path. Affected:
  0.12.0 (via usvg 0.49.0).
- **Problem:** `rect.rs` `bbox_transform` (line 691) multiplies the rect by
  the bounding box and unwraps `NonZeroRect::from_xywh`, which is `None`
  when a side overflows to infinity. usvg calls it for every
  `objectBoundingBox` region; the default filter region (−10%, 120%)
  overflows on a box wider than ~2.8·10³⁸.
- **Reproduction:** `<filter id='f'><feOffset/></filter><rect width='3e38'
  height='8' filter='url(#f)'/>`. Found while probing the U30 workaround on
  2026-11-09.
- **Local workaround:** ADR 0042: `svg_boxes::largest_box` converts a
  copy of the document that reaches no `bbox_transform` (filters, masks,
  clip paths off, paints plain, `defs`/`mask`/`clipPath`/`pattern` content
  renamed to `g`) and reads usvg's boxes; `svg::check_regions` refuses
  (`SvgError::RegionOverflow`) when the largest box × the largest region
  factor could overflow. Drop it once tiny-skia-path or usvg checks.
- **PR sketch:** return `Option<Self>` from `bbox_transform` (a breaking
  change), or a checked variant that usvg's callers use, dropping the
  element as they do for other invalid regions.

## U32 — resvg: box blur width overflows `i32`

- **Project:** linebender/resvg, crate resvg. Affected: 0.49.0.
- **Problem:** `filter/box_blur.rs` `create_box_gauss` computes the ideal
  box width `sqrt(12σ²/5) + 1` in `f32`, casts it to `i32` (saturating at
  `i32::MAX` once σ² is infinite, from σ ≈ 1.8·10¹⁹; the cast alone reaches
  `i32::MAX` from σ ≈ 1.4·10⁹) and adds 2 unchecked (line 48). Overflow
  checks panic; release wraps to a negative box, which `apply` turns into a
  huge `usize` radius [reading the code]. σ is the deviation × the
  transform's scale (`resolve_std_dev`), so a `viewBox` can grow it.
- **Reproduction:** `<filter id='f'><feGaussianBlur
  stdDeviation='1.4e31'/></filter><rect width='40' height='40'
  filter='url(#f)'/>` (seed in `fuzz/seeds/svg/`, bug_0198). Found by the
  nightly `svg` fuzz job on 2026-11-09.
- **Local workaround:** `svg::check_extent`'s walk refuses a deviation
  past `MAX_SVG_BLUR` (2²⁰ px at the drawn size; `SvgError::TooBlurred`).
- **PR sketch:** clamp σ to the image's larger side (a wider blur gives the
  same result: the loops already stop at the image size), or compute the
  box sizes in `i64` / with `saturating_add`.

## U33 — bevy_picking: ray picking needs a primary window, even for image targets

- **Project:** bevyengine/bevy, crate bevy_picking. Affected: 0.20.0.
- **Problem:** `pointer.rs` `Location::is_in_viewport` normalizes the
  camera's `RenderTarget` against `primary_window.single()` and returns
  `false` when there is no primary window, before comparing targets. An
  image target needs no window to normalize, so a pointer located on an
  image (a `ViewportNode`'s forwarded pointer, or the mouse of an app
  rendering offscreen) never gets a ray (`RayMap::repopulate`), and mesh
  and sprite picking hit nothing.
- **Reproduction:** the `canvas` example under `--offscreen` (no winit, no
  window): the forwarded pointer reaches the portrait image at the right
  position (`viewport_picking` works), but `RayMap` stays empty for every
  pointer. Found while playtesting ADR 0041's example on 2026-11-09.
- **Local workaround:** none needed in the library. The examples' harness
  (`examples/shared/harness.rs`) gives its `--offscreen` composition a
  primary `Window` entity with no native window (without winit it never
  gets a `RawHandleWrapper`, so the renderer never extracts it): dragging
  and clicking the `canvas` example's hero then work offscreen. The test
  harness has such a window too (`tests/canvas.rs`
  `pointer_input_reaches_the_cameras_scene`). Drop the harness's window
  once Bevy checks the target first.
- **PR sketch:** normalize with `primary_window.single().ok()` and let
  `RenderTarget::normalize` fail only for `WindowRef::Primary` targets.

## U34 — bevy_ui: UI picking never hits inline boxes

- **Project:** bevyengine/bevy, crate bevy_ui. Affected: 0.20.0.
- **Problem:** `picking_backend.rs` resolves a text node's hit to a span by
  `pick_ui_text_section` over `TextLayoutInfo::run_geometry`. Inline boxes
  (`InlineBox` children: `<img>`/inline `<svg>` images in bevy_markup) are
  laid out in `TextLayoutInfo::inline_boxes` (entity, kind, rect) but never
  hit-tested, so the pointer over one hits the text node itself.
- **Reproduction:** `<p><a href="next.html"><img src="frame.png"></a>
  text</p>`, click the image: the hit is the paragraph's
  (`signals::an_image_inside_a_link_opens_it` passes through the
  workaround).
- **Local workaround:** `signals::pick_inline_boxes`, a picking system
  after the backends: for a hit on a text node, it tests the hit's point
  (normalized position × node size, relative to the content box) against
  `inline_boxes` and adds a hit on a bound, pickable box's entity just in
  front of the text node (bug_0199). Drop it once Bevy hit-tests inline
  boxes.
- **PR sketch:** in `pick_ui_text_section`'s caller, test
  `inline_boxes`' rects before the runs and report the box's entity.

## U35 — tiny-skia-path: `to_int_rect` unwraps an overflowing rect

- **Project:** linebender/tiny-skia, crate tiny-skia-path. Affected:
  0.12.0 (via resvg 0.49.0).
- **Problem:** `NonZeroRect::to_int_rect` (`rect.rs:700`) and
  `IntSize::to_int_rect(x, y)` (`size.rs:87`) unwrap `IntRect::from_xywh`,
  which checks `x + width` in `i32`. resvg's `filter/mod.rs` converts the
  filter region and every primitive subregion with the first, and
  `apply_offset`/`apply_drop_shadow` position a `draw_pixmap` (the second)
  at the scaled offset saturated to `i32`.
- **Reproduction:** `<filter id='f' primitiveUnits='objectBoundingBox'>
  <feFlood width='4e5'/></filter><rect width='6000' height='10'
  filter='url(#f)'/>`; or `<feOffset dx='1e8'/>` under `scale(2e2)`.
  Found by bevy_markup's `hostile_magnitudes_never_panic` proptest,
  2026-11-09.
- **Local workaround:** `svg::check_extent` refuses regions, subregions
  and offsets reaching past `MAX_SVG_FILTER_REGION` (2²⁴ px).
- **PR sketch:** return `Option<IntRect>` (or clamp) and let resvg treat
  an unrepresentable region as empty, as it does for invalid ones.

## U36 — resvg: pattern tiles are allocated without a limit

- **Project:** linebender/resvg. Affected: 0.49.0 (tiny-skia 0.12.0).
- **Problem:** `path.rs` `render_pattern_pixmap` sizes the tile pixmap as
  the pattern rect × the scale of the path's transform with the
  pattern's, rounded, and creates it with `Pixmap::new`, which allocates
  `vec![0; len]` (tiny-skia's own note: it can't check the allocation).
  A tile of 10⁶ px a side asks for 4 TB and aborts the process.
- **Reproduction:** `<pattern id='p' width='1e4' height='1e4'
  patternUnits='userSpaceOnUse'><rect width='1' height='1'/></pattern>
  <rect width='8' height='8' fill='url(#p)' transform='scale(100)'/>`.
  Found by bevy_markup's `hostile_magnitudes_never_panic` proptest,
  2026-11-09 (133 TB).
- **Local workaround:** `svg::estimate_cost` charges each tile at its
  drawn size, so such a document exceeds the cost budget and is refused.
- **PR sketch:** cap the tile at the canvas size (a larger tile shows at
  most the canvas) or use `try_reserve` and skip the paint on failure.

## U37 — parley 0.11: tall inline boxes get negative leading

- **Project:** linebender/parley. Affected: 0.11.1 (Bevy 0.20's bevy_text
  depends on `^0.11`). Fixed in 0.12.0 (2026-10-09): line boxes are
  computed CSS-style from baseline-relative extents, with
  `vertical-align`.
- **Problem:** `layout/line_break.rs` `finish_line` sets
  `ascent = max(ascent, box.height)` for an in-flow inline box and takes
  the line height from `running_line_height` (the tallest cluster or
  box). For a box taller than the line height, `leading = line_height −
  (ascent + descent)` is −descent: the box rises by half the descent and
  the text's descent is missing from the line.
- **Reproduction:** `<p>Tall <img src="frame.png" style="…"> icon</p>`
  with `html { font-size: 20px }` and the image 20 × 40: Chromium's
  paragraph is 45px with the image at its top; Bevy's is 40px with the
  image at −3 (`a_tall_inline_image_grows_the_line_like_css`, ignored).
- **Local workaround:** none (Bevy places the box from parley's metrics).
  The `layout_svg` vector keeps its images below the font's ascent.
- **On a Bevy bump:** if bevy_text depends on parley ≥ 0.12, run
  `cargo test --test html_ui -- --ignored a_tall_inline_image`, un-ignore
  it, add a tall image to `layout_svg` (and re-record it), and mark this
  entry removed.

## U38 — bevy_ui: a viewport camera's missing target image panics

- **Project:** bevyengine/bevy, crate bevy_ui. Affected: 0.20.1.
- **Problem:** `widget/viewport.rs` `update_viewport_render_target_size`
  resizes the camera's target with `images.get_mut(image_handle)
  .unwrap()` (line 188). A camera whose `RenderTarget::Image` handle has
  no image in `Assets<Image>` (reserved, still loading, or removed)
  panics.
- **Reproduction:** a `ViewportNode` (here a `<canvas>` with
  `CanvasSource::Camera`) over a camera targeting a reserved handle:
  `called Option::unwrap() on a None value` on the first frame
  (review_0007 K15, bug_0218).
- **Local workaround:** `canvas.rs` `sync_canvases` binds a camera's
  viewport only while its target image is in `Assets<Image>` (bug_0218,
  `tests/canvas.rs` `a_camera_without_its_image_waits_for_it`). Drop the
  check once Bevy skips a missing image.
- **PR sketch:** skip (or warn once) when `images.get_mut` returns
  `None`, as the render-target extraction does for missing images.

## U39 — Tera: no limit on a render's work or memory

- **Project:** Keats/tera. Affected: 2.4.0.
- **Problem:** the interpreter bounds `range` (`MAX_RANGE_LEN`, 10⁵) and
  component recursion (20) but nothing else: instructions run without a
  budget, `~` concatenation, filters (`replace`, `join`, `indent`) and
  captures allocate without a cap, and a list nested in itself (`[a, a]`,
  shared `Arc`s) is exponential when written. A failed allocation aborts
  the process.
- **Reproduction** (release, 2026-11-11): `{% set_global s = "x" %}{% for i
  in range(end=24) %}{% set_global s = s ~ s %}{% endfor %}` builds 16 MB in
  5 ms (40 iterations: 1 TB); two nested `range(end=100000)` loops with an
  empty body run 10¹⁰ iterations.
- **Local workaround:** `src/tera_check/` (ADR 0044) ports Tera's lexer and
  parser and refuses, at load, a template whose bounds pass the limits;
  `template::MAX_RENDERED_BYTES` caps the output. A vendored Tera with an
  instruction budget and a value cap was tried and dropped (~10% render
  cost; `[patch]` doesn't reach dependents).
- **PR sketch:** an optional render budget on `Tera` (instructions, bytes
  written, largest value), checked in the interpreter loop with batched
  counters; and expose the parser (`parsing::parser`) so checks like ours
  don't have to port it.

## U40 — simplecss: descendant selectors match in exponential time

- **Project:** linebender/simplecss. Affected: 0.2.2 (used by usvg 0.49's
  `resolve_css` and by `svg_bounds::Graph::new`).
- **Problem:** `Selector::matches_impl` (`selector.rs:174`) matches a
  descendant combinator by trying each ancestor in turn and recursing
  into the rest of the selector, with no memo. When the leftmost
  compound never matches and the others always do, an element `d` deep
  costs Σ_{j≤k} C(d, j) calls for k descendant combinators (the count is
  exact: `svg_bounds::tests::descendant_calls_count_simplecss_backtracking`
  replays it).
- **Reproduction** (release, 2026-11-12): one `<style>` rule
  `q g g g g g g` over 60 nested `<g>`s around a `<rect>` (553 bytes):
  usvg's conversion takes 5.2 s; `q g g g g g g g` (555 bytes) 35.7 s
  (review_0008 L07, bug_0229).
- **Local workaround:** `svg_bounds::match_work` charges every rule by
  that count over the document's depth histogram, times its class,
  attribute and pseudo-class tests, into `MAX_SVG_WORK` before anything
  matches (bug_0229, bug_0231).
- **PR sketch:** for a selector of descendant combinators only, match
  greedily (the nearest ancestor matching a compound is always the best
  choice, so no backtracking is needed); with child combinators mixed
  in, memoize (compound index, element) failures for the duration of
  one `matches` call. Either makes a match O(depth × compounds).

## U41 — usvg: text layout is quadratic in a chunk's characters

- **Project:** linebender/resvg, crate usvg. Affected: 0.49 (`text`
  feature).
- **Problem:** `ByteIndex::code_point_at` (`text/layout.rs:1871`)
  converts a byte offset to a character index by counting
  `char_indices()` from the start of the chunk's text, and
  `resolve_relative_shifts` (`layout.rs:563`, run for every chunk) calls
  it once per cluster, as do the rotation (`layout.rs:610`) and
  text-on-path (`layout.rs:716`) code. A chunk of n characters costs
  O(n²); multi-byte characters decode slower (×3.4).
- **Reproduction** (release; Bevy's default font; one `<text>` at 10 px;
  conversion only): 20 000 `M` → 0.17 s, 40 000 → 0.65 s, 100 000 →
  3.93 s; with a character the font lacks, 0.55 s and 13.5 s. 20 000
  one-letter `<text>` elements take 0.10 s (review_0008 L10, bug_0232).
  Located by reading (no profiler on this machine); the doubling
  measurements fit n².
- **Local workaround:** `svg_bounds::Graph::text_work` charges a `<text>`
  by its characters² / 32 (a chunk is at most the whole element) into
  `MAX_SVG_WORK`.
- **PR sketch:** compute each cluster's code-point index once, with a
  single pass over `char_indices()` building a byte → code point map
  (or walk clusters in byte order with a running count).

## U42 — simplecss: parsing a style sheet is quadratic in its length

- **Project:** linebender/simplecss. Affected: 0.2.2.
- **Problem:** errors carry a `TextPos` computed eagerly by
  `Stream::gen_text_pos`, which counts the lines from the start of the
  text (`stream.rs:281`). The parser raises (and drops) such errors on
  ordinary input: `consume_declaration` ends every declaration with a
  failing `consume_term` (`InvalidIdent` at the `;` or `}`), a number
  without a unit tries `consume_ident` for its suffix, a `#` colour
  starting with a digit too. Each costs O(offset), so a sheet costs
  O(length × declarations).
- **Reproduction** (release, `StyleSheet::parse_more`, N rules of
  `rect { fill: red }`): 5 000 → 16.5 ms, 10 000 → 61.5 ms, 20 000 →
  245 ms, 40 000 → 962 ms. usvg parses each `<style>` again, and
  bevy_markup's box pass a third time (review_0008 L09's 3.25 s,
  bug_0231).
- **Local workaround:** `svg_bounds::style_scan_bytes` sums the offsets
  of the bytes where those errors arise (`:`, `{`, `#`, the start of a
  digit run) and charges them into `MAX_SVG_WORK` before simplecss
  parses the text.
- **PR sketch:** store the byte offset in the error and compute the
  row/column only in `Display` (or when asked); or have
  `consume_term`/`consume_ident` peek instead of erroring on the normal
  end of a value.

## U43 — usvg: each text span reshapes the whole chunk

- **Project:** linebender/resvg, crate usvg. Affected: 0.49 (`text`
  feature).
- **Problem:** `process_chunk` (`text/layout.rs:897`) shapes the chunk's whole
  text once per span (each `<tspan>` or text piece with its own font),
  then overwrites the first run's glyphs with the span's by looking each
  one up with `glyphs.iter().position(…)` and inserting or removing with
  `Vec::insert`/`remove`. A chunk of n characters in s spans costs s
  shapings of n characters plus up to n² for the merge.
- **Reproduction** (release; Bevy's default font; one `<text>` of n
  one-character `<tspan>`s alternating two font sizes): 1 000 → 0.12 s,
  2 000 → 0.48 s, 4 000 → 1.93 s; the same characters in one span take
  1.4–8.4 ms (bug_0232).
- **Local workaround:** `svg_bounds::Graph::text_work` charges a
  `<text>` by its spans × characters × 2 into `MAX_SVG_WORK`.
- **PR sketch:** shape each span's own byte range where the bidi runs
  allow it, or shape once per distinct font rather than per span; merge
  through a byte-index map instead of a linear search.

## Not upstream bugs (recorded so they aren't re-filed)

- **Bevy's text measure rounds up** (`TextMeasure::measure` ends in
  `.ceil()`): a deliberate pixel-snapping choice, not a defect; it makes
  layouts with fractional line heights drift from browsers. Layout vectors
  use whole line heights.
- **`rui314/setup-mold` has no effect on Rust builds:** rustc ≥ 1.90 links
  through its bundled `rust-lld` and bypasses `/usr/bin/ld`.
