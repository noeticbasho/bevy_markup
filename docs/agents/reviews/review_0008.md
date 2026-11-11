# review_0008 — Code review 2026-11-12 (the crate at `54bad82`: review_0007's fixes, the template and FTL checks)

| field | value |
|---|---|
| ID | review_0008 |
| Date | 2026-11-12 |
| Scope | the four commits since [review_0007](review_0007.md) (`174eda6`..`54bad82`, ~8 000 added lines): the fixes for K01–K17 (bug_0204–0221), the new load-time checks `src/tera_check/` (ADR 0044) and `src/ftl_check.rs` (ADR 0045), the `CanvasPicture` child (ADR 0043), the windowing spacer change (bug_0222) |
| Method | one session: CI's exact command list (`ci.yml`'s env) at `54bad82`; the diff read whole, `tera_check`'s analysis read against Tera 2.4's source, the SVG changes against simplecss 0.2.2 and tiny-skia's dasher; 22 behaviour probes (temporary tests, since removed), timed in release builds; `windowing_model` at 400 cases |
| Focus | regressions from the fixes, whether each fix closes its finding, and whether the new bounds bound what ADR 0044 and the README say they do |
| Findings | `L01`… (stable ids) |
| Discovery agent | Claude Code session, `claude-fable-5-2` |
| Machine | Arch Linux (kernel 7.2.14), AMD Ryzen 7 9800X3D, rustc 1.98.1 |

Nothing is fixed in this session; no code changed.

## Coverage — read this first

**Probed** ([probe], output quoted): L01–L14.

**Read** ([code]): the whole diff of `src/svg.rs`, `svg_bounds.rs`,
`svg_fonts.rs`, `canvas.rs`, `cascade/selector.rs`, `build.rs`,
`template.rs`, `fuzz.rs`; `tera_check/analysis.rs` and `mod.rs` whole,
its lexer whole, its parser's statements and expression loop;
`ftl_check.rs`'s first 480 lines; the three new ADRs; `tera_check`'s
tests.

**Not covered:** the rest of `ftl_check.rs` (I trusted its differential
test), the port of Tera's parser line by line against the original
(maps, arrays, component attributes), the bug reports' text beyond
bug_0222, `tests/canvas.rs`, `tests/html_ui.rs` and
`tests/windowing_model.rs` changes, the four fuzz lockfiles. No fuzz
run. No windowed run of an example, so L12 has no screenshot.

**Fixes I re-probed:** K01 and K03 (refused now), K02 (charged, but see
L08), K04 (charged, but see L09), K12 (through L11/L12's probes: frame
and picture coexist). K05–K11 and K13–K17 I took from their regression
tests, which pass in CI; I didn't run my review_0007 probes against
them again.

## Summary

CI's whole list is green. The fixes for review_0007's findings do what
their bug reports say for the inputs those reports name.

The trouble is in what the fix session added on top. ADR 0044 declared
every input untrusted and built a static template check to back it; the
README now promises that hostile content fails to load "instead of
stalling or crashing the app". The check doesn't deliver that yet:

- It crashes the app itself. Its analysis recurses per statement level
  across includes, and 24 small include files overflow the asset
  thread's stack in a release build (4 files in a debug build).
- Two ways around its value bound reach the same out-of-memory abort as
  bug_0220: `set_global` inside a component call's body, and the
  `default` argument of `get`.
- Its step count misses most of what makes a render slow: loops over
  context data count once however deeply they nest, and building a
  value costs one step whatever its size. Accepted templates of about
  100 bytes render for seconds to minutes.
- It refuses a counter in two nested loops, which is ordinary template
  code.

The SVG fixes charge the cases review_0007 named and stop there. Dashes
are charged per dash, but a dash with a round cap costs 250 times more.
`<style>` rules are charged per rule × element, but a rule's selector
can make one pair cost seconds: simplecss matches descendant selectors
by backtracking, and 555 bytes parse for 36 seconds.

The `CanvasPicture` child fixed the frame conflict and brought two
regressions: a `pointer-events: none` canvas now swallows clicks, and
the picture ignores the canvas's `border-radius`.

