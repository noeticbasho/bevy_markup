# testbench

![The testbench: a compact scenario list grouped into Performance, Security and Correctness on the left (Order independence selected); size, churn, restart and language controls on top; a line of live numbers (frame time, builds and restyles per second, UIs and elements); green PASS checks (built, twins match 30 / 0, frame budget); and the stage below showing the rows of the order-independence workload, styled by their data-* attributes and classes](../screenshots/testbench.png)

An interactive torture bench for performance, security and correctness. Pick
a scenario, change its size, toggle the per-frame churn, and watch the frame
time, the update rate and the scenario's checks (**PASS** / **FAIL** /
**WAIT**). With `--features hotpath`, every pipeline system and the bench's
own work is timed per call, in the real Bevy frame loop.

```sh
# Interactive, with live profiling: run `hotpath console` in a second terminal
# (cargo install hotpath --features=tui), then:
cargo run --release --example testbench --features hotpath

# Scripted: one scenario, a fixed size, a bounded headless run; the checks'
# final states are logged ("testbench: final …") and the hotpath report
# printed on exit.
cargo run --release --example testbench --features hotpath -- \
    --scenario big-document --size 2000 --no-render --exit-after 900

`--windowed` builds the workloads only over their scroll window (a
fixed-height scrolling viewport; ADR 0031) and `--scroll` advances the
main workload's scroll every frame (bouncing), so the band's movement —
the scroll-frame cost — is measurable. Windowing needs layout: run it
`--offscreen` (real layout, no window); under `--no-render` the root is
never laid out and a windowed UI builds everything. `--windowed` adds a
**band** check to every scenario: the built rows cover the main
workload's viewport (no gap taller than a row) — the symptom of bug_0151,
which a count of built rows can't see. A/B from the 2000-row
big-document (release, hotpath, ADR 0031): the worst frame 77.9 ms
unwindowed vs 17.2 ms windowed idle, 17.9 ms scrolling, churned at
parity.
```

## Scenarios

The workloads are generated at runtime into an in-memory asset source
(`testbench://`), so they go through the real loaders and their pre-scans,
and each start loads fresh paths. Size is the number of rows, cards or UIs
for the performance and correctness scenarios; the security inputs are
fixed.

| Group | Scenario | What it does | Checks |
|---|---|---|---|
| Performance | Big document | N rows; churn changes ~1 % of them per frame, so unchanged rows reuse the previous build (ADR 0021) | built, budget |
| | All dirty | every row changes every frame: full render, parse, cascade, diff | built, budget |
| | Restyle storm | a context `var()` recolours every row, a highlight moves | built, budget |
| | Indented Markdown | a `.md` template with indented Tera blocks (ADR 0024), re-rendered | built, budget |
| | Localized rows | Fluent rows in a `markdown: true` bundle, cached and per-value paths | built, budget |
| | Many UIs | N separate small `HtmlUi`s; one updates per frame | built, budget |
| | calc() layout | N cards sized by mixed-unit `calc()` | built, budget |
| | Selector list | N rows in a container, plus a list and a table, styled by structural pseudo-classes, `+`/`~`, `[data-*]` and `:not()` (ADR 0028/0030); ~1 % of the rows change per frame | built, reuse, budget |
| | Text features | `text-align`, `line-height`, `letter-spacing`, `text-transform`, `white-space`, list markers, a table, a tiled `background-image` and an `overflow: auto` panel whose class flips every frame (ADR 0027/0028) | built, auto-scroll, budget |
| Security | Nested CSS rules | 20 000 nested rules: the sheet's load fails cleanly (bug_0106) | sheet refused, built |
| | Deep DOM | 5 000 nested elements: content past 100 levels is dropped | built |
| | var() bombs | 2 000-link chains, doubling fallbacks, cycles, nested functions (bug_0092) | built |
| | Grid bombs | track counts that overflowed (bug_0081, bug_0121), 1 000 names (bug_0099) | built |
| | Hostile Fluent args | HTML, Markdown links and images, crafted markers, emoji in string args (bug_0088, bug_0107, bug_0128) | nothing forged, every payload localized |
| | Args JSON injection | a hostile name through a Tera map literal and through a hand-written JSON splice (bug_0119) | map literal kept the value |
| | Remote sources | an `https://` include and `border-image` (bug_0118) | include refused, image skipped |
| | Markdown bombs | 20 000 `*` and `[`, 2 000 nested quotes, crafted markers, 30 nested indented Tera blocks, 10 000 indented lines | survived |
| | Selector bombs | extreme `An+B` offsets (bug_0133), a 10 000-character attribute value, 1 000 `~` rules, nested `:not()`, a 500-class compound, `]`/quotes/combinators inside quoted values (bug_0144) | built, quoted values match |
| | Sibling bombs | one container of N siblings under `~`, `:nth-child`, `:last-of-type` rules while the `.hot` element moves (bug_0134) | built |
| | Entity bombs | 50 000 references without `;` (bug_0135), a 100 000-letter name, references in attributes, `&#+65;` (bug_0146) | built, references decode |
| | Function bombs | 30 nested `light-dark(` and 30 nested `calc()` around a `var()`, in a stylesheet and in `style` attributes: refused or dropped at once by the paren guard, where lightningcss took minutes to hours (bug_0179) | sheet refused, attribute dropped, built |
| | Selector engine | `:not\0()` (bug_0180), the forms parcel's matcher panics on (`:has()`, `:nth-child(2 of .a)`, `:nth-col()`, nested `&` rules), nested `:not()` and a far sibling combinator skip their rules while the sheet loads; `:nth-child(0)` no longer colours the root; `:is()`, `:only-of-type` and `^=` match (ADR 0034) | built, rules |
| Correctness | Reuse vs fresh build | random edits (change, insert, remove, move, highlight) every frame; every 20 frames the churn holds and the UI is compared with a fresh build of the same state | twins match |
| | Indented vs flat | an indented and a flat Markdown template of the same document must build the same UI | twins match |
| | Selectors vs fresh | the selector list under count-preserving random edits (value, flag, swap), compared with a fresh build every 20 frames (bug_0136) | twins match |
| | Scoped vs full restyle | the focus moves to a random row every frame (`:focus`, `:focus-visible`, `:focus-within`, `:not()`, `+`, `~`); every 20 frames the scoped result is compared with a forced full restyle of the same UI (ADR 0029, bug_0139) | twins match |
| | Order independence | rows whose style depends only on themselves; a fresh build of the rows in reverse order must style every row the same (bug_0137) | twins match |
| | Idle stays quiet | `calc()`, `overflow: auto`, a background image, transitions, structural selectors, fed identical contexts every frame: no build, restyle or `Node` write after settling (bug_0140's class) | quiet |
| | Scaling N vs 4N | a container of N siblings with unterminated references next to one of 4N; phases alternate, the 4N frame may cost at most 8× the N frame, best of three (bug_0134, bug_0135) | linear |

Every scenario also has a **frame budget** check: the worst frame after a
30-frame warm-up must stay under 250 ms, measured in real time (the
virtual clock clamps every frame to 250 ms, which hid slower frames).

## Methods

The correctness scenarios and checks follow review_0005's methods (the
testing guide, `docs/agents/skills/testing.md`, has the full list).
Each check was falsified: the bug it targets was put back and the check
failed.

