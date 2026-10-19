# review_0006 — Code review 2026-10-16 (the windowed build, ADR 0031)

| field | value |
|---|---|
| ID | review_0006 |
| Date | 2026-10-16 |
| Scope | ADR 0031's windowed build: `59f8f32`..`bd99546` (`HtmlWindowed`, `WindowModel`, `measure_window_heights`, the cull in `collect_node`/`collect_items`, `coalesce_spacers`, the count-mismatch replacement in `update_children`, the testbench's `--windowed`/`--scroll`), and the open bug_0151 |
| Method | the diff read against the ADR; layout-harness probes (temporary tests, since removed) and reproduction tests comparing a windowed UI with an unwindowed build of the same document at the same scroll offset; the testbench run offscreen through the MCP harness at `bd99546` and with the fix, screenshots compared |
| Focus | bug_0151's root cause; correctness, performance and the ADR's claims |
| Findings | `J01`… (stable ids) |
| Discovery agent | Claude Code session, `claude-opus-5-5` |

J01 (bug_0151) is fixed in this session; J02, J03, J08 and J09 in the
first follow-up, J04 and J05 in the second (see **Fixes**). Every finding has a reproduction test that
fails at `bd99546` (`review_0006::*` in `tests/html_ui.rs`).

## Summary

bug_0151 has a simple cause. Coalescing a run of k culled siblings into
one spacer drops the k − 1 row gaps between them. The cursor that
decides the band counts those gaps; the laid-out spacer doesn't. So
every materialized row sits (k − 1) × gap above where the band was
computed for. With the testbench's 2 px root gap, 5 500 culled rows put
the band 11 000 px above the viewport, the figure the bug report caught.
The fix makes a spacer remember its column's gap and adds one gap per
merge. The earlier attempts fixed other things (pixel units,
re-measurement) and couldn't touch it. The probes checked the model's
total against the laid-out content, and both lacked the same gaps
(200 000 + 66 × 2 = 200 132). The vector had no gap and checked which
rows exist, never where they sit.

The rest of the review: windowing works only for a flat top-level
document. Content inside any container (`<main>`, a `<section>`)
materializes in full, because only the root's direct children are ever
measured. A root with padding or a border (and any HiDPI display)
clears the model and rebuilds every frame. And scrolling never reuses
the band: each scroll frame re-collects every materialized row from
scratch, and every sub-pixel step rebuilds.

## Findings

**J01 · high · The band drifts above the viewport while scrolling (bug_0151)** — `ec085b1` [probe] — **fixed**
- `coalesce_spacers` summed the merged members' heights. A column puts
  one gap after a node, so k culled siblings lose k − 1 gaps once merged.
  The cursor (`collect_items`' root loop, `collect_node`'s `ys`) adds
  `outer + gap` per row. The materialized rows land
  `(culled above − 1) × gap` higher than the band was computed for.
- A second, smaller error in the same cursor: the root loop advanced by
  `win.frames.borrow().last()`'s gap. That is the root's only until the
  first materialized container pushes its own frame; after that it was
  the row's `gap: 8px`, not the root's 2 px. The band's lower edge then
  falls short; a one-viewport margin hides it.
- **Probe:** a windowed UI and an unwindowed one, 300 one-line
  paragraphs, `gap: 4px`, both scrolled to 2 400 px: all 26 materialized
  rows sit 360 px above their place (90 gaps × 4 px). The testbench at
  `bd99546` (`big-document --size 10000 --no-churn --windowed --scroll`,
  offscreen) shows an empty stage mid-sweep, the owner's symptom. The
  scripted sweep does reproduce it.