## CI at `54bad82` (ci.yml's env)

| command | exit |
|---|---|
| `check --lib` | 0 |
| `check --lib --features text-input` | 0 |
| `check --lib --features fuzzing` | 0 |
| `check --lib --features svg` | 0 |
| `check --lib --features svg-text` | 0 |
| `build --all-targets` | 0 |
| `test` | 0 (542 tests, 2 ignored) |
| `test --features text-input` | 0 (548, 2 ignored) |
| `test --features svg --lib --test html_ui --test content_lint` | 0 |
| `test --features svg-text --lib --test html_ui` | 0 |
| `check --examples --features hot-reload,dev-tools,text-input,svg-text` | 0 |
| `test --example hot-reload --features hot-reload` | 0 |
| `test --lib --features fuzzing` | 0 (271) |
| `doc --no-deps` | 0 |

Also run: `PROPTEST_CASES=400 cargo test --release --test
windowing_model` (AGENTS.md asks for it after windowing changes): 17
passed.

rustc 1.98.1 (48a229cea 2026-09-01)

## Findings

Severity as in the bug reports: reachable from content = high when it
aborts or hangs without bound. "DoS" marks a stall. "Regression" marks
behaviour that was right at `f76b320`.

### The template check (`src/tera_check/`, ADR 0044)

**L01 · high · regression · The check overflows the asset thread's stack and aborts the app** [probe]
- `Analyzer` recurses once per statement level (`nodes` → `for_loop` →
  `nodes` …) and continues into each `{% include %}`. `MAX_NESTING` (64)
  counts templates and components only; Tera allows 40 levels in each.
  Bevy's IO task pool threads have the default 2 MB stack.
- **Probe** (through the real loader, release build): 24 files, each 30
  nested `{% for a in items %}` around an include of the next (23.6 KB
  in all):
  ```
  thread 'IO Task Pool (1)' has overflowed its stack
  fatal runtime error: stack overflow, aborting   (SIGABRT)
  ```
  On a 2 MB thread, `tera_check::check` alone:

  | build | accepted | overflows |
  |---|---|---|
  | release | 16 files × 30 levels | 24 × 30 |
  | debug | 2 × 39 | 4 × 39 (5 KB), 8 × 20 |

  Tera itself compiles and runs 60 files × 38 levels on the same thread.
- At `f76b320` these templates loaded. `the_check_bounds_its_own_work`
  tests a chain of 200 includes with nothing nested in them.
- **Fix direction:** one depth budget for the whole evaluation (statement
  levels included), sized for a debug build on 2 MB, refusing with
  `TooComplex`; or an explicit work stack.

**L02 · high · `set_global` inside a component call's body isn't tracked: doubling passes** [probe]
- `carried` finds a loop's `set_global` targets through `if`, `for`,
  captures, blocks and includes. A component call with a body is
  `Node::Expr(Expr::Component { body })`, which it doesn't enter; the
  body runs in the caller's scope (`self.nodes(body, env)`), and Tera
  runs it there too.
- **Probe:**
  ```
  {% component wrap() %}{{ body }}{% endcomponent %}
  {% set_global s = 'aaaaaaaa' %}
  {% for i in range(end=N) %}{% <wrap> %}{% set_global s = s ~ s %}{% </wrap> %}{% endfor %}
  [{{ s | length }}]
  ```
  N = 16: accepted, renders `[524288]`. N = 20: accepted (bound on the
  output: 146 bytes), renders `[8388608]`. N = 100 000: accepted
  (steps 1 000 005, 3% of the budget). Without the component the same
  loop is refused (`Superlinear`). I didn't render past N = 20: N = 40
  asks for 8 TB, which is bug_0220's abort.
- **Fix direction:** `carried` must walk expressions (component bodies
  are the only statements inside one).

**L03 · high · `get`'s `default` argument isn't in the bound** [probe]
- `filter` maps `get` to `x.element()` and never reads `default`
  (`analysis.rs`, the `"first" | "last" | "nth" | "get"` arm). Tera
  returns the default when the key is missing (`filters.rs:647`).