- **Fresh twin** (`reuse-vs-fresh`, `selectors-vs-fresh`): the incremental
  paths (content reuse, keyed pairing) against a from-scratch build of the
  same state. The edits keep the list's length where reuse is the point:
  a length change turns the pairing off, and nothing could go stale.
- **Full restyle** (`restyle-vs-full`): ADR 0029's scoped interaction
  restyles against the same UI after a forced full restyle (its
  stylesheet re-inserted), with the interaction state held. Interaction
  comes from `InputFocus`, not hand-set hover states: picking rewrites
  `PseudoState` every frame. One row element is styled by a
  `:not(:focus)` rule alone, because an element that another pseudo rule
  also targets re-cascades anyway.
- **Metamorphic reversal** (`order-independence`): a fresh twin shares a
  matching or caching bug; reversing order-independent rows doesn't. The
  rows are id-less and each sits alone in its slot, so they share
  everything but what the selectors read.
- **Reuse happens** (`reuse`): `Skeleton::last_reuse()` must show at least
  three quarters of the specs reused. Comparing results can't see lost
  reuse.
- **Quiet when idle** (`quiet`): change loops show as builds, restyles or
  `Node` writes on an idle UI.
- **Scaling** (`linear`): N against 4N, best of three, at most 8×, with the
  other UI hidden so its layout doesn't dilute the ratio.

## Controls

| Input | Action |
|---|---|
| Click a scenario, or focus it and Enter / A | Start it |
| N / P, gamepad RB / LB | Next / previous scenario |
| − / + buttons, [ / ], gamepad LT / RT, ← → on the focused size | Size (10 … 10 000) |
| C / gamepad X, or the Churn button | Toggle the per-frame churn |
| R / gamepad Select, or Restart | Restart the scenario |
| L / gamepad Y, or Language | English ↔ German |

Command-line flags: `--scenario <id>` (pins it; the scenario ids are the
ones in `scenarios.rs`, e.g. `reuse-vs-fresh`), `--size <n>` (the nearest
size step at or above; pins the size, so `--exit-after`'s injected keys
can't step it), `--no-churn`, plus the examples' launcher flags
(`--no-render`, `--offscreen`, `--exit-after <frames>`).

## Reading the numbers

The shell shows the frame time over the last 120 frames, the workloads'
builds and restyles per second, and the element count. The hotpath report
splits the frame by function: bevy_markup's systems (`render_templates`,
`localize`, `build_html_ui` and their internals such as `pair_trees_into`,
`collect_items`, `update_children`, `HtmlStyles::get_with`) and the bench's
own model work (`testbench::churn`, `testbench::serialize_context`,
`scenarios::context`). Whatever the frame time holds beyond those is Bevy's
own layout, text and rendering. For example, a 2 000-row big document in
release spent about 4 ms per frame in `build_html_ui` against a ~65 ms frame,
which tells you where to optimise next.

## Files

| File | Contents |
|---|---|
| `main.rs` | The app: the in-memory asset source, the controls, starting scenarios, churn (rows, focus, scaling phases), the comparisons (`TwinKind`), the checks, the shell's numbers |
| `scenarios.rs` | The scenarios: generated templates and stylesheets, the row model, the per-frame churn, the workload contexts |
| `../assets/testbench/shell.html` | The shell template (scenario list, controls, numbers, checks, the stage) |
| `../assets/testbench/style.css` | The shell's stylesheet |
| `../assets/testbench/locales/{en-US,de}/` | Fluent bundles (`markdown: true`): the shell's strings and the workloads' messages |
