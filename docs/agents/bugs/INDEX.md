# Bug Reports — Index

Upstream defects (in dependencies and tools) are also tracked in
[UPSTREAM.md](UPSTREAM.md) until they're reported and fixed upstream.

Whole-crate review sessions are filed in
[reviews/](../reviews/) as numbered reports: [review_0001](../reviews/review_0001.md)
(80 findings, `R01`–`R80`), [review_0002](../reviews/review_0002.md)
(their verdicts + new defects `F01`–`F43`),
[review_0003](../reviews/review_0003.md) (the fix session's audit + new
defects `G01`–`G44`) and [review_0004](../reviews/review_0004.md) (the
G fix session's audit + new defects `H01`–`H10`),
[review_0005](../reviews/review_0005.md) (the ADR 0027–0029 work: new
defects `I01`–`I16`) and [review_0006](../reviews/review_0006.md) (the
windowed build, ADR 0031: `J01`–`J09`). A finding gets its
own `bug_NNNN.md` here when its fix lands.

The crate was called `p23` until 2026-10-05 and is now `bevy_markup`; the
reports keep the name, paths and commands as they were at the time
(`p23::fuzz` is now `bevy_markup::fuzz`, `P23_*` variables are
`BEVY_MARKUP_*`).

Bugs found by the automated testing infrastructure. One file per bug,
`bug_NNNN.md`, numbered in discovery order. This page is the title/metadata
record; each discovery session has its own metadata section below
(bug_0001–0005: property testing, stateful testing and fuzzing — the fuzzers
immediately paid for themselves; bug_0006–0007: the Fluent oracle; bug_0008–0013: property tests and the layout oracle; bug_0014: mutation-testing triage; bug_0015: the fuzz-corpus coverage replay; bug_0016–0017: demo crashes with nested `HtmlUi`s; bug_0020: the picking-driven signals tests; bug_0021: prototype_19 integration; bug_0022: the `menu` example's smoke run; bug_0023: user report; bug_0024: code reading; bug_0025: repeated suite runs; bug_0027: the `world` example's smoke run; bug_0028: user report; bug_0029–0030: user report; bug_0031–0080: the 2026-10-13 whole-crate code review's findings, fixed the same day).

## Bugs

| ID      | Title | Component | Severity | Status | Found by |
|---------|-------|-----------|----------|--------|----------|
| [bug_0001](bug_0001.md) | Stylesheet that fails to load leaves the `HtmlUi` permanently blank | `build.rs` (pipeline) | high | fixed (`9e87b67`) | `arbtest` (tests/arbtest.rs) |
| [bug_0002](bug_0002.md) | Non-ASCII selector panics `Compound::parse` (byte slice across char boundary) | `cascade.rs` | high | fixed (`017cfac`) | cargo-fuzz `css` target |
| [bug_0003](bug_0003.md) | Removing `HtmlDebugOutline` never triggers a rebuild | `build.rs` (pipeline) | medium | fixed (`06b86ae`) | `proptest-stateful` (tests/stateful.rs) |
| [bug_0004](bug_0004.md) | Global failed-stylesheet latch swallows a later re-select of the same broken sheet | `build.rs` (`FailedSheets`) | medium | fixed (`06b86ae`) | `proptest-stateful` (tests/stateful.rs) |
| [bug_0005](bug_0005.md) | fluent-syntax panics on an invalid unicode escape before a multi-byte character | upstream `fluent-syntax` 0.11.1 | high | fixed in the fluent-syntax fork (git patch); **upstream unfixed**, PR pending | honggfuzz `ftl` target |
| [bug_0006](bug_0006.md) | `data-l10n-id` on containers and inline elements is silently ignored | `build.rs` (translation walk) | medium | fixed (`94a4fb9`) | Fluent oracle (`fluent_oracle`) |
| [bug_0007](bug_0007.md) | `data-l10n-name` overlays unsupported: source attributes lost | `build.rs` (translation walk) | medium | fixed (`94a4fb9`) | Fluent oracle (`fluent_oracle`) |
| [bug_0008](bug_0008.md) | Removing a per-entity `HtmlStylesheet` never rebuilds | `src/build.rs` | medium | fixed (`0d9d6cb`) | targeted probe while designing proptest-stateful ops |
| [bug_0009](bug_0009.md) | Entity with a failed `HtmlStylesheet` ignores `DefaultStylesheet` swaps | `src/build.rs` | medium | fixed (`0d9d6cb`) | probe, then proptest-stateful |
| [bug_0010](bug_0010.md) | Re-requesting a failed stylesheet eats the change signal | `src/build.rs` | medium | fixed (`0d9d6cb`) | proptest-stateful, minimal `[SetLocale(En), SetOwnSheet(Broken), SetTheme(Broken)]`, ~1 in 3 runs |
| [bug_0011](bug_0011.md) | NBSP and other Unicode spaces were collapsed and trimmed | `src/build.rs` | medium | fixed (`0d9d6cb`) | arbtest structure-aware document model |
| [bug_0012](bug_0012.md) | `HtmlElements` lookups were breadth-first, not document order | `src/html.rs` | medium | fixed (`0d9d6cb`) | unit test + arbtest model |
| [bug_0013](bug_0013.md) | CSS padding on a boxed `pre` added to the default 8px | `src/build.rs` | low | fixed (`0d9d6cb`) | code reading while adding layout support; confirmed by the layout oracle |
| [bug_0014](bug_0014.md) | Text in a boxed block never wraps: it overflows the box | `src/build.rs` | medium | fixed (`311c808`) | mutation-testing triage (layout oracle case) |
| [bug_0015](bug_0015.md) | Deeply nested FTL expressions overflow the stack | upstream `fluent-syntax` 0.11.1 | medium | fixed in the fluent-syntax fork (git patch; first fix missed nested calls); **upstream unfixed**, PR pending | fuzz-corpus coverage replay |
| [bug_0016](bug_0016.md) | Rebuilding an `HtmlUi` under a rebuilding ancestor `HtmlUi` panics (commands on despawned entities) | `src/build.rs` | high | fixed (`9d21b17`) | user report (demo locale click), minimized to a vector |
| [bug_0017](bug_0017.md) | Restyle fallback rebuild despawned app-nested `HtmlUi`s (same_shape counted them as shape mismatches) | `src/build.rs` (`same_shape`) | high | fixed (`d079b08`) | startup crash of the demo, traced via `bevy/debug` |
| [bug_0018](bug_0018.md) | An app `ImageNode` on a built element turned every restyle into a rebuild (rebuild every frame) | `src/build.rs` (`same_shape`) | high | fixed (`f99c6ec`) | prototype_19 integration (entity-id sampling over BRP) |
| [bug_0019](bug_0019.md) | A value-less attribute (`autofocus`) ate the next attribute's first character | upstream `tl` 0.7.8 (UPSTREAM.md U11) | high | fixed (`f81acce`): dependency switched to `astral-tl` 0.8.0 | prototype_19 integration (gamepad smoke run) |
| [bug_0020](bug_0020.md) | `pointer-events: none` didn't stop clicks on a block's text (spans carried no `Pickable`) | `src/build.rs` (text spans) | medium | fixed (`96f2cd3`) | new picking-driven signals tests (`tests/signals.rs`) |
| [bug_0021](bug_0021.md) | Despawning a UI in the same frame panicked `update_pseudo_states` (plain insert on a despawned element) | `src/signals.rs`, `src/focus.rs` | high | fixed (`ebfbf22`) | prototype_19 integration (headless smoke run) |
| [bug_0022](bug_0022.md) | In a windowed app, `autofocus` never took focus (Bevy focuses the window at startup; repair left it there) | `src/focus.rs` (`repair_focus`) | high | fixed (`35125b6`) | smoke run of the `menu` example |
| [bug_0023](bug_0023.md) | Wiring on `HtmlUiBuilt` stacked up once updates kept elements (quickstart clicks added 1, 2, 3 coins; demo documents duplicated) | examples + `HtmlUiBuilt` docs | high | fixed (`c8ab00f`) | user report |
| [bug_0024](bug_0024.md) | Right and middle clicks fired `data-on-click` (no button check) | `src/signals.rs` | medium | fixed (`4cef523`) | code reading (signal sources design) |
| [bug_0025](bug_0025.md) | `ui_state_machine_matches_model` fails ~1 in 10 runs (signals mismatch) | `tests/stateful.rs` / `src/signals.rs` | low | **open** | repeated full-suite runs |
| [bug_0026](bug_0026.md) | Editing an included template never updates its users (loader dependencies don't invalidate) | `src/template.rs` (`HtmlTemplateLoader`) | — | **not a bug** (withdrawn: the file watcher does reload users; only a manual `AssetServer::reload` of the dependency doesn't) | the template-dependency vector |
| [bug_0027](bug_0027.md) | `HtmlWorldAnchor` never shows without an `IsDefaultUiCamera` marker | `src/anchor.rs` | high | fixed (`61c0b94`) | `world` example smoke run |
| [bug_0028](bug_0028.md) | The demo's panels never scroll: the layout grows to the tallest content instead (`flex-shrink: 0` default vs the browser scroll idiom) | demo themes (`examples/assets/ui/themes/*.css`) | medium | fixed (uncommitted at filing) | user report |
| [bug_0029](bug_0029.md) | Links from a Fluent translation never match `:hover`/`:active`/`:focus` (fragment walks cascaded with no state; `run_extras` ignored state for every inline element) | `src/build.rs` (fragment state maps, `run_extras`) | medium | fixed (uncommitted at filing) | user report (`links` example) |
| [bug_0030](bug_0030.md) | `repair_focus` cleared focus the app set on an activation target (a link span) every frame | `src/focus.rs` (`repair_focus`) | medium | fixed (uncommitted at filing) | user report (`links` example) |
| [bug_0031](bug_0031.md) | A context change dropped unchanged siblings of the reused subtree (walk-through tags produced no spec) | `src/build.rs` (`collect_node` reuse) | high | fixed (`0ede9b6`) | code review 2026-10-13 (R02) |
| [bug_0032](bug_0032.md) | Reused subtrees kept the previous stylesheet's styles across a swap (+ the failing-sheet variant) | `src/build.rs` (`skeleton_eligible`) | high | fixed (`0ede9b6`) | code review 2026-10-13 (R03) |
| [bug_0033](bug_0033.md) | Builds that skipped the reuse walk served stale cached specs | `src/build.rs` (reuse gate) | high | fixed (`0ede9b6`) | code review 2026-10-13 (R04) |
| [bug_0034](bug_0034.md) | The skeleton's document fell out of step with its specs after Wait/outline frames | `src/build.rs` (skeleton document) | high | fixed (`0ede9b6`) | code review 2026-10-13 (R05) |
| [bug_0035](bug_0035.md) | Reused subtrees kept stale `DomNode` handles (`:hover`/`:active` died or hit the wrong element) | `src/build.rs` (`refresh_cached_handles`) | high | fixed (`0ede9b6`) | code review 2026-10-13 (R06) |
| [bug_0036](bug_0036.md) | The reuse gate scored the dynamic-header/static-body case useless and switched pairing off | `src/build.rs` (`spec_count`) | medium | fixed (`0ede9b6`) | code review 2026-10-13 (R07) |
| [bug_0037](bug_0037.md) | Any `type` attribute became an editable text field (the code read `data-type`) | `src/build.rs` (text-input mapping) | medium | fixed (`bbd0959`) | code review 2026-10-13 (R08) |
| [bug_0038](bug_0038.md) | A kept `<input>` was never updated (value/name/cascade; `maxlength`/`placeholder` retracted) | `src/build.rs` (`update_spec`) | medium | fixed (`bbd0959`) | code review 2026-10-13 (R09, R10) |
| [bug_0039](bug_0039.md) | A named grid-line placement on an `<input>` fell back to `auto` | `src/build.rs` (text-input branch) | low | fixed (`bbd0959`) | code review 2026-10-13 (R11) |
| [bug_0040](bug_0040.md) | Deeply nested content overflowed the stack (~150 nested `<div>`s abort) | `src/build.rs` (unbounded recursion) | high | fixed (`c07dda7`) | code review 2026-10-13 (R12) |
| [bug_0041](bug_0041.md) | A dropped root `calc()` kept resolving; a top-level root never resolved one | `src/build.rs` (`apply_root`, `resolve_calc`) | low | fixed (`c07dda7`) | code review 2026-10-13 (R13) |
| [bug_0042](bug_0042.md) | A restyle that respawned spans reported only `HtmlUiRestyled` (app wiring lost) | `src/build.rs` (`update_spec`) | low | fixed (`c07dda7`) | code review 2026-10-13 (R14) |
| [bug_0043](bug_0043.md) | Syncing `RemSize` queued a spurious restyle on the first build of every UI | `src/build.rs`/`src/rebuild.rs` | low | fixed (`c07dda7`) | code review 2026-10-13 (R17) |
| [bug_0044](bug_0044.md) | `var()` substitution was exponential and could hang the app on one element | `src/cascade.rs` (`substitute`) | high | fixed (`e992bf1`) | code review 2026-10-13 (R19) |
| [bug_0045](bug_0045.md) | Pseudo-only compounds never matched (`:focus-visible { … }` dropped) | `src/cascade.rs` (compound matching) | medium | fixed (`e992bf1`) | code review 2026-10-13 (R20) |
| [bug_0046](bug_0046.md) | CSS-wide keywords (`initial`/`inherit`/`unset`) silently ignored | `src/cascade.rs` | medium | fixed (`e992bf1`) | code review 2026-10-13 (R21) |
| [bug_0047](bug_0047.md) | Context variables referenced only transitively (`--a: var(--brand)`) never restyled (inline `style` names still untracked) | `src/cascade.rs` (`var_names`) | medium | fixed (`6195a88`, partly open) | code review 2026-10-13 (R22) |
| [bug_0048](bug_0048.md) | `restore_from` left the root positioned (`fixed`) and kept the areas' implied tracks | `src/cascade.rs` (`restore_from`) | medium | fixed (`6195a88`) | code review 2026-10-13 (R23) |
| [bug_0049](bug_0049.md) | Negative named-line occurrences off by one; out-of-range clamped instead of `auto` | `src/cascade.rs` (named lines) | low | fixed (`6195a88`) | code review 2026-10-13 (R24) |
| [bug_0050](bug_0050.md) | Grid names at the end of a `repeat()` or around `auto-fill` never registered | `src/cascade.rs` (track lists) | low | fixed (`6195a88`) | code review 2026-10-13 (R25) |
| [bug_0051](bug_0051.md) | A `border-image` URL behind a `var()` never loaded | `src/cascade.rs` (`image_urls`) | low | fixed (`49525d2`) | code review 2026-10-13 (R26) |
| [bug_0052](bug_0052.md) | An unsupported value erased the earlier rule's value | `src/cascade.rs` (declaration application) | low | fixed (`49525d2`) | code review 2026-10-13 (R27) |
| [bug_0053](bug_0053.md) | Later grid declarations never took the earlier rule's names back | `src/cascade.rs` (grid name table) | low | fixed (`6195a88`) | code review 2026-10-13 (R28) |
| [bug_0054](bug_0054.md) | Descendant-combinator matching was exponential in the ancestor chain (~600 ms/element) | `src/cascade.rs` (`matches_rest`) | low–medium | fixed (`49525d2`) | code review 2026-10-13 (R30) |
| [bug_0055](bug_0055.md) | `var()` in `pointer-events`/`outline-offset` treated as unsupported | `src/cascade.rs` (raw properties) | low | fixed (`e992bf1`) | code review 2026-10-13 (R31) |
| [bug_0056](bug_0056.md) | The Markdown Fluent cache misaligned arguments with their markers (control characters in the UI) | `src/l10n.rs` (`markdown_cache`) | high | fixed (`402362c`) | code review 2026-10-13 (R32) |
| [bug_0057](bug_0057.md) | String arguments were escaped twice through the Markdown cache | `src/l10n.rs` (`substitute`) | high | fixed (`402362c`) | code review 2026-10-13 (R33) |
| [bug_0058](bug_0058.md) | Code spans/blocks in `.md` templates ran through Tera (contradicting ADR 0020) | `src/md.rs` | high | fixed (`402362c`) | code review 2026-10-13 (R34) |
| [bug_0059](bug_0059.md) | `separate_statements` split inline statements; mis-ordered `{%`/`{#`; empty paragraphs from comments | `src/md.rs` (`separate_statements`) | medium | fixed (`402362c`) | code review 2026-10-13 (R35, R42) |
| [bug_0060](bug_0060.md) | Markdown conversion was chosen by the root template's path, not per file | `src/template.rs` (loader) | medium | fixed (`402362c`) | code review 2026-10-13 (R36) |
| [bug_0061](bug_0061.md) | Cached Markdown messages escaped term/message references as literal text | `src/l10n.rs` (Markdown cache) | medium | fixed (`402362c`) | code review 2026-10-13 (R37) |
| [bug_0062](bug_0062.md) | A placeable in a Markdown link destination killed the link (cache path) | `src/l10n.rs` (Markdown cache) | medium | fixed (`402362c`) | code review 2026-10-13 (R38) |
| [bug_0063](bug_0063.md) | A cache substitution failure errored (no per-value fallback); duplicate ids kept the later pattern | `src/l10n.rs` (Markdown cache) | medium | fixed (`402362c`) | code review 2026-10-13 (R39) |
| [bug_0064](bug_0064.md) | A freshly rendered document inherited the previous document's translations (bundle loading/failed) | `src/l10n.rs` (`localize`) | medium | fixed (`402362c`) | code review 2026-10-13 (R40) |
| [bug_0065](bug_0065.md) | Messages without placeables were Markdown-converted every frame | `src/l10n.rs` (Markdown cache) | low | fixed (`402362c`) | code review 2026-10-13 (R41) |
| [bug_0066](bug_0066.md) | Arguments containing a later literal part disturbed the substitution | `src/l10n.rs` (`substitute`) | low | fixed (`402362c`) | code review 2026-10-13 (R43) |
| [bug_0067](bug_0067.md) | `resolve_references` followed includes inside comments, raw blocks and code samples | `src/template.rs` (`resolve_references`) | low | fixed (`402362c`) | code review 2026-10-13 (R44) |
| [bug_0068](bug_0068.md) | A second matching bundle event in a frame was served a frame late (`events.read().any`) | `src/l10n.rs` (`localize`) | low | fixed (`402362c`) | code review 2026-10-13 (R45) |
| [bug_0069](bug_0069.md) | Template includes and FTL resources could escape the asset root with `../` paths | `src/template.rs`, `src/l10n.rs` (loaders) | low | fixed (`402362c`) | code review 2026-10-13 (R46) |
| [bug_0070](bug_0070.md) | Nine-slice manifests weren't validated (`Tile(0.0)` → infinite repeats) | `src/nine_slice.rs` (loader) | low | fixed (`402362c`) | code review 2026-10-13 (R47) |
| [bug_0071](bug_0071.md) | Every primary click inside a `<form data-on-submit>` submitted the form | `src/signals.rs` (`on_pointer`) | high | fixed (`cd21ffd`) | code review 2026-10-13 (R49) |
| [bug_0072](bug_0072.md) | A same-frame despawn panicked `update_focus_states` (plain `insert`) | `src/focus.rs` (`update_focus_states`) | high | fixed (`cd21ffd`) | code review 2026-10-13 (R50) |
| [bug_0073](bug_0073.md) | Activation never submitted forms; the `Submit` source was a hard-coded mouse click | `src/focus.rs`, `src/signals.rs` | medium | fixed (`cd21ffd`) | code review 2026-10-13 (R51, R58) |
| [bug_0074](bug_0074.md) | A tooltip on a link was respawned every frame and never shown | `src/tooltips.rs`, `src/anchor.rs` | medium | fixed (`cd21ffd`) | code review 2026-10-13 (R52) |
| [bug_0075](bug_0075.md) | The first-focusable fallback picked by `Entity` order, not document order | `src/focus.rs` (fallback) | medium | fixed (`cd21ffd`) | code review 2026-10-13 (R53) |
| [bug_0076](bug_0076.md) | Touch pointers never emitted `data-on-leave`; the entered map leaked per touch | `src/signals.rs` (hover tracking) | medium | fixed (`cd21ffd`) | code review 2026-10-13 (R54) |
| [bug_0077](bug_0077.md) | `Above`/`Left` overlays weren't clamped to the viewport | `src/anchor.rs` (`place_anchored`) | medium | fixed (`cd21ffd`) | code review 2026-10-13 (R55) |
| [bug_0078](bug_0078.md) | `HtmlWorldAnchor` ignored `UiScale` and the viewport's origin | `src/anchor.rs` (`place_world_anchored`) | medium | fixed (`cd21ffd`) | code review 2026-10-13 (R56, R57) |
| [bug_0079](bug_0079.md) | An app adding `DirectionalNavigationPlugin` after `BevyMarkupPlugin` panicked | `src/focus.rs`/`src/lib.rs` | medium | fixed (`cd21ffd`) | code review 2026-10-13 (R59) |
| [bug_0080](bug_0080.md) | Focus moved by a signal handler showed its state a frame late | `src/lib.rs`, `src/focus.rs` | low | fixed (`cd21ffd`) | code review 2026-10-13 (R60) |
| [bug_0081](bug_0081.md) | Pathological track counts panicked in debug builds; the areas cap is unguarded (TODO) | `src/cascade.rs` (track parsing, areas) | low | fixed (`d723f6a`) | follow-up review (F27) |
| [bug_0082](bug_0082.md) | `type` values were compared case-sensitively; the Submit half untested | `src/build.rs` (input mapping) | low | fixed (`ae27fc3`) | follow-up review (F28) |
| [bug_0083](bug_0083.md) | The stale refresh gave up when a reused subtree held an app-nested root | `src/build.rs` (`refresh_cached_handles`) | low | fixed (`700d047`); no failing shape found | follow-up review (F29) |
| [bug_0084](bug_0084.md) | A Junk message in an FTL resource skipped the whole Markdown cache | `src/l10n.rs` (`markdown_cache`) | low | fixed (`73ae77f`) | follow-up review (F31) |
| [bug_0085](bug_0085.md) | A grid name on a merged repeat boundary registered twice | `src/cascade.rs` (`GridNames`) | low | fixed (`8512b7a`); the dedupe was quadratic (G28) | follow-up review (F39) |
| [bug_0086](bug_0086.md) | The per-value Markdown path double-escaped string args | `src/l10n.rs` | low | fixed (`15b4255`); that fix injected markup (bug_0088) | follow-up review (F40) |
| [bug_0087](bug_0087.md) | The unapproved-path check ignores `unapproved_path_mode`; open, documented (the 'no API' premise is false) | `src/template.rs`, `src/l10n.rs` | low | open — documented deviation | follow-up review (F32) |
| [bug_0088](bug_0088.md) | Fluent string args injected raw HTML on the Markdown per-value path | `src/l10n.rs` | high | fixed (`754f910`) | review_0003 (G01) |
| [bug_0089](bug_0089.md) | CI failed: -D warnings errors; two regression tests silently disabled | tests/, `src/` | high | fixed (`aa12dd6`) | review_0003 (G02) |
| [bug_0090](bug_0090.md) | --features fuzzing didn't compile | `src/fuzz.rs` | high | fixed (`ef16397`) | review_0003 (G03) |
| [bug_0091](bug_0091.md) | Crafted markers panicked inject_class | `src/md.rs` | medium | fixed (`5969530`) | review_0003 (G04) |
| [bug_0092](bug_0092.md) | A few KB of CSS aborted the app (var()/paren recursion) | `src/cascade.rs` | high | fixed (`6343345`) | review_0003 (G08) |
| [bug_0093](bug_0093.md) | The template scanner's code protection disagreed with pulldown | `src/template.rs` | medium | fixed (`2e14ba8`) | review_0003 (G13) |
| [bug_0094](bug_0094.md) | The template scanners were quadratic on load | `src/template.rs`, `src/md.rs` | medium | fixed (`63c6c49`) | review_0003 (G14) |
| [bug_0095](bug_0095.md) | resolve_calc took &mut through Mut<Node> unconditionally; no failing guard shape | `src/build.rs` | medium | fixed (`100dd03`) | review_0003 (G15) |
| [bug_0096](bug_0096.md) | libFuzzer hangs could never be reported (no -timeout) | `scripts/fuzz-libfuzzer.sh` | low | fixed (`6a732bf`) | review_0003 (G21) |
| [bug_0097](bug_0097.md) | A theme swap while the outline was shown kept the old theme | `src/build.rs` (skeleton gate) | medium | fixed (`16bef4a`) | review_0003 (G26) |
| [bug_0098](bug_0098.md) | A reuse build forced two full restyles per content change | `src/build.rs` (inline_vars) | medium | fixed (`0ed28a1`) | review_0003 (G27) |
| [bug_0099](bug_0099.md) | Grid-name registration was quadratic in the name count | `src/cascade.rs` | medium | fixed (`a0547c8`) | review_0003 (G28) |
| [bug_0100](bug_0100.md) | Consumed custom declarations were substituted per element | `src/cascade.rs` | medium | fixed (`36fb457`) | review_0003 (G29) |
| [bug_0101](bug_0101.md) | F14's inline-var tracking missed runs, translations, non-ASCII names | `src/build.rs`, `src/cascade.rs` | medium | fixed (`2183260`) | review_0003 (G30) |
| [bug_0102](bug_0102.md) | The ancestor chain was cloned per child; inline nesting was uncapped and quadratic | `src/build.rs` (Ancestor, run_walk) | medium | fixed (`944e8fa`) | review_0003 (G05+G06) |
| [bug_0103](bug_0103.md) | Code inside a `{% raw %}` block rendered as brace garbage | `src/md.rs` (rewrite) | medium | fixed (`bde26f8`) | review_0003 (G07) |
| [bug_0104](bug_0104.md) | A skipped custom URL shifted the stylesheet's image pairing | `src/style.rs` | medium | fixed (`bc01dc5`) | review_0003 (G09) |
| [bug_0105](bug_0105.md) | The `fuzzing` feature stopped compiling after the `Ancestor` rework | `src/fuzz.rs` | high | fixed (`ce19a69`) | manual fuzz sweep; review_0004 (H01) |
| [bug_0106](bug_0106.md) | Nested style rules still overflowed lightningcss's parser and aborted the app | `src/cascade.rs`, `src/style.rs` | high | fixed (uncommitted) | review_0004 (H02) |
| [bug_0107](bug_0107.md) | Fluent string args forged links and images on the Markdown per-value path | `src/l10n.rs` | medium | fixed (uncommitted) | review_0004 (H03) |
| [bug_0108](bug_0108.md) | The Markdown template scanners' code rules disagreed with CommonMark | `src/md.rs`, `src/template.rs` | medium | fixed (uncommitted) | review_0004 (H04), review_0003 (G17, G18) |
| [bug_0109](bug_0109.md) | Debug prints shipped in library code | `src/md.rs`, `src/template.rs` | medium | fixed (uncommitted) | review_0004 (H05) |
| [bug_0110](bug_0110.md) | calc() on a direct child of a top-level HtmlUi root never resolved | `src/build.rs` | medium | fixed (uncommitted) | review_0004 (H07) |
| [bug_0111](bug_0111.md) | Viewport units in a nested calc() measured the root's box, not the viewport | `src/build.rs` | low | fixed (uncommitted) | review_0004 (H08) |
| [bug_0112](bug_0112.md) | A root's calc() used the last camera when none was marked | `src/build.rs` | medium | fixed (uncommitted) | review_0003 (G10) |
| [bug_0113](bug_0113.md) | text-input headless apps panicked on the first update (PointerState) | `src/lib.rs` | medium | fixed (uncommitted) | review_0003 (G11) |
| [bug_0114](bug_0114.md) | An empty data-on-submit made its buttons swallow clicks | `src/build.rs` | low | fixed (uncommitted) | review_0003 (G12) |
| [bug_0115](bug_0115.md) | An invalid `<button type>` didn't submit | `src/build.rs` | low | fixed (uncommitted) | review_0003 (G16) |
| [bug_0116](bug_0116.md) | An invalid `<input type>` built no field | `src/build.rs` | low | fixed (uncommitted) | review_0003 (G36) |
| [bug_0117](bug_0117.md) | A var() cycle invalidated only the reference closing it | `src/cascade.rs` | low | fixed (uncommitted) | review_0002 (F25) |
| [bug_0118](bug_0118.md) | Content could load assets from network sources | `src/template.rs`, `src/l10n.rs`, `src/nine_slice.rs`, `src/style.rs` | low | fixed (uncommitted) | review_0003 (G19) |
| [bug_0119](bug_0119.md) | The documented data-l10n-args pattern allowed argument injection | README, docs, examples | low | fixed (uncommitted) | review_0003 (G20) |
| [bug_0120](bug_0120.md) | Unbounded repetition driven by content | `src/l10n.rs`, `src/custom_elements.rs`, `src/build.rs` | low | fixed (uncommitted) | review_0003 (G22) |
| [bug_0121](bug_0121.md) | A grid track count could wrap on 32-bit targets | `src/cascade.rs` | low | fixed (uncommitted) | review_0003 (G38) |
| [bug_0122](bug_0122.md) | The examples' linear navigation picked the last modal, not the topmost | `examples/shared/input.rs` | low | fixed (uncommitted) | review_0003 (G40, G41) |
| [bug_0123](bug_0123.md) | Esc in the menu's Reset dialog sent focus to Volume | `examples/menu/main.rs` | low | fixed (uncommitted) | review_0003 (G42) |
| [bug_0124](bug_0124.md) | honggfuzz/test-fuzz scripts exited 0 after finding crashes | `scripts/` | low | fixed (uncommitted) | review_0003 (G43) |
| [bug_0125](bug_0125.md) | Example unit tests (grid-names) never ran | `Cargo.toml`, CI | low | fixed (uncommitted) | review_0003 (R64) |
| [bug_0126](bug_0126.md) | Indented Tera block bodies in Markdown templates rendered as code blocks | `src/md.rs` | medium | fixed (uncommitted; ADR 0024) | review_0004 (H11) |
| [bug_0127](bug_0127.md) | Indented raw HTML container bodies in Markdown templates rendered as code blocks | `src/md.rs` | medium | fixed (uncommitted; ADR 0025) | ADR 0024's open item |
| [bug_0128](bug_0128.md) | Tera-printed args with emoji or combining marks fell back to the untranslated content | `src/template.rs` (+ l10n, signals, tooltips) | high | fixed (uncommitted) | testbench example (`hostile-args`) |
| [bug_0129](bug_0129.md) | A stylesheet swap to a failing sheet restyled scoped, keeping stale styles (found as an `inline_backgrounds` flake) | `src/build.rs`, `src/rebuild.rs` | medium | fixed (`4b02251`) | the ADR 0027 test waves |
| [bug_0130](bug_0130.md) | `+` matched the farthest preceding sibling instead of the adjacent one | `src/cascade.rs` | medium | fixed (`e3646ae`) | browser oracle (`sibling_combinators`) |
| [bug_0131](bug_0131.md) | A tiled `background-image` divided by zero and drew a solid rim | `src/build.rs` | high | fixed (`e3646ae`) | `background` golden scene |
| [bug_0132](bug_0132.md) | `--features fuzzing` didn't compile: the harness missed `style_of`/`box_of`'s element state | `src/fuzz.rs` | high | fixed (uncommitted) | review_0005 I01 |
| [bug_0133](bug_0133.md) | `:nth-child(n-2147483647)` panicked with an i32 overflow | `src/cascade.rs` | high | fixed (uncommitted) | review_0005 I02 |
| [bug_0134](bug_0134.md) | Long sibling lists built in O(n²) time and memory | `src/build.rs` | medium | fixed (uncommitted) | review_0005 I03 |
| [bug_0135](bug_0135.md) | The entity decoder was quadratic on `&#` references without a `;` | `src/template.rs` | medium | fixed (uncommitted) | review_0005 I04 |
| [bug_0136](bug_0136.md) | Content reuse kept stale matches for sibling- and position-dependent rules | `src/build.rs` | medium | fixed (uncommitted) | review_0005 I05 |
| [bug_0137](bug_0137.md) | The style cache's key omitted the dataset and the preceding siblings | `src/cascade.rs` | medium | fixed (uncommitted) | review_0005 I06 |
| [bug_0138](bug_0138.md) | Sibling and ancestor compounds ignored pseudo-classes, attributes and structural parts | `src/cascade.rs` | medium | fixed (uncommitted) | review_0005 I07 |
| [bug_0139](bug_0139.md) | `:not()` around a pseudo-class never matched | `src/cascade.rs` | medium | fixed (uncommitted) | review_0005 I08 |
| [bug_0140](bug_0140.md) | `overflow: auto` stopped scrolling after any in-place update | `src/build.rs` | medium | fixed (uncommitted) | review_0005 I09 |
| [bug_0141](bug_0141.md) | A debug `println!` printed every frame in apps without a default stylesheet | `src/build.rs` | medium | fixed (uncommitted) | review_0005 I10 |
| [bug_0142](bug_0142.md) | `:nth-child(+n+3)` was rejected | `src/cascade.rs` | low | fixed (uncommitted) | review_0005 I11 |
| [bug_0143](bug_0143.md) | Every build appended a line to `/tmp/bevy_markup_buildlog.txt` | `src/build.rs` | high | fixed (uncommitted) | review_0005 (I16, mutation audit) |
| [bug_0144](bug_0144.md) | `:nth-child(-n+3)` and quoted attribute values with spaces, `+`, `]` or quotes failed | `src/cascade.rs` | medium | fixed (uncommitted) | writing the specificity test in the review_0005 fix session |
| [bug_0145](bug_0145.md) | Structural pseudo-classes added nothing to specificity | `src/cascade.rs` | medium | fixed (uncommitted) | reading `Compound::specificity` in the review_0005 fix session |
| [bug_0146](bug_0146.md) | `&#+65;` decoded as `A` | `src/template.rs` | low | fixed (uncommitted) | the bug_0135 fix |
| [bug_0147](bug_0147.md) | Inline elements had no sibling list, and an `<img>` in a container always counted as an only child | `src/build.rs` | low | fixed (uncommitted) | the bug_0134 refactor |
| [bug_0148](bug_0148.md) | Two flip tests inserted `PseudoState` by hand under picking, which reset it | `tests/html_ui.rs` | low | fixed (uncommitted) | the bug_0138 fix |
| [bug_0149](bug_0149.md) | `style::tests::skipped_custom_urls_do_not_shift_the_pairing` flaked under load | `src/style.rs` | low | fixed (uncommitted) | a full `cargo test --lib --features fuzzing` run in the review_0005 fix session |
| [bug_0150](bug_0150.md) | Content reuse worked only for the document's top-level elements | `src/build.rs` | medium | fixed (uncommitted) | review_0005 fix session (mutation run) |
| [bug_0151](bug_0151.md) | A windowed UI's band drifts off the viewport while scrolling (coalesced spacers dropped the row gaps) | `src/build.rs` (the windowed build) | high | fixed (uncommitted) | the project owner (the interactive wheel scroll); root cause: review_0006 (J01) |
| [bug_0152](bug_0152.md) | A windowed UI culled nothing inside a container | `src/build.rs` | high | fixed (uncommitted) | review_0006 (J02) |
| [bug_0153](bug_0153.md) | A windowed root with padding, a border or a HiDPI scale rebuilt every frame and culled nothing | `src/build.rs` | high | fixed (uncommitted) | review_0006 (J03) |
| [bug_0154](bug_0154.md) | A windowed UI stayed fully built until it was scrolled | `src/build.rs` | medium | fixed (uncommitted) | review_0006 (J08) |
| [bug_0155](bug_0155.md) | Every layout-harness test panicked under `--features text-input` | `tests/common/mod.rs` | medium | fixed (uncommitted) | review_0006 (J09) |
| [bug_0156](bug_0156.md) | Scrolling a windowed UI reused nothing: every scroll frame re-collected the whole band | `src/build.rs` | medium | fixed (uncommitted) | review_0006 (J04) |
| [bug_0157](bug_0157.md) | Every sub-pixel scroll rebuilt a windowed UI, and a taller viewport never did | `src/build.rs` | low | fixed (uncommitted) | review_0006 (J05) |
| [bug_0158](bug_0158.md) | Non-windowed UIs paid the windowing measurement every frame | `src/build.rs` | low | fixed (uncommitted) | review_0006 follow-up |
| [bug_0159](bug_0159.md) | The testbench's scripted runs changed their own size | `examples/testbench/main.rs` | low | fixed (uncommitted) | review_0006 follow-up |
| [bug_0160](bug_0160.md) | A text change stopped a windowed UI culling at the first loose text | `src/build.rs` | low | fixed (uncommitted) | the windowing property (review_0006 follow-up) |
| [bug_0161](bug_0161.md) | A changed row above the band left its old height in the spacer, displacing the visible rows | `src/build.rs` | high | fixed (uncommitted) | the windowing property (review_0006 follow-up) |
| [bug_0162](bug_0162.md) | A refused cull whose height didn't change stayed built | `src/build.rs` | low | fixed (uncommitted) | the windowing property (review_0006 follow-up) |
| [bug_0163](bug_0163.md) | A cleared height model was never measured again | `src/build.rs` | medium | fixed (uncommitted) | the windowing property (review_0006 follow-up) |
| [bug_0164](bug_0164.md) | An overshooting scroll offset culled the whole viewport | `src/build.rs` | high | fixed (uncommitted) | the windowing property (review_0006 follow-up) |
| [bug_0165](bug_0165.md) | An unmeasured child didn't stop its container's cursor | `src/build.rs` | high | fixed (uncommitted) | the windowing property (review_0006 follow-up) |
| [bug_0166](bug_0166.md) | Heights a content change dropped were never measured again | `src/build.rs` | medium | fixed (uncommitted) | the windowing property (review_0006 follow-up) |
| [bug_0167](bug_0167.md) | Low reuse turned off the pairing that carries a windowed UI's heights | `src/build.rs` | medium | fixed (uncommitted) | the windowing property (review_0006 follow-up) |
| [bug_0168](bug_0168.md) | One inserted or removed row materializes a windowed document for a frame | `src/build.rs` | medium | fixed (uncommitted; ADR 0033) | the windowing property (review_0006 follow-up) |
| [bug_0169](bug_0169.md) | A subtree reused whole after a handle shift kept its old window key | `src/build.rs` | high | fixed (uncommitted) | the windowing property (review_0006 follow-up) |
| [bug_0170](bug_0170.md) | Content that changes height above the band shifts the visible rows for one frame | `src/build.rs` | low | open | the windowing property (review_0006 follow-up) |
| [bug_0171](bug_0171.md) | A container that stopped being a grid or row never culled its children again | `src/build.rs` | low | fixed (uncommitted) | the windowing property (review_0006 follow-up) |
| [bug_0172](bug_0172.md) | A document's `<body>` lost its style and its place in selectors | `src/build.rs` | high | fixed (uncommitted) | the windowing mutation run (review_0006 follow-up) |
| [bug_0173](bug_0173.md) | A windowed root that changed width kept its old heights | `src/build.rs` | high | fixed (uncommitted) | the windowing mutation run (review_0006 follow-up) |
| [bug_0174](bug_0174.md) | A top-level element's percent margins measured as zero | `src/build.rs` | high | fixed (uncommitted) | the windowing mutation run (review_0006 follow-up) |
| [bug_0175](bug_0175.md) | A windowed whole document (`<html><head>…<body>`) never culled | `src/build.rs` | medium | fixed (uncommitted) | the windowing mutation run (review_0006 follow-up) |
| [bug_0176](bug_0176.md) | Spacers drifted by the layout's per-node rounding | `src/build.rs` | low | fixed (uncommitted) | the windowing mutation run (review_0006 follow-up) |
| [bug_0177](bug_0177.md) | Reversed containers and non-column roots were culled as columns | `src/build.rs` | high | fixed (uncommitted) | the windowing mutation rerun (review_0006 follow-up) |

## Discovery session metadata: bug_0001–0005

### When / where

- **Date:** 2026-10-04, 09:00–10:30 +04:00
- **Machine:** hostname `anne`; Arch Linux, kernel `7.2.7-arch1-1`
  (`#1 SMP PREEMPT_DYNAMIC Mon, 21 Sep 2026 18:51:14 +0000`), x86_64
- **CPU:** AMD Ryzen 7 9800X3D 8-Core (16 threads)
- **Memory:** 60 GiB

### Repository state

- **Repo:** `p23` (Bevy 0.19 HTML/CSS/Fluent UI library), working tree at
  `/home/a/fs/Bevy/PROTOTYPE_23/p23`
- **Session start HEAD:** `969556c` — "Check test vectors against real browser"
- All bugs were found in uncommitted working-tree state built on top of that
  commit; the fixes landed in the session's commits:
  `c59357f` → `042782f` (see each bug file for its exact fix commit).

### Toolchain

- rustc stable `1.98.1 (48a229cea 2026-09-01)` (library, tests)
- rustc nightly `1.100.0-nightly (4aa1fbcf4 2026-09-08)` (fuzz drivers)
- cargo-fuzz `0.13.2` (libFuzzer + AddressSanitizer)
- honggfuzz-rs `0.5.62` (vendored native honggfuzz, patched `bfd.c`)

### Dependency versions under test

- bevy `0.19.1` (minimal features: `ui_api`, `default_font`, `bevy_log`)
- bevy_fluent `0.15`, fluent `0.16.1`, fluent-syntax `0.11.1`
- lightningcss `1.0.0-alpha.72`, tera `2.4.0`, tl `0.7.8`

### Discovery agent

- Model: **`zai/glm-5.3-flash:high`** (LLM coding assistant), driving the
  fuzzers/tests interactively; every reported crash was reproduced
  deterministically and minimized before a fix was written.

### Common contract for the discovery harnesses

All fuzz targets drive the feature-gated `p23::fuzz` harness
(`#[doc(hidden)]`, `--features fuzzing`): arbitrary `&str` inputs may return
`Err` or produce any output, but must never panic, hang, or abort. The
state-machine tests (`tests/stateful.rs`) require each applied operation to
settle (rebuild observed) and the resulting dump to match a reference model.

## Discovery session metadata: bug_0006–0007

- **Date:** 2026-10-04, ~12:00 +04:00; same machine (`anne`) and Rust
  toolchain/dependency versions as above.
- **Session start HEAD:** `1169ca4` — "Add scripts for running fuzzers";
  found and fixed in the working tree on top of it, committed as `94a4fb9`.
- **Harness:** the Fluent oracle — `scripts/fluent_oracle.sh` runs
  `@fluent/dom` 0.10.2 + `@fluent/bundle` 0.19.1 in jsdom 30.1.1 (Node
  v26.10.0) over `tests/vectors/fluent_*`, writing `fluent.html`; the
  `fluent_oracle` test requires p23's localized build to equal p23's build of
  that reference DOM.
- **Discovery agent:** model **`anthropic/claude-opus-5-5:high`**. Of the
  first run's eight differences, two were these bugs; the other six were
  triaged with the user as deliberate (no sanitizing, escaped string args,
  fluent-rs number formatting) and are pinned by hand-written vectors.

## Discovery session metadata: bug_0008–0013

- **Date:** 2026-10-04, ~12:00–13:00 +04:00; same machine and toolchain.
- **Session start HEAD:** `e1b910c` — "Add browser based layout oracle".
- **Harnesses:** proptest-stateful ops for `HtmlStylesheet`/`FontFamilies`, a structure-aware arbtest document model (PropertyTests subagent), and the layout oracle extended to flex/sizes/margins.
- **Discovery agent:** `anthropic/claude-opus-5-5:high` with subagents.

## Discovery session metadata: bug_0020

- **Date:** 2026-10-05; same machine (`anne`) and toolchain.
- **Session start state:** working tree mid-development (uncommitted
  signals/focus work on top of the signals bindings).
- **Harness:** the new picking-driven test harness — `TestUi::with_pointer`
  (`tests/common/mod.rs`) spawns a primary window and aims the layout camera
  at it, then the tests write real `WindowEvent`s (`CursorMoved`,
  `MouseButtonInput`) so Bevy's full picking stack runs end to end
  (`PointerInputPlugin` → UI picking backend → pointer events → bevy_markup's
  observers and hover tracking). The `pointer-events: none` test failed on
  its first run; component dumps (`Pickable` was correctly on the block and
  its text node) pinned the miss to the span entities.
- **Discovery agent:** model `zai/glm-5.3-flash:high`.

## Discovery session metadata: bug_0016

- **Date:** 2026-10-05, ~01:50 +04:00; same machine (`anne`) and toolchain.
- **Session start HEAD:** `955e6ea` — "Use system fonts for examples, add
  ./examples/assets", plus the uncommitted HTML-only demo rewrite (a
  full-screen shell `HtmlUi` with content documents nested into its slots).
- **Harness:** the user ran `cargo run --example demo` and clicked a language
  button; the agent minimized it to a headless vector
  (`nested_ui_under_a_rebuilding_ancestor_is_skipped_not_panics`) that
  panicked identically.
- **Discovery agent:** model `zai/glm-5.3-flash:high`.

## Discovery session metadata: bug_0017

- **Date:** 2026-10-05, morning +04:00; same machine (`anne`) and toolchain.
- **Session start HEAD:** `8952749` — "Add ElementSignal bindings in HTML",
  plus the uncommitted `:hover`/`:active` work; fixed with it in `d079b08`.
- **Harness:** the demo crashed at startup (no input); traced with
  `--features bevy/debug` and `bevy_markup=debug` rebuild-decision logs, then
  pinned by the vector `restyle_with_nested_uis_keeps_them`).