- **Probe:** `s` is a 10 MB string (100 000 appends of 100 bytes,
  accepted: it's under `MAX_TEMPLATE_VALUE_BYTES`). Then
  `{% set y = {'a': 1} | get(key='b', default=s) %}` and
  `{% set z = y ~ y ~ … %}` with 8 copies: accepted with an output bound
  of 146 bytes, renders `[80000000]`. Sixteen `{% set y = y ~ y %}`
  after the `get` (655 GB): accepted. The same without `get`: refused
  (`ValueTooLarge`).
- **Fix direction:** `x.element().max(default)`. Worth a test that walks
  every built-in filter's arguments against the model: this is the one
  I found by reading, not by enumeration.

**L04 · high (DoS) · Loops over context data count once at any depth** [probe]
- A loop over an app value has the count `items.fold()`: one. That's the
  ADR's "per app element". Nested, it's still one per level, so n items
  cost n^levels and the check reports a handful of steps. `range(end=x)`
  with a context number `x` counts once too (`k.min(1)`), though a
  number isn't a collection the app sized.
- **Probe** (release; nothing printed, so `MAX_RENDERED_BYTES` never
  applies):

  | context value | levels | template | check: steps | render |
  |---|---|---|---|---|
  | list of 30 | 4 | 132 B | 16 | 7.5 ms |
  | list of 30 | 5 | 165 B | 20 | 223 ms |
  | 33-char string | 5 | 165 B | 20 | 513 ms |
  | `hp = 100`, `range(end=hp)` | 3 | 123 B | 15 | 36.7 ms |
  | `hp = 100`, `range(end=hp)` | 4 | 164 B | 20 | 3.54 s |
  | list of 30 | 30 | 990 B | 120 | (not rendered) |

  Each level multiplies by n: six levels over the list would be about
  7 s, five of `range(end=hp)` about 6 minutes. Tera's parser allows 40.
- Any app whose context holds a string, a list or a number above 1 is
  open to this. `accepted_templates_render_within_the_bound` generates
  context lists of at most one element, so it can't see it.
- The single-level case has the same shape in small: the step budget is
  per element, so a template may spend the whole budget (0.45 s by the
  ADR's calibration) for each of the app's rows.
- **Fix direction:** multiply app counts when loops nest (a second loop
  over app data inside one is `A²`: refuse it unless the inner target
  derives from the outer loop's variable, `group.cards`); treat a
  context number in `range` as `RANGE_MAX`, not as one.

**L05 · medium (DoS) · Building a value costs one step whatever its size** [probe]
- Only `filter` charges for size (`x.bytes.c / 16`). `range()`, `~`,
  `in`, array spreads and comparisons are one tick each.
- **Probe** (release):

  | template (outer loop count N) | check: steps at N = 100 000 | measured |
  |---|---|---|
  | `{% for i in range(end=N) %}{% set x = range(end=100000) %}{% endfor %}` | 500 003 (1.5% of the budget) | N = 1 000: 2.59 s; 4 000: 10.4 s |
  | `{% set big = range(end=100000) %}` … `{% if 100001 in big %}` in the loop | 800 006 | N = 1 000: 0.25 s; 4 000: 0.99 s |
  | `{% set_global s = s ~ '<100 bytes>' %}` in the loop | 600 024 | N = 100 000: 13–17 s |

  The first two scale linearly in N: about 260 s and 25 s at the
  accepted N = 100 000 (extrapolated, not run). The third was run at
  100 000: a linear accumulator copies the whole string on every
  append, so the form the check was built to accept is quadratic in
  time.
- **Fix direction:** charge `bytes / 16` (or the item count) wherever a
  value is built or scanned, as `filter` does.

**L06 · medium · regression · A `set_global` counter in two nested loops is refused** [probe]
- **Probe:**
  ```
  {% set_global n = 0 %}{% for g in groups %}{% for c in g.cards %}{% set_global n = n + 1 %}{% endfor %}{% endfor %}{{ n }}
  ```
  → ``refused: `n`, set with `set_global` in a loop, could grow faster
  than linearly``. The same with `names = names ~ c.name`. One loop
  level is accepted. This loaded at `f76b320`; a refused template fails
  its asset load.
- [code] My reading of the cause: `growth` gives each carried variable
  two symbols (bytes, items) and builds the inner loop's result from
  `start = before.bytes.max(before.items)`, so the outer pass sees two
  of its own symbols in one value and counts two copies.
- Also refused, and listed in ADR 0044 as accepted costs:
  `{{ text | replace(from='{name}', to=player) }}` and
  `{{ items | join(sep=sep) }}` (two context values in one filter).
  The first is how a page fills a placeholder in a context string.
- **Fix direction:** one symbol per carried variable, or compare the
  parts separately. `ordinary_templates_pass` needs nested accumulators.

### SVG

**L07 · high (DoS) · A descendant selector in `<style>` takes exponential time** [probe]
- simplecss 0.2.2 matches a descendant combinator by trying every
  ancestor and recursing (`selector.rs:174`, no memo): a selector of k
  parts over d nested elements costs about C(d, k) when its leftmost
  part never matches. `svg_bounds::Graph::new` runs that match before
  any budget is counted (bug_0207's charge is rules × elements: 1 × 61
  here); usvg's `resolve_css` would run it again.
- **Probe** (release, `svg::parse`; the document is one rule and nested
  `<g>`s around a `<rect>`):

  | `<g>` depth | selector | document | parse |
  |---|---|---|---|
  | 40 | `q g g g g` | 409 B | 9.6 ms |
  | 40 | `q g g g g g g` | 413 B | 288 ms |
  | 40 | `q g g g g g g g` | 415 B | 1.25 s |
  | 60 | `q g g g g g g` | 553 B | 5.24 s |
  | 60 | `q g g g g g g g` | 555 B | 35.7 s |

  All five then load. One more part or a deeper nest multiplies it.
- [code] For an inline `<svg>` up to 64 KB this runs during the build,
  on the main thread: `parse_unless_costly` computes the bounds before
  it can defer.
- Present at `f76b320`; review_0007 K04 looked at this pass and missed it.
- **Fix direction:** bound the selector before matching (combinator
  count × depth), or match with a memo per (part, element); UPSTREAM
  entry for simplecss.

**L08 · medium (DoS) · The drawing estimate doesn't see overlap** [probe]
- `CostWalk::path` charges segments, dashes and the path's bounding box
  once per paint. tiny-skia's time follows the covered area summed over
  every subpath, dash, cap and edge crossing. bug_0205's `DASH_COST`
  was calibrated on butt-capped hairlines.
- **Probe** (release; all drawn, none refused):

  | document | estimate (of `MAX_SVG_COST`) | draw |
  |---|---|---|
  | 4 lines, 10⁶ dashes each, butt caps, width 1 (K02's case) | 47% | 135 ms |
  | the same, round caps, width 10 | 47% | 10.7 s |
  | the same, round caps, width 40 | 48% | 33.0 s |
  | the same, square caps, width 40 | 48% | 1.84 s |
  | one path of 10⁶ zero-length subpaths, round caps, width 40 | 37% | 28.8 s |
  | the same, width 200 | 38% | 107 s |
  | one filled path of 10⁶ segments, each 4 096 px tall | 55% | 57.1 s |
  | the same path stroked, width 30 | 56% | 139 s |
  | 200 000 such segments, filled | 41% | 9.6 s |

- The first three rows are K02 not closed; the rest was there at
  `f76b320` and is new to the reviews. These draw on a task or the
  loader's thread, not the main thread (10 000 dots are already over
  the inline cache's synchronous share).
- **Fix direction:** charge each dash and each subpath its own stroked
  area (length × width + caps), and a filled path its segments × the
  rows they span, not only its bounding box.

**L09 · medium (DoS) · The `<style>` charge assumes cheap selectors, and its refusal comes after the work** [probe]
- bug_0207 charges rules × elements ÷ 4 and refuses early only when that
  alone passes `MAX_SVG_WORK`. Below it the matching runs, and the
  total is checked afterwards.
- **Probe** (release, `svg::parse`):

  | rules × elements | document | result |
  |---|---|---|
  | 5 000 × 6 000, type selectors | 71 KB | loads, 299 ms |
  | 30 000 × 1 000, type selectors | 315 KB | refused (`TooLarge`) after 3.25 s |
  | 2 000 rules of 16 classes × 16 000 elements with 16 classes | 737 KB | refused (`TooLarge`) after 13.6 s |

  The last is 3.2 × 10⁷ pairs, just under the early check, at about
  400 ns a pair where the charge assumes 15.
- **Fix direction:** weigh a rule by its selector's parts and classes;
  with L07's fix this is one bound on selector cost.

**L10 · medium (DoS) · Text conversion is quadratic in a text's length** (`svg-text`) [probe]
- `svg_bounds` charges 16 units per text byte, linear. usvg's text
  layout isn't.
- **Probe** (release, Bevy's default font, one `<text>` of N characters
  at 10 px; parse time only):

  | N | `M` | a character the font lacks |
  |---|---|---|
  | 20 000 | 173 ms | 548 ms |
  | 100 000 | 3.93 s | 13.5 s |

  5 × the text, 23–25 × the time. Both are inside the work budget (the
  larger is 57% of it). 20 000 separate one-letter `<text>` elements
  parse in 104 ms, so it's per text chunk.
- Present at `f76b320`. I didn't find where in usvg the square is.
- **Fix direction:** charge a text chunk by the square of its length,
  or cap a chunk's length.

### `<canvas>` (ADR 0043)

**L11 · medium · regression · A `pointer-events: none` canvas swallows clicks** [probe]
- CSS `pointer-events: none` puts `Pickable::IGNORE` on the element.
  The `CanvasPicture` child that `bind` spawns has no `Pickable`, and
  every UI node without one blocks the pointer.
- **Probe** (real picking stack): a `data-on-click` button with a
  `position: absolute; pointer-events: none` canvas over it. Before a
  source is registered a click gives `["hit"]`; with the canvas showing
  an image, `[]`. The picture reports `Pickable=None`.
- At `f76b320` the source was on the element, which ignored the
  pointer. A decorative 3D view over a HUD is the case.
- **Fix direction:** copy the element's `Pickable` to the picture on
  every `bind` (it changes with restyles).

**L12 · low · regression · The picture ignores the canvas's `border-radius`** [probe]
- Bevy rounds a `ViewportNode` and an `ImageNode` by their own node's
  radius (`bevy_ui_render` `lib.rs:1461`). The picture's node declares
  none, and Bevy's overflow clip is rectangular.
- **Probe:** a canvas with `border-radius: 50px; border-width: 4px;
  overflow: hidden`: the element resolves 50 px corners, both pictures
  (image and viewport) resolve `Vec2(0.0, 0.0)` on all four.
- The `canvas` example's `.portrait` has `border-radius: 8px` and a
  3 px border; I didn't take a screenshot, so how visible the square
  corners are there is unconfirmed.
- **Fix direction:** give the picture the element's resolved radius
  less the border, each frame.

### CSS

**L13 · low (DoS) · One level of `:is()` with a combinator is still cubic in depth** [probe]
- bug_0213 allows a combinator inside one level. Each test of such a
  compound walks to the root, and the enclosing descendant walk tests
  it at every ancestor.
- **Probe** (release, settle time of one build; N copies of one rule,
  `plain` = `.never div div`, `is` = `:is(.never div) div`):

  | page | rules | `plain` | `is` |
  |---|---|---|---|
  | chains 95 deep | 100 | 22 ms | 162 ms |
  | chains 95 deep | 400 (13 KB) | 33 ms | 599 ms |
  | chains 48 deep | 400 | 17 ms | 92 ms |

  1.5 ms per rule at depth 95; ten or twenty identical chains cost the
  same (the style cache shares them). A second `:is()` in the rule adds
  nothing. Pages with differing chains weren't measured.
- `allowed_is_arguments_match_in_linear_time` asserts at most 8× for 4×
  the depth; what it measures is this quadratic per element plus fixed
  overhead.
- **Fix direction:** bounded already by `MAX_DOM_DEPTH`; a rule budget
  per sheet (rules × combinators) would close it and the plain case.

### Fluent (`src/ftl_check.rs`, ADR 0045)

**L14 · low · The escape scan refuses bundles whose text holds `\u` before a non-ASCII letter** [probe]
- `bad_escape` scans the whole file, text included (the ADR says so and
  calls it rare).
- **Probe:** `path = Folder C:\użytkownik` and `cmd = Wpisz \użyj w
  konsoli`: the check refuses the resource; fluent-syntax parses both.
  The loader then fails the whole bundle, so every string falls back.
  `C:\usługi` and `C:\users\anna` pass.
- Polish, Czech and Turkish words starting `uż`, `uč`, `uç` after a
  backslash: Windows paths and command names in help text.
- **Fix direction:** the port already knows where string literals are;
  check the escape there instead of scanning the file.

### Records and claims

**L15 · low · What the records say and the code doesn't do** [code]
- README "Untrusted content" says every input, images included, is
  bounded and fails to load "instead of stalling or crashing the app".
  Raster images go through Bevy's loader with no bound from this crate,
  and L01–L10 are stalls and crashes from the inputs it names.
- `docs/agents/bugs/INDEX.md` lists bug_0204–0221 as "fixed
  (uncommitted)"; they are in `d7ddbe2`.
- review_0007 K18 is still open: AGENTS.md's Layout has no line for
  `src/md.rs`, `src/fuzz.rs`, `tests/source_lint.rs` or
  `tests/selector_model.rs`, and WORKSPACE.md/AIWG.md name
  `nchashch/bevy_markup` where `Cargo.toml` and the CHANGELOG links
  name `noeticbasho/bevy_markup`.

**L16 · low · An inline `<svg>` refused for the memory budget on its task is never retried** [code]
- A large or costly inline text reserves its bytes inside the task. If
  the reservation fails, the task returns an error; the entry stays in
  `entries` with no image and isn't in `over_budget`, so freeing the
  budget (`over_budget.clear()`) doesn't bring it back while the page
  shows it. The synchronous path does retry.

## Checked, no finding

- K01 and K03: a pattern on text is refused (estimate 2 × 10¹²), 64 and
  800 glyphs at 3 000 px are refused, 16 draw in 65 ms at 66% of the
  budget.
- A `set_global` in the parent of a template included in a loop doesn't
  reach the including template's globals (`s` stayed 8 bytes), so
  `carried` not following `extends` there is harmless.
- Tera clears a loop's scope every iteration (`for_loop.rs:269`), so a
  plain `set` can't carry a value.
- The inline cache's refusal memory past its 4 096 cap: 4 200 refused
  texts cost 4 ms per round.
- A chain of 99 message references formats on a 2 MB thread; 400 and
  3 000 end in fluent's own "Too many placeables".
- 100 000 `<p>`s from a 58-byte template settle in 0.5 s without layout
  (20 000 with layout: 0.4 s, 4 ms per idle frame).
- Four nested loops over 40 context items through the real loader:
  settles in 29 ms (L04's numbers start to hurt one level later).

## Triage

1. L01: an abort from a fix, on a path every template takes.
2. L02, L03: bug_0220's abort, still reachable.
3. L07: the smallest input with the longest stall, and possibly on the
   main thread.
4. L04, L05: together they decide whether the step budget means
   anything; L06 goes with them (the same `growth` code).
5. L11 (a regression users will hit), then L12.
6. L08–L10, L13: calibration. L08 and L09 want one model of cost per
   primitive, not another constant per reported case.
7. L14–L16.

A note for whoever fixes these: review_0007's fixes each closed the
input its finding named, and L08, L09 and L13 are the next input along.
For a bound, the regression test worth having is a search for the worst
accepted input, not the reported one.

## Filed

Filed as open bug reports on 2026-11-12, before any fix: L01 bug_0223,
L02 bug_0224, L03 bug_0225, L04 bug_0226, L05 bug_0227, L06 bug_0228,
L07 bug_0229 (UPSTREAM U40), L08 bug_0230, L09 bug_0231, L10 bug_0232
(UPSTREAM U41), L11 bug_0233, L12 bug_0234, L13 bug_0235, L14
bug_0236, L16 bug_0237. L15 is records drift, not a defect.
