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
| U1 | fluent-syntax (fluent-rs) | Invalid unicode escape before a multi-byte character panics ([bug_0005](bug_0005.md)) | panic from any FTL asset | fork (git patch) | fix on fork; upstream PR pending |
| U2 | fluent-syntax (fluent-rs) | Unbounded expression nesting overflows the stack ([bug_0015](bug_0015.md)) | stack overflow (abort) from a few KB of FTL | fork (git patch) | fix on fork; upstream PR pending |
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

## Not upstream bugs (recorded so they aren't re-filed)

- **Bevy's text measure rounds up** (`TextMeasure::measure` ends in
  `.ceil()`): a deliberate pixel-snapping choice, not a defect; it makes
  layouts with fractional line heights drift from browsers. Layout vectors
  use whole line heights.
- **`rui314/setup-mold` has no effect on Rust builds:** rustc ≥ 1.90 links
  through its bundled `rust-lld` and bypasses `/usr/bin/ld`.