- **Discovery agent:** model `zai/glm-5.3-flash:high`.

## Discovery session metadata: code review 2026-10-13 (reviews/review_0001.md)

- **Date:** 2026-10-13; same machine (`anne`, kernel `7.2.8-arch1-2`).
- **Repository state:** `bevy_markup` at `b224619` ("Bounded headless
  example runs; …"), clean tree on `main`.
- **Method:** six parallel review slices (build/skeleton reuse,
  cascade/fonts, interaction, templates/Markdown/Fluent/loaders,
  core wiring/rebuild, examples/scripts/CI) reading against AGENTS.md and
  the ADRs, each free to run throwaway probes in scratch copies; the
  integrator re-read the highest-impact claims directly. Evidence levels
  per finding: [probe] reproduced, [code] verified by reading,
  [traced] path traced without execution.
- **Fix status:** a fix session later that day (six per-slice branches,
  `fix/reuse`, `fix/build`, `fix/cascade`, `fix/content`,
  `fix/interaction`, `fix/examples`) landed the fixes: merged to `main`
  the same day (merge base `aed8fbf`, HEAD `97e16bc`). Findings with their
  own report: R02–R12, R13–R14, R17, R19–R28, R30–R47, R49–R60 →
  bug_0031–bug_0080 (sequential in review order). Not filed here: R01/R18
  (test-infrastructure fixes), R10 (doc retraction, folded into
  bug_0038/ADR 0022), R15/R16/R29 (internal perf, no observable defect),
  R48 (fuzz-harness coverage), R58 (folded into bug_0073),
  R61–R75 (example/CI-only, fixed in the same pass),
  R76–R80 (stale docs, fixed by the doc edits).
- **Discovery agent:** model `anthropic/claude-opus-5-5:high` with six
  `reviewer` subagents.
- **Fix sessions for the 2026-10-13 reviews** (opus reviewer; two fix
  sessions, then review_0003's audit + a third fix wave — see
  review_0003's Coverage/Status sections; its records corrections are
  G23–G25):
  - New defect reports: bug_0081 (F27), bug_0082 (F28), bug_0083 (F29),
    bug_0084 (F31), bug_0085 (F39), bug_0086 (F40), bug_0087 (F32,
    open — a documented deviation whose "no API" premise is FALSE, see
    the file). review_0003's G-series fixes are filed as bug_0088–
    bug_0104 (G01–G04, G08, G13–G15, G21, G26–G30, then G05+G06, G07,
    G09 as bug_0102–bug_0104). bug_0088–bug_0095 were listed here but
    never committed; review_0004's fix session wrote them (H06).
  - **The F→fix mapping** (authoritative; review_0002's Status claimed a
    mapping existed here — it didn't):
    - Re-findings of an R finding or its test: F04→R01 (bug_0031/0036),
      F05→R12 (bug_0040), F07→R55 (bug_0077), F08→R34 (bug_0058),
      F09→R49 (bug_0071), F12→R44 (bug_0067), F13→R44 (bug_0067),
      F14→R39 (bug_0063), F15→R27 (bug_0052), F16→R56 (bug_0078),
      F17→R63 (review_0002), F19→ADR 0022/0023, F20→R27 (bug_0052),
      F25→R27 (bug_0052), F28→R08 (bug_0037), F38→R27 (bug_0052),
      F39→R36 (bug_0060), F40→R35 (bug_0059), F42→R67 (bug_0069 n/a,
      test-only), F43→R69 (review_0002).
    - Internal-only (no bug file): F01/F02 (CI blockers), F03/F06 (part
      of the depth-cap work), F10/F11/F13/F22/F23/F26/F27/F29/F30/F31/
      F33/F34/F35/F36/F37/F38/F39 (fixed in the F-wave commits, each
      named in its subject), F04/F21/F41/F42 (test/CI work), F18/F19
      (bug_0079 class / ADR 0023), F24 (build.rs), G-series →
      review_0003.

## Discovery session metadata: review_0004 and its fix session (2026-10-15)

- **Review:** [review_0004](../reviews/review_0004.md) audited the G-fix
  session (`4717e13..4776aea`) at `4776aea`: CI's exact commands in a
  worktree, behaviour probes and guard reverts in a scratch worktree.
- **Reports:** bug_0105 (H01, filed by the user's session that fixed it,
  `glm-5.3-flash`), bug_0106–bug_0111 (H02–H05, H07, H08),
  bug_0112–bug_0125 (G-series and F25/R64 findings review_0003 left
  open), bug_0126 (H11, decided by the user: dedent, ADR 0024), bug_0127 (the
  same for HTML containers, ADR 0025), bug_0128 (found by the new testbench
  example); bug_0088–bug_0095 written retroactively (H06).
- **Discovery and fix agent:** Claude Code session, `claude-opus-5-5`
  (same model for review and fixes). Each fix's regression test was
  checked by reverting exactly that fix and watching the test fail,
  except where a report says otherwise (bug_0095, bug_0109, bug_0121,
  bug_0123, bug_0124).
- **Machine/toolchain:** as for review_0003 (Arch Linux, rustc 1.98.1,
  Bevy 0.20.0).


## Discovery session metadata: the windowing property (2026-10-16)

- **Tool:** `tests/windowing_model.rs`, written in the review_0006 follow-up:
  random documents (paragraphs of three heights, margins, nested padded
  and bordered sections, loose text, tall blocks, `{% if f %}` rows) × 8
  random steps (text changes that may wrap, row flips, small/medium/jump
  scrolls), a windowed and an unwindowed build of the same document
  stepped in lockstep and compared in every frame (placement, coverage,
  unique window keys; tightness once settled). Failing seeds are kept in
  `tests/windowing_model.proptest-regressions` and minimized by hand
  (a page-file delta debugger) into the named regression tests.
- **Reports:** bug_0160–bug_0171 (bug_0168 and bug_0170 open); bug_0172–bug_0177 from the mutation runs over the windowing diff (`902d81c..`, 323 then 330 mutants), whose survivors each got a test or a documented reason.
- **Discovery and fix agent:** Claude Code session, `claude-opus-5-5`.
  Every fix was ablated against the suite (the property at 128 cases plus
  the regressions); each fails it. `Changed<WindowKey>` in the
  measurement's filter survives: every handle shift found so far also
  changes the kept entities' `ComputedNode::stack_index`, which
  re-measures them anyway — kept as defense in depth.
- **Commit at discovery:** `2678c8f` (all fixes uncommitted at filing).
- **Machine/toolchain:** Arch Linux (kernel 7.2.8), rustc 1.98.1, Bevy
  0.20.0.