- **Fix:** `Item::Spacer { height, gap }` (the culling frame's gap),
  `coalesce_spacers` adds one gap per merge, and the root loop reads the
  root frame's gap. The fixed sweep renders rows throughout, and the
  content extent becomes 219 998 px (10 000 rows + 9 999 gaps; it read
  200 132 at `bd99546`).
- **Tests:**
  - `windowed_rows_sit_where_the_full_build_puts_them`,
    `windowed_bench_rows_track_the_scroll_at_every_depth` and
    `a_hover_restyle_keeps_the_band`: every materialized row sits within
    1 px of the unwindowed build, and every row in the viewport is
    materialized. Each fails without the coalescing fix.
  - `a_marginless_band_still_covers_the_viewport` fails without the
    root-gap fix.
- The second symptom (hovering changes the visible count) wasn't
  reproduced separately; the hover test passes with the fix.

**J02 · high (perf) · Nothing inside a container is ever culled** [probe]
- `measure_window_heights` looks the model up on the measured entity's
  *parent* (`models.get_mut(parent)`, a query `With<HtmlUi>`). Only the
  root's direct children find it. For every deeper keyed entity it
  inserts a `WindowModel` on the parent container instead
  (`commands.entity(parent).insert(WindowModel::default())`), and the
  height is never recorded.
- So a container's children have no heights, its cursor frame
  invalidates at the first child, and nothing inside it culls. The ADR's
  nested cursor frames (`frame_for`, the `ys` walk) never cull anything.
  Most real pages wrap their content (`<main>`, a `<section>`, a
  `<div class="list">`). For them windowing is a no-op that still pays
  the measuring and the stray components.
- **Probe:** 600 rows inside `<main>`: 1 801 of 1 801 elements
  materialized. The same rows at the top level: 72. One band at the top
  level leaves 25 row containers carrying a `WindowModel`; inside
  `<main>`, 602.
- **Tests:** `content_inside_a_container_is_windowed`,
  `only_the_windowed_root_carries_a_window_model`.
- **Fix direction:** walk up to the windowed root (the nearest
  `HtmlUi` ancestor) for the model. Bail out when it isn't windowed;
  never insert on an arbitrary parent.

**J03 · high (perf) · A root with padding or a border, or a HiDPI display, rebuilds every frame and culls nothing** [probe]/[code]
- The width anchor mixes units. The build compares
  `computed.size.x` (the root's **border box**, **physical** pixels)
  with `model.width` (`content_width`: border and padding subtracted,
  times `inverse_scale_factor`, so a **logical content** width). They
  agree only for a root with no padding and no border at scale factor 1.
  Otherwise `window_changed` is true every frame (a rebuild every frame)
  and `model.clear()` runs every build (nothing culls).
- The same mix elsewhere: the band's `viewport = node.size.y` is
  physical while the scroll offset is logical. The measurement also
  multiplies the logical margins by `inverse_scale_factor`
  (`(margin_top + size.y + margin_bottom) * inverse_scale_factor`). So
  `77086c8`'s HiDPI fix is incomplete, and the CHANGELOG's "Fixed" entry
  overstates it.
- **Probe:** `html { padding: 8px }` on a windowed root: 30 builds in 30
  idle frames, and the dump fully materialized.
- **Test:** `a_padded_windowed_root_settles_and_culls`. HiDPI isn't
  testable in the harness (scale 1); [code].
- **Fix direction:** one unit system (logical) and one box (content)
  for the anchor, the viewport and the heights; compare like with like.

**J04 · medium (perf) · Scrolling never reuses the band** [probe]
- A scroll-only build is a `Decision::Build` with an unchanged document.
  `skeleton_eligible` requires `rendered.is_changed() || restyle`, so it
  never pairs: every scroll frame re-cascades and re-collects every
  materialized row, and `update_children` diffs them all. ADR 0031 §1
  ("reuse-before-cull") and the CHANGELOG's "scrolling rebuilds at band
  cost" describe this cost, but the rows that stay in the band don't
  have to pay it: the skeleton holds their specs.
- **Probe:** a one-row scroll: `last_reuse() = (0, 74)`. The testbench's
  `reuse` check fails under `--windowed` (0 of 248).
- **Test:** `scrolling_by_one_row_reuses_the_band`.

**J05 · low (perf) · Every sub-pixel scroll rebuilds** [probe]
- `window_changed` compares the scroll offset exactly
  (`scroll.y != rebuild.window_scroll`). A smooth or trackpad scroll
  rebuilds every frame even when the band's membership doesn't change.
  The band has a viewport of margin on each side, so a rebuild is
  needed only when the offset crosses a row boundary at the band's
  edges.
- **Probe:** a 0.5 px scroll: one rebuild, the same band.
- **Test:** `a_scroll_inside_the_band_does_not_rebuild`.

**J06 · medium · The tests checked existence, never position** [code]
- `windowed_build_materializes_the_band_and_spacers_the_rest`:
  - Its root has no `gap`, the one setting that exposes J01.
  - It checks which paragraphs exist, never where they are laid out.
  - Its "height invariant" (`assert_eq!(100.0 + 500.0 + 30.0 * 20.0,
    60.0 * 20.0)`) compares constants with each other.
- `mouse_wheel_scrolls_a_windowed_ui` asserts only that a spacer exists.
- The CULLPROBE diagnostic compared the model's total with the laid-out
  content. Both lacked the same gaps, so it read "consistent".
- **The method:** compare a windowed UI with an unwindowed build of the
  same document at the same scroll offset, by position (the J01 tests'
  `band_errors`). An incremental path checked against itself shares its
  bugs.

**J07 · low · ADR 0031 and the docs describe behavior the code doesn't have** [code]
- §3 "focus navigation to an off-window element materializes it first":
  nothing in `focus.rs` reads the window (the `HtmlWindowed` doc says
  focus stays inside the window, so the doc is right and the ADR wrong).
- §5 "off … in headless test harnesses": not implemented (harmless; the
  tests rely on it being on).
- The ADR's measured win (17.2 vs 77.9 ms) holds for a flat top-level
  document only (J02).
- AGENTS.md says only *newly added* keys are measured; `5f8e673` also
  re-measures on `Changed<ComputedNode>`.

**J08 · medium (perf) · A windowed UI stays fully built until it is scrolled** [probe] — found fixing J03
- The first build has no heights and builds everything. The measurement
  fills the model, but only a scroll or a width change triggers another
  build. So a long windowed document built every block until the user
  first scrolled.
- **Test:** `a_windowed_ui_culls_without_being_scrolled`.

**J09 · medium · CI's `cargo test --features text-input` was red since `ec085b1`** [probe]
- The layout harness added `UiWidgetsPlugins`, whose `TextInputPlugin`
  `BevyMarkupPlugin` also adds under `text-input`. Every layout-harness
  test panicked ("plugin was already added", ~70 tests).

## Refuted

- **The count-mismatch replacement (`5f8e673`) respawns every text block
  each scroll** (text runs are child entities): entity ids are stable
  across a 1 px scroll (72 of 72 kept, and 28 of 28 for multi-run
  blocks).
- bug_0151's hypotheses: (1) a stale-band path in the wheel and restyle
  interleaving, (2) the `ScrollArea` clamp, (3) a second unit error in
  the real-window path. None was the cause. The scripted sweep
  reproduces the bug without a wheel, a pointer or a restyle. (3) is
  real (J03) but needs padding or a scale factor ≠ 1, and the owner's
  run had neither.

## Triage

1. J01 (done).
2. J02 and J03: windowing is a no-op, or worse (a rebuild every frame),
   for most real pages. Fix both before relying on the feature.
3. J04 and J05: scrolling cost.
4. J06's method for every windowing test; J07's records.

## Fixes (2026-10-16, same model)

| Finding | Bug | Fix |
|---|---|---|
| J01 | bug_0151 | spacers keep their column's gap; the root cursor reads the root frame's gap |
| J02 | bug_0152 | the measurement walks to the nearest `HtmlUi` and skips unwindowed UIs; the container cursor's double `margin-top` removed |
| J03 | bug_0153 | logical pixels of the root's content box throughout (`logical_content_width`, `visible_region`); only the measured size converted |
| J08 | bug_0154 | `WindowModel::changed` (a new or moved height) triggers one rebuild |
| J09 | bug_0155 | the harness adds only `ScrollAreaPlugin` |
| J04 | bug_0156 | scroll builds pair with the skeleton; a cached subtree reuses only without spacers and wholly inside the band; an unchanged document keeps the model's keys; scroll builds keep the strike state |
| J05 | bug_0157 | the cull decisions record the offset slack; the UI rebuilds when the offset leaves it, or the viewport's width or height changes |
| J06 | — | the original vector checks the laid-out content height and a paragraph's position; the wheel test checks the top paragraph's position; the testbench's `--windowed` adds a `band` check (the built rows cover the viewport; fails at frame 62 with bug_0151 put back) |
| J07 | — | ADR 0032 amends 0031: §3 (focus beyond the band) and §5 (headless off) dropped from the contract, the measured win re-taken with real layout (85.9 → 23.0 ms idle, 81.5 → 21.5 ms scrolling, 2000 rows, offscreen) |
| — | bug_0158 | window keys only on windowed UIs (`update_spec` keeps them in step; keyless specs don't reuse in windowed builds) |
| — | bug_0159 | the testbench's `--size` pins the size (`--exit-after`'s injected keys stepped it) |

Each fix part was falsified against its test. The J03 HiDPI test runs
the harness's camera at scale 2. `windowed_build_materializes_the_band_and_spacers_the_rest`
now expects the first build to cull (J08).

J05's `note_reused` (the slack around a subtree reused whole) binds only
when nothing sits between the band's edge and the subtree in its column.
Any neighbour there is tighter by one gap. Its test uses the first child
of a container whose top padding holds the band's edge
(`a_reused_section_culls_its_rows_once_they_leave_the_band`). It fails
without the note: the section's first row stays built above the band.

All findings are fixed; the design changes are recorded in ADR 0032.

## Follow-up: the windowing property (2026-10-16, same model)

`tests/windowing_model.rs` compares a windowed build with an unwindowed
one of the same document, as a property: random documents (paragraphs
of three heights with optional margins, nested padded and bordered
sections, loose text, tall blocks, rows under `{% if f %}`) × random
steps (text changes that may wrap, row flips, scrolls). Both are stepped
in lockstep and checked in every frame: placement within 1 px, coverage
of the viewport, no two entities sharing a `WindowKey`. Tightness (nothing
built outside the band) is checked once settled. Text changes only, then
row flips, then per-frame checks each found new defects:

| Bug | Found by | Fix |
|---|---|---|
| bug_0160 | text changes | raw text pairs recorded in `pairing`/`reverse`/`clean` |
| bug_0161 | text changes | `Ctx::cull` culls only subtrees clean in this build's pairing |
| bug_0162 | text changes | a refused cull flags the band stale: one rebuild |
| bug_0163 | text changes | `WindowModel::clear` sets `remeasure` |
| bug_0164 | row flips | `visible_scroll` clamps the offset to the last layout's range |
| bug_0165 | row flips | the nested cursor borrows its frame (`CursorFrame` not `Clone`) |
| bug_0166 | row flips | a remap that drops heights sets `remeasure` |
| bug_0167 | tracing bug_0166 | a windowed UI always pairs (the adaptive gate skips it) |
| bug_0168 | measuring bug_0166 | fixed 2026-10-19 (ADR 0033): siblings align by common head and tail; a new element's height is estimated from its siblings |
| bug_0169 | per-frame checks | the cached skip refreshes the `WindowKey` with the `DomNode` |
| bug_0170 | per-frame checks | open (known limit): rows above the band that change height shift the view for one frame |
| bug_0171 | the bug_0169 audit | opaque marks kept in step both ways, a flip is news, `Changed<Node>` re-measures |

| bug_0172 | the mutation run | no root unwrap: `<body>` keeps its style and selectors (every UI) |
| bug_0173 | the mutation run | the measurement clears the model on a width change |
| bug_0174 | the mutation run | `%` margins read the parent width without requiring `ChildOf` |
| bug_0175 | the mutation run | walked-through elements advance the cursor by their children (`column_height`); itemless nodes by zero |
| bug_0176 | the property (new `%` margins) | heights from `unrounded_size` |
| bug_0177 | the mutation rerun | one `children_are_opaque` predicate on the declared `Node` (reversed directions count), the root checked too; the measured opaque marks removed |

The mutation run (cargo-mutants over `git diff 902d81c` of `src/build.rs`,
323 mutants; lib + html_ui + windowing_model at 16 cases) caught 211,
timed out 20 (rebuild-every-frame mutants: `settle` never stabilizes) and
missed 70. Each missed group got a test or a reason:
- new tests: the root's box, scale and scrollbars at margin 0 with a
  tightness check (`the_root_box_and_scale_bound_the_band_exactly`, which
  also caught bug_0164's first scrollbar term), a width change (bug_0173),
  `%` margins and boxed paragraphs in the property's generator (bug_0174,
  bug_0176), a shrinking visible row, table rows (a `tr` is a row without
  CSS saying so), a whole document (bug_0172, bug_0175), a restyle into a
  grid or a row (frame-0 coverage);
- dead code removed: `frame_for`'s `margin_top`/`border_padding_top`
  (every caller passed constants);
- equivalent: `WindowModel::probe` (testbench diagnostics); the adaptive
  gate's arithmetic (non-windowed performance only); the
  `rendered.is_changed()` remap/clear guards (redundant since a windowed
  UI always pairs and raw text pairs); `> 0.5` → `>=` thresholds; the
  spacer's `pickable` field; the remap's `<` → `<=`; the slack's `-` → `/`
  (a smaller slack only rebuilds earlier); the declared-layout opaque
  checks for wrap (the measurement's mark covers it a frame later);
  dropping the clamp's horizontal-scrollbar term (it moves the band's
  edge only under that scrollbar); the text-input item's key (needs
  `--features text-input`; not windowed in any test).

A rerun on the fixed code (330 mutants: 215 caught, 24 timeouts, 24
unviable, 67 missed) showed which new tests didn't reach: the table test
passed with the opaque marks off (bug_0177's two checks duplicated each
other), bug_0166's regression had gone quiet (re-minimized from a new
seed), kept elements could be replaced on every build unnoticed (now
`rows_that_stay_in_the_band_keep_their_entities`), `note_reused`'s bottom
term (`a_reused_section_culls_its_rows_once_they_leave_the_band_bottom`),
the clamp's and the visible region's top-border terms (the root-box test
now has a 20 px top border) and the walk-through sum (a walked-through
group with rows after it). The build-side width check, redundant with the
measurement's (bug_0173), was removed. Also equivalent: a one-sided
`auto` margin's `&&` → `||` (auto margins are 0 in an overflowing column),
`item_shell_reactive` (ADR 0029's restyle scoping, in the diff's context
only), `column_height`'s depth arithmetic (only the recursion bound) and
`is_walked_through` → `true` (a non-walked node without a height has no
measured children either).

The final run (318 mutants: 227 caught, 19 timeouts, 24 unviable, 48
missed) left the equivalents above plus three gaps, now closed: the
slack's materialized terms (`every_row_that_leaves_the_band_culls_at_once`:
a tall block holds the far edge so only the leaving rows bind, 1 px
tightness), the `positioned` mark (`a_fixed_element_survives_its_section_leaving_the_band`)
and the restyle into a row seen in view (the restyle test ends at the
top). Also equivalent: the `changed` flag's `(outer - previous) > 0.5`
arithmetic (an always-true flag only adds a rebuild after a measurement)
and the slacks' `1e-3` epsilon variants.

Every fix was ablated against the suite (the property at 128 cases plus
the regression tests) and fails it. Each found seed has a minimized
regression test. `Changed<WindowKey>` in the measurement's filter survives
the ablation: every handle shift found also changes the kept entities'
`ComputedNode::stack_index`. It stays as defense in depth. The property
passes 400 cases (release, 83 s).
