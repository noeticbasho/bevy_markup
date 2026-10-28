# bevy_markup

Bevy 0.20 library: HTML templates (Tera) + Fluent + CSS → Bevy UI, with
9-slice frames. Dependencies are limited to that; don't add unrelated crates.
A maintained crate in that domain that replaces code the crate maintains
is in scope (ADR 0036: `htmlize` replaced the generated entity table).

- `cargo run --example quickstart` — the API in one small app.
- `cargo run --example grid` — CSS grid layout (page grid, responsive slots).
- `cargo run --example demo` — everything: themes, languages, DOM outlines,
  scrolling, 9-slice frames.
- `cargo doc --open` — the user-facing documentation (crate docs = guide).
- `docs/agents/adr/` — Architecture Decision Records: *why* the code is the
  way it is (this file says *what* it is). Read the relevant record before
  reversing a design decision; write a new one (`docs/agents/skills/adr.md`)
  when you make one.

The library depends on `bevy` with `default-features = false` and only the
features its code uses (`ui_api`, `default_font`, `bevy_log`, `bevy_picking`
for interaction signals and `:hover`/`:active`, `bevy_ui_widgets` behind the
`text-input` cargo feature; see `Cargo.toml`). Rendering,
windowing, picking backends, widgets and image formats are the
app's choice. Examples get full Bevy via `[dev-dependencies]`, whose features
never reach library users. A missing feature shows up in `cargo check --lib`.

## Public API (`bevy_markup::prelude::*`)

| Item | Kind | Role |
|---|---|---|
| `BevyMarkupPlugin` | Plugin | loaders, resources, systems |
| `HtmlUiSystems::{Render, Localize, Build}` | SystemSet | chained in `PostUpdate`, before `UiSystems::Prepare` |
| `HtmlUi(Handle<HtmlTemplate>)` | Component | the UI; requires `Node`, `TemplateContext`, `RenderedHtml`, `LocalizedText` |
| `TemplateContext(tera::Context)` | Component | Tera variables; `.with(k, &v)` builder; Deref to `tera::Context`; mutate → re-render (rebuild only if the HTML changed) |
| `HtmlStylesheet(Handle<Stylesheet>)` | Component | per-entity stylesheet override |
| `HtmlDebugOutline` | Component | show the DOM outline (styled like `pre`) instead of the UI |
| `RenderedHtml` | Component | `Pending` / `Ready(HtmlDocument)` / `Failed(msg)` (read-only) |
| `HtmlElement { tag, id, classes, dataset }` | Component | on each spawned block and container node; `dataset` = `data-*` attributes (prefix stripped), `data(key)`; signals expose it as `ElementSignal::data` |
| `HtmlUiBuilt { entity }` | EntityEvent | after a content update (or a restyle that spawned nodes or respawned an element's spans); kept elements keep their entities *and what was attached*, so handlers must be idempotent (no `observe` here: it stacks per update — use `data-on-*` or `is=`) |
| `HtmlUiRestyled { entity }` | EntityEvent | after a style-only change applied in place (entities and attached components kept); a restyle that respawns an element's spans reports `HtmlUiBuilt` instead (new span entities: the kept-everything promise doesn't hold) |
| `HtmlElements` | SystemParam | `iter` / `by_id` / `by_class` / `by_tag` below an `HtmlUi` |
| `DefaultStylesheet(Option<Handle<Stylesheet>>)` | Resource | stylesheet for `HtmlUi`s without an override; swap = theme |
| `ActiveLocale(Option<Handle<LocaleBundle>>)` | Resource | Fluent bundle; `None` = no localization; swap = language |
| `FontFamilies` / `FontFaces` / `GenericFamily` | Resource + types | CSS `font-family` name → faces: font files or system families (any `FontSource`; for system families bold/italic are requested via `TextFont` weight/style), + generic keyword mapping. A CSS list resolves to every registered family in order and composes Bevy 0.20's per-glyph fallback list (ADR 0015) |
| `HtmlTemplate`, `Stylesheet`, `NineSlice`, `LocaleBundle` | Assets | `.html`/`.htm`/`.md`/`.markdown`, `.css`, `*.slice.ron`, `*.ftl.ron` |
| `NineSliceFrame(Handle<NineSlice>)` | Component | 9-slice image as a node's border-box background (non-HTML nodes; HTML uses CSS `border-image`) |
| `HtmlCustomElementsExt::define_html_element(name, system)`, `ElementConnected { entity, root, name, dataset }` | App ext, system input | `is="<name>"` customized built-ins: the system (`In<ElementConnected>`, `data-*` attributes as `dataset`) runs on every spawn of the element, in document order, before `HtmlUiBuilt`; not on restyles |
| `HtmlWorldAnchor { target, offset, pivot, camera }`, `HtmlWorldAnchorView { distance, on_screen }` | Component | keeps a UI root's `pivot` (fraction of its size, default bottom center) over `target`'s translation + `offset` projected through `camera` / its `UiTargetCamera` / the default UI camera (viewport-relative and `UiScale`-divided: offset viewports and scale factors read correctly); owns its `Visibility` (hidden behind the camera, off screen, or over an invisible target); despawned with `target`; `place_world_anchored`, `PostUpdate` before `UiSystems::Prepare` (last frame's transforms) |
| `HtmlAnchor { element, placement, gap }`, `AnchorPlacement::{Right, Left, Above, Below}` | Component | keeps an (absolute) overlay node beside `element` each frame: insets from its rect, clamped to the viewport for every placement (Right/Below by the overlay's size; Above/Left measured from the far edge, clamped once that size is known), `UiTargetCamera` copied, despawned with the element |
| `HtmlSignalsExt::on_html_click(name, system)` / `on_html_signal(name, system)` | App ext | route `ElementSignal`s by name to systems taking `In<ElementSignal>` (`on_html_click`: `Click` only); run by `dispatch_signals` (exclusive, `PostUpdate` before `Render`, own message cursor); messages still sent |
| `HtmlTooltips { template, gap }`, `HtmlTooltip { element }` | Resource, Component | `data-tooltip="key"` (+ `data-tooltip-args` JSON, `data-tooltip-placement` right/left/above/below): while a pointer hovers the element or a descendant (nearest wins), an `HtmlUi` of `template` with `key`/`args`/`placement` is anchored beside it (the nearest ancestor that has a node — a link span carries the tooltip but no box); refreshed by updates, despawned on leave |
| `ElementSignal`, `ElementSignals`, `SignalBinding`, `SignalTrigger`, `SignalSource`, `ActivationInput` | Message, Component, enums | `data-on-<trigger>`/`data-with` hooks: buffered interaction signals (click = primary button or activation, auxclick = other buttons, press/release = any button, enter/leave — a pointer that vanishes (touch end: Bevy despawns it and drops it from the hover map) emits every entered element's leave —, submit = the form's `<button type="submit">` (or a `type`-less `<button>`) clicked or activated — it counts as bound for `click`, so a bound ancestor's click doesn't fire, and the activation is the submit's source — payload `{name: value}` of its named inputs); `source`: `Pointer { pointer, button, position, count }` / `Hover { pointer }` / `Activation(input)`; deepest bound element wins |
| `PseudoState { hovered, active, focused, focus_visible }` | Component | `:hover`/`:active` (from picking) and `:focus`/`:focus-visible` (from `InputFocus`/`InputFocusVisible`) per element; a change restyles in place; apps may set it |
| `Focusable { autofocus, tab_index }` | Component | on focusable elements (`data-on-click`, `<button>`, text `<input>` or `tabindex >= 0`; a negative `tabindex` opts out); `focus.rs` `sync_navigation` gives the ones in scope Bevy's `TabIndex` (`bevy_tab_index()`: 0 → `i32::MAX` so positive values come first, as in HTML; none under `display: none`) and each focus-taking root a `TabGroup` (modal on the active `HtmlModal` only) |
| `HtmlFocus` | SystemParam | `navigate(CompassOctant)` (shows focus; `FocusEdge` at an edge), `activate(ActivationInput)` (the input the app saw: `Key`, `GamepadButton`, `Synthetic` for harnesses, `Other`), `focused()` — the app binds its own input |
| `HtmlModal`, `HtmlNoFocus` | Component | on an `HtmlUi` root: confine focus to it while visible / never take focus |
| `FocusEdge { entity, direction }`, `ActivateElement { entity }` | EntityEvent | no neighbour in that direction / emit the element's click signals |

Re-exported crates (their types appear in the API): `tera`, `tl` (the
`astral-tl` fork, bug_0019 / UPSTREAM.md U11),
`fluent`, `lightningcss`. Cargo features: `system_fonts` (Bevy's
`system_font_discovery`), `text-input` (the editable-text widget),
`hot-reload` (the file watcher), `fuzzing` (the fuzz harness),
`dev-tools` (the examples' QA surface), and `hotpath` / `hotpath-alloc` /
`hotpath-prometheus` / `hotpath-cloud` (hotpath profiling: `#[hotpath::measure]`
on the pipeline systems and hot internals, `#[hotpath::main]` on every
example's `main`, no-ops without the feature; regression policy in
`hotpath/policy.toml`; `cargo run --example demo --features hotpath --
--no-render --exit-after 300` prints a report).

## Layout

```
README.md        human-facing overview: what bevy_markup is, how it works, testing strategy, Bevy
                 compatibility table (add a row on every release or Bevy bump; keep in sync)
LICENSE-MIT, LICENSE-APACHE  dual license (MIT OR Apache-2.0, Cargo.toml `license`)
src/
  lib.rs           crate docs (guide), BevyMarkupPlugin, HtmlUiSystems, prelude, re-exports
  html.rs          HtmlUi, TemplateContext, RenderedHtml, HtmlDebugOutline, HtmlElement,
                   HtmlUiBuilt, HtmlElements; render system (Tera + tl)
  template.rs      HtmlTemplate asset + loader (resolve_references: `{% include/extends "…" %}`
                   paths resolved relative to the file, rewritten, loaded with read_asset_bytes
                   (loader dependencies: Bevy's file watcher reloads every user of an edited
                   file; a manual `AssetServer::reload` reloads only its own path — bug_0026)
                   → one Tera set per template, components shared), each file converted by its
                   own root extension (a `.md` partial included from an `.html` page converts,
                   an `.html` partial from a `.md` page stays HTML), HtmlDocument (+ outline),
                   decode_text / decode_attribute (htmlize, ADR 0036)
  l10n.rs          LocaleBundle asset + loader (`.ftl.ron`: manifest → Fluent resources, read as
                   dependencies), ActiveLocale, LocalizedText; localize system (data-l10n-id/-args);
                   with `markdown: true`, each cacheable message's pattern pre-converts at load
                   into HTML with placeable markers and every frame substitutes the formatted
                   args into it (values escaped once by the args parser, substituted verbatim;
                   selects, adjacent placeables, term/message references and values whose
                   marker would land in a link destination convert per value) (ADR 0020)
  style.rs         Stylesheet asset + loader, DefaultStylesheet, HtmlStylesheet; CSS subset docs
  cascade.rs       (internal) stylesheet → declared style per element (+ unit tests)
  cascade/selector.rs (internal) selector allowlist + parcel_selectors matching (ADR 0034)
  fonts.rs         FontFamilies, FontFaces, GenericFamily
  build.rs         (internal) DOM + styles → Bevy UI children; HtmlUiBuilt trigger
  rebuild.rs       (internal) pure rebuild decision: Frame (load phases + change signals) → Build/Skip/Wait
  signals.rs       `data-on-*`/`data-with` → ElementSignal messages (picking observers, hover tracking)
  tooltips.rs      HtmlTooltips/HtmlTooltip: show_tooltips (Update; HoverMap → nearest `data-tooltip`
                   ancestor per hovered node → anchored tooltip HtmlUi (anchored to the nearest
                   ancestor that has a node — a link span carries no box), refreshed, despawned
                   on leave)
  anchor.rs        HtmlAnchor/AnchorPlacement: place_anchored (PostUpdate, after Build, before
                   UiSystems::Prepare; reads the previous frame's layout; clamped to the
                   viewport for every placement — Above/Left measured from the far edge,
                   clamped once the overlay's size is known)
  custom_elements.rs  `is="…"` + `data-*` dataset → app-defined systems run on spawn
                   (define_html_element, ElementConnected; CustomElements registry)
  focus.rs         Focusable/HtmlFocus/HtmlModal/HtmlNoFocus: focus scope sync, repair (id restore,
                   autofocus, else the first focusable in document order), despawn-safe focus
                   states (a signal handler that despawns the UI the same frame a focus state
                   changes can't panic — `try_insert`; `(remember_focus, update_focus_states)`
                   run after `dispatch_signals`, so a handler-driven focus move styles the same
                   frame), focus pseudo-state, press-to-focus, activation (InputFocus-based;
                   a submit button's activation submits its form)
  nine_slice.rs    NineSlice asset + loader, NineSliceFrame
examples/
                   One directory per example (`<name>/main.rs`, which cargo finds as the
                   example `<name>`), each with a README.md: a screenshot, what it shows,
                   controls, files. Screenshots: `examples/screenshots/<name>.png` — Git LFS
                   (`.gitattributes`), ≤ 1280 px wide, pngquant + oxipng; excluded from the
                   crate package. Retake them after visible changes.
                   Their assets stay under `examples/assets/<name>/` (one `AssetPlugin` root,
                   shared `ui/` frames). Every example works with mouse, keyboard and gamepad:
                   focusable controls (`data-on-click`, `tabindex`), a `:focus-visible` ring,
                   and each hotkey on a gamepad button too.
  shared/input.rs  (not an example; `#[path]`-included) ExampleInputPlugin: arrows / D-pad /
                   left stick → HtmlFocus::navigate with hold-to-repeat, Enter / A → activate;
                   left/right on a focused `data-setting` element → `SettingStep` message
                   instead; Tab / Shift+Tab → HtmlFocus::tab; `ArrowMode` resource
                   (`Directional` default; `Linear`: ← → step through Tab order via
                   `HtmlFocus::tab`, stopping at the ends — and ↑ ↓ are left to the app);
                   `Arrows` SystemParam (held axes from arrows / D-pad / left stick); `Hotkeys`
                   SystemParam (`just_pressed(key, gamepad_button)`)
  shared/harness.rs (not an example; `#[path]`-included; `dev-tools` feature)
                   HarnessPlugin + the examples' launcher (`harness::run`):
                   adds bevy_mcp_harness's localhost BRP + MCP
                   surfaces to the example (screenshots, `game/ui` with the
                   `data-on-click` `clickable` hook, input mocks); compiles
                   away without the feature. `run` picks the composition
                   from the command line: windowed (default), `--no-render`
                   (render-less headless: no winit/wgpu, `ScheduleRunner`
                   drives frames, `game/ui` + input mocks work, screenshots
                   error) and `--offscreen` (windowed rendering minus winit
                   into the harness's offscreen texture: real screenshots
                   without a window). Headless exists because a windowed
                   run stops ticking when its window is unfocused
                   (`ExampleWindow::before_plugins` runs on the empty `App` — what must
                   exist before `AssetPlugin`, e.g. the testbench's asset source)
  quickstart/      fonts, DefaultStylesheet, ActiveLocale, one HtmlUi, two `data-on-click`
                   buttons (add a coin, language) in a column, with an inline minimal `navigate` (arrows / D-pad, all four directions,
                   Enter / A; kept self-contained); Space / Y = language
  hot-reload/      asset file watcher (`--features hot-reload`): pressing H / B rewrites
                   the stylesheet with the next accent palette (the watcher reloads it and
                   the UI restyles in place); the palette *name* is a Fluent select over the
                   context's `palette` number; Language button, L / Y
  grid/            CSS grid: page track template switched via context (Layout button, Space / X);
                   tabs (`data-tab` → `signal.data`, Q / E, LB / RB) pick one of three screens
                   (`active_tab`), each a grid technique — Items: auto-fill slots with a 2×2
                   span and dense packing; Gear: a fixed 3×4 paper doll, slots placed by line
                   numbers (`.gear-<slot>`), the figure spanning two rows, empty cells; Quests:
                   three `minmax(0, 1fr)` columns of nested card grids under a main quest
                   spanning `1 / -1` —; every cell `tabindex="0"` so directional focus walks
                   each grid, the details panel follows the focused cell (`selected` = its
                   id: `slot-`/`gear-`/`quest-`); small grids in slots/stats; Language
                   button, L / Y
  grid-names/      named grid lines and areas: a camp board whose panels place by area
                   (`grid-area: portrait`; the cat panel spans two rows) and a supply
                   list placing between named column lines (`grid-column: left /
                   right`) — the areas imply the board's explicit tracks; clicking a
                   panel or a supply row checks it off in place (context flags dim it;
                   focus and hover survive), arrows / D-pad / Enter / A drive it;
                   Language button, Space / Y
  combinators/     descendant and child combinators: three group panels, one selected
                   at a time (click / Enter / A); `.selected p` highlights every
                   paragraph inside (the nested one goes mint) while `.selected > p`
                   hits only the direct child (blue, bold) and `.selected .nested`
                   lights the nested block's border — one class flip in the context,
                   re-rendered in place; Language button, Space / Y
  calc/            CSS calc() in layout: a sidebar (templated px) + a fluid content column
                   (`style="width: calc(100% - {{ sidebar }}px - 12px)"` — inline styles carry
                   the context values), cards dividing the column (`calc(100% / {{ cards }} -
                   8px)`), a headline indent `calc({{ indent }}px + 1em)`; three `data-setting`
                   steppers (− / + buttons, ← → on a focused value) update the context every
                   frame (`show_settings`) and the expressions re-resolve in place; the live
                   formulas are l10n'd text; L / Y language
  menu/            focus/navigation (shared/input.rs; autofocus, :focus-visible ring), a volume slider (focusable
                   `tabindex="0"` bar: ← → step it while focused; `is="slider"` observes picking
                   press/drag to set it at the pointer; ◀ ▶ mouse-only `tabindex="-1"` step
                   buttons; fill = templated `style` width), a difficulty stepper (◀ focusable
                   `tabindex="0"` label ▶, clamped at Easy/Hard; both controls carry
                   `data-setting`, which routes ← → to them), HtmlModal confirm dialog,
                   `data-tooltip` + HtmlTooltips (on the slider row: nearest ancestor wins),
                   buttons as a Tera 2 component (`../shared/components.html`: body, `class`,
                   `focusable`; included by menu and dialog), handlers routed with on_html_click
                   (dialog answer via `data-answer` → `signal.data`), a MessageReader beside
                   them (last input), roots styled by `<html class>` (backdrop, z-index,
                   pointer-events), `is="icon"`; L = language, Esc = cancel
  live/            a keyed party list rendered from data every frame and updated in place
                   (`id="unit-<name>"`, rows `tabindex="0"`: a focused row keeps focus while
                   members join above it), Recruit / Knock out / Language buttons (N / X, K / B —
                   the focused member else the last, fading via `opacity` —, L / Y), `style`
                   health bars, `is="badge"` spawn counter vs update counter
  world/           nameplates over walking 3D units with HtmlWorldAnchor (orbiting camera: plates
                   hide behind it/off screen, with a blinking unit, and go with a knocked-out
                   unit), distance fade from HtmlWorldAnchorView, `hidden` class toggle, HUD
                   counting on-screen plates; plates are styled HTML (faction card: 9-slice
                   `border-image` / rounded border / `outline`; monospace level badge, sans name,
                   italic title, three-color bar; inline `z-index` by distance) with Fluent lines
                   (markup, plurals, `<span class="{ $faction }">`); HUD buttons (root
                   `pointer-events: none`, button row `auto`): Hit nearest (K / B), Everyone
                   back (R / X), Plates (Space / Select), Language (L / Y)
  inline/          the 0.5.1 features in one journal page: inline
                   `background-color` (author) and `text-decoration`
                   (underlined day; clicking a quest toggles its
                   strikethrough), `<img>` seals in the text, an elliptical
                   `border-radius` quest panel, a `position: fixed` ribbon;
                   Language button, Space / Y
  markdown/        localized Markdown documents (ADR 0014): a `.md` shell
                   includes the pages, the tabs and buttons are raw HTML
                   inside the Markdown, the prose is Fluent values in
                   Markdown (`markdown: true`); Q / E + bumpers switch
                   pages, Language button, Space / Y
  inputs/          text input fields (ADR 0016, `--features text-input`):
                   name/title fields (first `autofocus`), the app reads the
                   fields' `EditableText` into the context so a preview
                   paragraph follows every keystroke, reset buttons write a
                   fresh `EditableText` (by id via `HtmlElements`); arrows
                   / D-pad move between fields, ← → move the caret while a
                   field holds focus (shared input.rs is input-aware);
                   Space / Y = language
  links/           `<a href>` links (ADR 0011): two inline in the intro
                   (from the Fluent value), three standalone; a click (or
                   activation) emits the `open` signal and the status line
                   re-renders with the href; link runs aren't `Focusable`
                   blocks (no box), so the app cycles them itself (← → / LB
                   RB over `HtmlElements::by_tag("a")` spans, `InputFocus`
                   + `:focus-visible` recolor, Enter / A activates); Space /
                   Y = language
  forms/           form submission: a `<form data-on-submit>` with two
                   fields and a `type="submit"` button (plus a
                   `type="button"` clear); the `Submit` payload goes into
                   the context and Tera `{% if submitted %}` swaps the form
                   for a receipt; "edit again" clears it; Space / Y =
                   language
  testbench/       main.rs + scenarios.rs: an interactive torture bench (scenario list,
                   size, churn, live frame/build numbers, pass/fail checks). Workloads are
                   generated into an in-memory asset source (`testbench://`, a `Dir` +
                   `MemoryAssetReader` registered through `ExampleWindow::before_plugins`,
                   i.e. before `AssetPlugin`) and spawned as nested `HtmlUi`s in the shell's
                   `is="stage"` element (an app node there would be reconciled away).
                   Groups: performance (big document, all dirty, restyle storm, indented
                   Markdown, localized rows, many UIs, calc(), selector list, text
                   features), security (nested rules, deep DOM, var()/grid/Markdown/
                   selector/sibling/entity bombs, hostile args, args-JSON injection,
                   remote sources), correctness (`TwinKind`: a fresh twin for reuse and
                   selectors, a forced full restyle under focus churn, a reversed-order
                   twin compared per row; plus `reuse` via `Skeleton::last_reuse`,
                   `quiet` (idle: no build/restyle/`Node` write), `linear` (N vs 4N,
                   best of three, ≤ 8×, real time); indented vs flat Markdown). Every
                   check was falsified against its bug (README "Methods").
                   `--windowed` (+ `--scroll`) windows the workloads and adds a `band`
                   check (the built rows cover the viewport; needs `--offscreen`
                   layout). `--scenario <id> --size <n> --no-churn` pin a run (`--size`
                   pins the size against `--exit-after`'s injected keys); checks log
                   `testbench: final …` on exit; frame times are `Time<Real>` (the
                   virtual clock clamps at 250 ms);
                   `#[hotpath::measure]` on its systems + the library's for profiling
  demo/            main.rs (setup: fonts, window), shell.rs (the full-screen shell HtmlUi:
                   `is="content-slot"` viewports filled once per slot, contexts),
                   controls.rs (language/theme selection; L / Y, T / X hotkeys), consts.rs (fonts);
                   options have ids (`lang-N`, `theme-N`), viewports are `tabindex="0"`;
                   `ArrowMode::Linear`: ← → step through panels and buttons, ↑ ↓ (tap = 60 px
                   step, hold = smooth; also right stick, PageUp/PageDown) scroll the nearest
                   `ScrollArea` at or above the focus — the viewport, or the middle column for
                   its buttons (`scroll_focused`, clamped to content), every theme's chrome has `.opt/.viewport:focus-visible`;
                   the middle column (`.column.side`, a row) holds `.side-scroll`, which is
                   `is="scroll-column"` (`ScrollArea`, wheel) with `overflow-y: scroll`;
                   styled scrollbars: `<div class="scrollbar" is="scrollbar"
                   data-target="<id>"><div class="thumb">` beside each scrolling area
                   (`.fill` panels are rows: viewport + track), themed in CSS (track/thumb
                   colors per theme, `.thumb:hover/:active/.dragging`); `update_scrollbars`
                   writes the shell context's `scrollbars[<id>]` (show, thumb top/height %,
                   dragging; rounded to 0.1%), which the template turns into `.hidden` and
                   the thumb's inline `style` — template values, not `Node` writes (see
                   Gotchas); `drag_thumb` / `press_track` observers scroll by pointer,
                   `DraggedThumb` (DragStart/DragEnd) keeps `.dragging` while the pointer is
                   off the thumb (a Bevy `ScrollbarThumb` must have no `Node`, so it couldn't
                   be themed by CSS); the selection handlers insert only
                   `lang_active`/`theme_active`, keeping the scrollbar state; a Scrollbars
                   On/Off toggle panel (`set-scrollbars`, `data-with` `{"on": …}` →
                   `Selection.scrollbars` → `scrollbars_on`; off adds `.hidden` to every
                   track, wheel/keys still scroll), and `scroll_focus_into_view`
                   triggers Bevy's `ScrollIntoView` on navigated focus (only while
                   `InputFocusVisible`: startup autofocus fires before layout settles)
tests/
  html_ui.rs       headless test vectors: HTML/CSS/Fluent/Tera → world dump, + browser_oracle (see Testing)
  signals.rs       picking-driven signal tests: real WindowEvent input → Bevy picking → ElementSignal/PseudoState
                   (needs TestUi::with_pointer; see Testing)
  signal_properties.rs  proptest: random pages (nested hooks, pointer-events none) × random pointer-op
                   sequences vs a reference signal model over the real picking stack (see Testing)
  properties.rs    proptest metamorphic properties over the pipeline (shorthand=longhands, round trips,
                   selector lists/formatting/duplicates, color notations, relative font sizes, 9-slice manifests)
  quickcheck.rs    quickcheck structured-input properties (cascade vs reference models — single rule winner and
                   selector lists × per-declaration importance × several elements — and text round trip)
  arbtest.rs       arbtest: arbitrary bytes through the pipeline (robustness) + structure-aware random documents
                   vs an HTML model (node tree, collapsed text, HtmlElements order)
  stateful.rs      proptest-stateful machine: random op sequences (default theme, own HtmlStylesheet set/remove,
                   locale, context, outline toggle, FontFamilies swap, pointer moves/presses/releases) vs a
                   reference model (dump + ElementSignals)
  layout_properties.rs  flexbox invariants on real headless layout (row-reverse mirror, auto margins,
                   grow shares, space-between, wrap without overlap)
  windowing_model.rs  proptest: random documents × text changes / row flips / scrolls, a windowed
                   build vs an unwindowed one stepped in lockstep, checked every frame (placement,
                   coverage, unique WindowKeys; tightness once settled) + minimized regressions
                   (bug_0160–0177)
  content_lint.rs  lint tests over the examples' content in examples/assets/: locale parity,
                   templates render (+ every data-l10n-id resolves), no hard-coded text, CSS url()s, pseudo-locale
  golden.rs        golden images (#[ignore]d): tests/golden/<scene>/ rendered offscreen by real Bevy,
                   compared with expected.png within a tolerance (see Testing)
  fuzz_corpus.rs   replays fuzz/seeds/* + local fuzz/corpus/* through bevy_markup::fuzz (#[ignore]d, --features fuzzing) for coverage
  common/mod.rs    shared headless harness: TestUi (temp asset root, settle, dump; with_layout = Bevy UI layout)
  vectors/<name>/  file-based vectors: page.html, style.css, browser.json (CSS oracle output);
                   Fluent vectors add messages.ftl + fluent.html (Fluent oracle output);
                   layout_* vectors also get rects in browser.json (layout oracle)
  golden/<scene>/  page.html, style.css, [messages.ftl], expected.png (text, frame, l10n)
  fixtures/        frame.png (32×24)
.github/workflows/
  ci.yml           per push to main / PR: -D warnings check/build/doc, cargo test, fuzzing-feature
                   tests, Fluent oracle references current
  nightly.yml      golden images (Ubuntu lavapipe), browser oracle vs current Chrome, 15 min
                   cargo-fuzz per target, other fuzz drivers build, criterion + divan
                   benchmarks (reports as artifacts, not a gate; fuzz on pinned
                   BEVY_MARKUP_NIGHTLY)
  mutants.yml      twice-weekly mutation testing, 4 shards, --in-place, fast settings, informational
  cache.yml        builds the shared `mutants`, `fuzz` and `bench` (both harnesses)
                   dependency caches once (push to main, daily); scheduled jobs only
                   restore them (see Testing, CI)
scripts/
  browser_oracle.py  headless Chromium → tests/vectors/*/browser.json (stdlib Python only)
  fluent_oracle.sh   @fluent/dom in jsdom → tests/vectors/*/fluent.html (Node + npm;
                     pinned packages in fluent-oracle/, node_modules gitignored)
  mutants.sh         mutation testing (cargo-mutants), fast settings + cache parking; see Testing TODO 6
  coverage.py        line coverage of src/ per testing layer and merged (cargo-llvm-cov);
                     the `benchmarks` layers cover what the criterion + divan benches
                     exercise (see Testing)
  golden.sh          golden-image test on Mesa lavapipe (`--update` rewrites references; fetches
                     Arch's vulkan-swrast into target/golden-lavapipe/ if no lavapipe ICD)
  fuzz-<driver>.sh   run one fuzzer over one target:
                     `scripts/fuzz-{libfuzzer,honggfuzz,fuzzcheck,test-fuzz}.sh
                     <html|css|ftl> [seconds=60]` (details in each script's
                     header; they encapsulate the per-driver workarounds
                     described under Testing TODO 5)
fuzz/
  fuzz_targets/      cargo-fuzz targets (`html`, `css`, `ftl`) over `bevy_markup::fuzz`
  seeds/<target>/    committed minimized seed corpora (read-only extra input to every fuzz run)
  corpus/, artifacts/, coverage/  gitignored (corpus/ = working corpus; CI carries it over nightly)
honggfuzz/
  targets/           the same three harnesses driven by honggfuzz (`cargo +nightly hfuzz run <t>`)
  vendor/honggfuzz/  vendored honggfuzz crate (patched bfd.c for current binutils)
fuzzcheck/
  tests/             the same three harnesses as fuzzcheck `#[test]`s
                     (`cargo +nightly fuzzcheck --test <name> fuzz_<name> --stop-after-duration 60`)
  vendor/            vendored fuzzcheck 0.13 git-master (edition 2024; patched
                     `__llvm_prf_data` parser for the u64 record layout of
                     LLVM 21+; see the comment in `llvm_coverage.rs`)
  .cargo/config.toml linker override: the repo's clang+mold cannot link the
                     LLVM coverage section symbols (`__start___llvm_prf_*`)
test-fuzz/
  src/lib.rs         the same three harnesses as AFLplus targets via
                     `#[test_fuzz]` (`cargo +nightly test-fuzz tests::fuzz_html`
                     from this directory; corpus seeds come from plain
                     `cargo test` runs)
examples/assets/   the examples' content (AssetPlugin file_path; no fonts: system fonts)
  src/             artwork sources (frame.kra) for the UI images
  calc/            page.html, style.css (the calc() widths + the steppers' chrome),
                   locales/{en-US,de}
  quickstart/      hello.html, style.css (html rule: border-image frame), locales/{en-US,de}
  grid/            grid.html, style.css (all of the grid example's layout), locales/{en-US,de}
  grid-names/      board.html, style.css (the areas, the named lines, the panel chrome),
                   locales/{en-US,de}
  combinators/     panel.html, style.css (the two depth rules), locales/{en-US,de}
  menu/            menu.html, dialog.html, tooltip.html, style.css
                   (every root's rule), locales/{en-US,de}
  live/            party.html, stats.html, style.css, locales/{en-US,de}
  world/           plate.html, hud.html, style.css, locales/{en-US,de}
  inline/          page.html, notes.md (Markdown template), style.css,
                   locales/{en-US,de} (`markdown: true` — journal + notes values)
  markdown/        reader.md (+ release.md, tutorial.md), style.css,
                   locales/{en-US,de} (`markdown: true` — the documents)
  inputs/          profile.html, style.css (input + focus ring), locales/{en-US,de}
  links/           report.html, style.css (a:hover/:focus-visible), locales/{en-US,de}
  forms/           signup.html, style.css, locales/{en-US,de}
  testbench/       shell.html, style.css, locales/{en-US,de} (`markdown: true`: shell.ftl +
                   workload.ftl, the generated workloads' messages)
  shared/          reused across examples: components.html (Tera 2 component
                   library: `ui.button`; unstyled — the including page's
                   stylesheet styles its classes), images: frame.png (256x256
                   9-slice frame), frame_transparent.png (clear center; the
                   parchment/terminal panels frame themselves with it),
                   cat.png (the examples' seal/cat icon)
  ui/themes/       demo CSS themes: crimson (default), parchment (framed `pre` via border-image longhands), terminal, large_print;
                   each carries the identical `Demo chrome` block styling the shell's panels/buttons/viewports
  ui/content/      test.html (plain), inventory.html (Tera), l10n.html (Tera + Fluent; ends
                   with a 60-entry "Expedition log" — one `log-entry` message selecting
                   place/event/coin plural from `range` loop args — so the inventory panel
                   overflows even on a 4K screen),
                   shell.html (the demo app itself: panels, buttons, slots), test.css
  locales/<id>/    demo bundles (main.ftl.ron + ui.ftl) for en-US, ru, de, ja
```

## Conventions

- **CHANGELOG.md** ([Keep a Changelog](https://keepachangelog.com/en/1.1.0/)):
  every user-visible change (public API, CSS/HTML/Fluent support, behavior,
  user-facing fixes) gets a line under `## [Unreleased]` in the same change,
  under Added / Changed / Deprecated / Removed / Fixed / Security, written for
  users (what they can do or must change; not internal refactors, tests or
  agent docs). Mark breaking changes. A release renames `[Unreleased]` to
  `[x.y.z] - YYYY-MM-DD`, adds a fresh empty `[Unreleased]`, and updates the
  compare links at the bottom.

- **Formatting:** run `cargo fmt` (the whole workspace) before
  committing. Formatting every file is fine, including files a change
  didn't touch and `tests/common/mod.rs`: don't restore by hand what the
  formatter changed.

- Library = HTML/CSS/Fluent/Tera → Bevy UI only. Panels, layouts, selectors,
  scrollbars, cameras, `ClearColor`, font files and asset paths belong to apps
  (the examples). Global config is resources; per-entity config is components.
- New user-facing items go in the prelude; internals stay `pub(crate)`. Keep
  rustdoc warning-free and doc examples compiling (`no_run`).
- Asset-backed state reacts to `AssetEvent::LoadedWithDependencies` /
  `Modified` plus change detection, so hot reload works with Bevy's
  `file_watcher` (not enabled in the examples).
- When to update is decided by the pure `rebuild` module (`RebuildState`,
  a component every `HtmlUi` requires): `build_html_ui` only gathers a
  `Frame` per entity (own/default stylesheet load phase, document ready,
  change signals) and acts on the `Decision` (`Build`/`Restyle`/`Skip`/
  `Wait`). Change update triggers there, and unit-test them in
  `rebuild::tests`, not via full apps. A stylesheet that *fails* to load
  emits no asset event: a change that arrives while a needed sheet loads is
  kept pending and updates the frame the sheet resolves (ready or failed) —
  that's what turns a failure into an (unstyled) update, and why a failed
  `HtmlStylesheet` falls back to `DefaultStylesheet` without a separate
  latch. Re-requesting a failed asset can flip its state to `Loading` for a
  frame, so never read change signals only after resolving load states.
- Updates (`build.rs`): items become a `NodeSpec` tree (plain data: `Node`,
  element, background, frame, text spans), reconciled with the existing
  children by `update_children` for content updates and restyles alike. A
  content change first pairs the fresh DOM against the previous build's
  (`Skeleton`, ADR 0021): subtrees whose tag, attributes and text are
  unchanged reuse the previous build's spec tree by refcount
  (`Arc<NodeSpec>`) and the diff skips them entirely (their entities
  already match; the one queued write refreshes the `DomNode` handle,
  remapped up front while the `Arc`s are exclusively held), skipping the
  cascade, the run construction and all copying for them. Reuse is
  content-only — except for scoped restyles (ADR 0029): a pure
  `PseudoState` change pairs too, and reuses everything outside the
  flipped elements' paths to the root; a re-collected element's children
  re-cascade when its fresh inherited style differs from the cached
  spec's (`NodeSpec::style`), or when they are themselves
  pseudo-reactive (`NodeSpec::pseudo_reactive`: some pseudo-referencing
  selector's non-pseudo parts match the element or its content —
  `.row:hover .label` restyles the label when the row flips, hovered
  itself or not). Other style-side changes (`var()` values, font,
  image or stylesheet changes) still rebuild fully
  (`RebuildState::style_pending` — set on
  any style signal, surviving a `Wait` until an update applies it — and
  `full_style` in the pass gate), translated elements never reuse, and an
  all-dirty workload turns the pairing off (retried every 16th build;
  restyle passes preserve the strike counters — their reuse rate says
  nothing about content-pairing utility). Every tag pair that matches
  (tag, attributes) is paired even when its subtree changed — the
  inherited-style gate reads the changed container's cached spec;
  `clean` alone gates reuse (bug_0150: pairing only clean nodes made
  every child of a changed container re-collect). Sibling lists align
  by position when their counts match, else by their common head and
  tail (`align_siblings`, identical source from each end) and the middle
  by position (ADR 0033, bug_0168); a container whose count changed is
  paired but not clean, and an `li` moved to another ordinal leaves the
  clean set (its marker number changed). Per sibling list, an element whose `id` is unique among the old *and* new
  siblings is matched by it, the rest by position among themselves (a
  duplicated `id` keys nothing — keying it looped: the second copy was
  respawned on every restyle its `PseudoState` triggered). A match with the
  same identity (`same_identity`: text block or not, tag, `id`, `is` +
  dataset via `ConnectedAs`) is updated in place by `update_spec` (every
  component re-inserted or removed: `Node`, element, `DomNode`, `Focusable`,
  `ElementSignals`, background, frame, CSS-owned; spans updated, or respawned
  when their count changes); others are despawned/spawned and the list is put
  in document order (`replace_children`) with app-nested `HtmlUi` roots kept
  last. `HtmlUiRestyled` fires only for a restyle that spawned nothing — a
  span respawn counts (new span entities: the kept-everything promise no
  longer holds, so `HtmlUiBuilt` fires and its observers re-wire the
  replacements, link spans in particular). Pointer signal observers are
  attached once per
  element (`PointerSignalObservers`) and read the element's current
  `ElementSignals` at event time, so updated bindings need no new observers.
  UIs are processed deepest first, so a nested UI's queued commands come
  before an ancestor's despawn of its slot (bug_0016).
  `content_update_matches_a_fresh_build` and `restyle_matches_a_fresh_build`
  (properties) guard that the update paths agree with a fresh spawn.
  Custom elements (`is`): `spawn_spec` collects an
  `ElementConnected` per spawned `is` element; the build queues
  `custom_elements::connect` for each (document order) after the spawn
  commands and before the `HtmlUiBuilt` trigger, so `HtmlUiBuilt` observers
  see what definitions attached; kept elements never re-run them.
  An undefined name warns once per name. User content nested deeper than
  `MAX_DOM_DEPTH` (100) is dropped with a one-time warning (the walks
  are iterative; such subtrees also never pair for reuse). Shape compares
  only what the spec owns: a frame is the
  `CssFrame` marker spawned with its `ImageNode` (an app `ImageNode` is app
  state, bug_0018), nested `HtmlUi` children are skipped (bug_0017).
- Inline styles and opacity (`cascade.rs` `InlineStyle`/`HtmlStyles::get_with`,
  `build.rs` `Style::opacity`/`BoxStyle::fade`/`faded`): the `style`
  attribute is parsed per element (lightningcss `StyleAttribute`) and applied
  after normal rules / after `!important` rules for its `!important` part
  (uncached). `opacity` multiplies down the tree in the computed `Style`
  (the `<html>` element reuses the root style, or its opacity would square)
  and fades text/span colors at spec time and box colors in `collect_node`.
- fluent-syntax comes from a fork, `nchashch/fluent-rs` branch
  `fix/fuzzing-bugs-0.11` (upstream's 0.11.1 tag plus fixes; Cargo.lock pins
  the commit), wired via `[patch.crates-io]` in the root and all four fuzz
  manifests. It fixes two bugs reachable from any FTL asset:
  - bug_0005: an invalid `\U` escape before a multi-byte character panicked
    (the error quoted the escape plus one *byte* of the next character).
  - bug_0015: expression nesting recursed without a limit (placeables *and*
    call arguments), so a few KB of `{{{…}}}` or `F(F(…))` overflowed the
    stack; `get_inline_expression` now fails past `MAX_NESTING_DEPTH` = 100
    with `ErrorKind::NestingTooDeep` (the entry becomes Junk).
  The same fixes are proposed upstream from branch `fix/fuzzing-bugs` (on
  upstream `main`, 0.12). `[patch]` doesn't reach crates that depend on
  bevy_markup: they need the same entry (README). Re-check when bumping
  `fluent`; drop the patch once upstream releases the fixes in the
  fluent-syntax version `fluent` uses (UPSTREAM.md U1/U2).
- `vendor/honggfuzz` is a patched fork of the honggfuzz crate used only by
  `honggfuzz/` targets: its bundled C source fails against current binutils
  (`bfd.h` no longer defines `TRUE`).
- Demo typography: system fonts (Bevy `system_font_discovery`, a
  dev-dependency feature) — headers `sans-serif` (red), body `serif`
  (off-white), code/debug `monospace`. CSS themes use the generic keywords;
  the examples map them to `FontSource::{SansSerif, Serif, Monospace}`
  families (`FontFaces::new(FontSource::Serif)` etc.,
  `examples/demo/consts.rs`).

## Pipeline

```
.html/.md (Tera) --render(TemplateContext)--> HTML --tl--> DOM       [Render]
    --data-l10n-id × ActiveLocale--> LocalizedText                     [Localize]
    --CSS (HtmlStylesheet | DefaultStylesheet) × FontFamilies--> UI    [Build] → HtmlUiBuilt
```

- Templates: compiled at load (syntax errors fail the load); name = asset path,
  so `.html` gets Tera HTML autoescaping (the loader forces `.md`/`.markdown`
  into the autoescape list). Plain HTML renders to itself; Markdown
  (`src/md.rs::to_html`/`to_html_template`, pulldown-cmark: CommonMark + tables, strikethrough,
  task lists, footnotes; raw HTML passes through unclassed; every element
  the conversion constructs carries `class="markdown"` — `img.markdown`
  styles a document's Markdown images apart from raw-HTML `<img>` tags)
  converts once per file, at template load (ADR 0020): each file in the
  include graph decides by its own root extension — an `.html` partial
  included from a `.md` page is never converted, a `.md` partial included
  from an `.html` page is. Tera syntax survives verbatim (`to_html_template`:
  Tera runs pass raw, a Tera-only paragraph keeps no `<p>` wrapper; text
  inside code fences and code spans stays Tera-literal escaped text —
  `{% … %}` there doesn't execute), and `separate_statements` puts a
  `{% … %}`/`{# … #}` on its own line only when it already owns the line
  (an inline statement and `{{ … }}` stay where they are) so statements
  can't merge with Markdown content. Before that, `dedent_blocks` (ADR
  0024) strips from each own-line Tera block body (`if`/`for`/`block`/
  `component`/`filter`/`raw`/block `set`, per `elif`/`else` segment) the
  indentation its lines share, recursively up to `MAX_DEDENT_DEPTH` (16):
  authors indent Tera for readability, and the separated statement's blank
  line made a 4-space body an indented code block (review_0004 H11).
  Own-line raw HTML containers (`<div …>` … `</div>`, any non-void tag
  alone on its line) are blocks too, nested with Tera blocks on one stack
  (ADR 0025); `<pre>`/`<textarea>`/`<script>`/`<style>` bodies stay
  verbatim. Lines inside code are content; `raw` bodies dedent but aren't
  searched. *What is code* is pulldown's answer,
  never a hand-written rule (`md::code_ranges` over the conversion's
  options): `separate_statements` skips statements in the source's code,
  and `resolve_references` skips includes in the code of the text the
  conversion parses (`template_code_ranges`: after separation, mapped back
  — its blank lines can make an indented line a code block). Hand-written
  fence/indent/backtick rules disagreed with CommonMark in both directions
  (review_0003 G13/G17, review_0004 H04).
  Fluent bundles set `markdown: true` in the manifest to pre-convert each
  cacheable message's pattern at bundle load; localization substitutes the
  formatted arguments into the cached HTML (the values were escaped once by
  the args parser and are substituted verbatim; messages with selectors or
  adjacent placeables, term/message references, values whose marker would
  land in a link destination, or a pattern that fails to anchor the
  substitution, convert per value as before).
- Update triggers: rebuild on template/context change or reload when the
  rendered HTML differs from the previous render (`render_templates` compares
  `HtmlDocument::source`; an identical render leaves `RenderedHtml`
  untouched, a repeated identical error is logged once), locale
  change or bundle (re)load, outline marker added or removed; restyle on
  stylesheet swap or (re)load (a failed sheet updates unstyled once, when it
  resolves), a stylesheet's `border-image` image loading (a `url()` behind a
  `var()` loads too — the sheet's `--` declaration carries the URL tokens;
  an inline `style="border-image: url(…)"` does NOT load — the URL has no
  base to resolve against, a documented limit),
  a context `var()` change (the sheet's tracked `var()` names, computed at
  load, plus the inline `style` attributes' names the last build
  collected — unknown properties' refs (`pointer-events`,
  `outline-offset`) count too), `FontFamilies`
  change. The UI isn't built while its stylesheet is loading (images may
  arrive later; their load restyles).
- Structure (`build.rs`: DOM → `Item` tree → nodes): blocks `h1`–`h6`, `p`,
  `li` (bulleted), `pre` (whitespace kept, no wrap; leading newline and
  trailing whitespace dropped), loose text; containers (`CONTAINERS`: `div`,
  `section`, `article`, `header`, `footer`, `main`, `nav`, `aside`, `ul`,
  `ol`, `blockquote`, `figure`, `form`) become column nodes holding their
  children, `row_gap` = the `HtmlUi` node's own unless CSS `gap`; other
  elements are inline in a block or walked through outside one (`html`,
  `body` included — the `HtmlUi` node is the root); `head`/`script`/`style`
  skipped. Each block is a `Text` + one `TextSpan` per styled run; whitespace
  collapses outside `pre`.
- Localization (fluent-dom convention): `<p data-l10n-id="key"
  data-l10n-args='{"n": 3}'>fallback</p>` on any element (block, container,
  inline, walked-through); the translation replaces the children, the
  element's own content is the fallback. Args are JSON (numbers stay numbers
  for plurals), printed by the crate's `json` Tera filter from a map literal
  (`'{{ {"name": player} | json }}'`) or one map variable
  (`'{{ args | json }}'`). Never splice a value into hand-written JSON
  (`'{"name": "{{ player }}"}'`): attribute decoding undoes Tera's `&quot;`,
  so a value holding `"` adds its own arguments (review_0003 G20; the
  examples and README use map literals). Translations are markup
  (`walk_translation` in `build.rs` parses them as a fragment): inline
  elements in them are styled; `data-l10n-name` elements take the
  same-named, same-tag source descendant's tag/id/classes (each usable once,
  otherwise plain text). In `.ftl`: `&lt;`/`&amp;` for literal `<`/`&`,
  `{"{"}`/`{"}"}` for braces. Deliberate differences from fluent-dom: no
  sanitizing (nested markup, `class`/`id`, any element kept), string args
  HTML-escaped, fluent-rs number formatting (no grouping). CJK paragraphs go
  on one line (a wrapped line becomes a stray space).
- CSS subset (`style.rs` docs): selectors as lightningcss parses them,
  matched by its engine (parcel_selectors, ADR 0034) within an allowlist
  (`cascade/selector.rs`): types, classes, ids, comma lists, descendant/child
  combinators, `data-*` attribute selectors, `+`/`~` next to the subject,
  `:is()`/`:where()`/`:not()` and the structural pseudo-classes per the
  Selectors limit below; anything else skips its rule at `debug`. Spec
  specificity; the per-element style cache folds the ancestor chain into its
  key. Plus
  `:hover`/`:active`/`:focus`/`:focus-visible`,
  matched per element (`HtmlElement`
  tag/id/classes + `PseudoState`, cached per combination) with CSS
  precedence: `!important`, then specificity (ids,
  classes, type), then source order. An unsupported value drops only its
  declaration — an earlier rule's value stands (track lists with one
  unsupported track, unsupported `display`/alignment/flex/sizing values, an
  invalid side of a `border-width`/`padding` shorthand, `%`/`calc()` gap, a
  `fit-content()` with a non-px/% limit, var() failures).
  `var()` resolves the UI's
  `TemplateContext` first (a `--name` key; its values are opaque — their own
  `var()`s don't resolve), then the sheet's own `--name:`
  declarations (sheet-level, last wins; substituted transitively, memoized
  with cycle detection and a 64 KiB budget per declaration — an unresolvable,
  cyclic or over-budget reference drops just that declaration), then the
  reference's fallback; reference depth is capped (`MAX_VAR_DEPTH` = 64)
  and so is the whole substitution's recursion (`MAX_SUBSTITUTE_DEPTH` =
  256, counting into fallback and function-argument expansion too) —
  without both caps a chain of fallback-bearing `var()`s can double its
  output at every level and abort the process (bug_0092);
  CSS text (sheet sources, `style=` attributes, substituted values) nesting
  blocks deeper than `MAX_CSS_NESTING` (32) or parentheses deeper than
  `MAX_CSS_PAREN_NESTING` (8) is rejected before lightningcss parses it:
  its parser recurses without bound and parses nested `light-dark(` /
  `calc(var(…))` in exponential time (`nesting_over_limit`; bug_0092,
  bug_0106, bug_0179; UPSTREAM.md U14, U15);
  `transition` (the shorthand) animates a restyled color's change
  (`color`/`background-color`/`border-color`), driven by an
  `advance_transitions` system after `Build`.
  Inherited `color`, `font-family` (the
  whole list chains into Bevy 0.20's per-glyph fallback list over registered
  families, generics via `set_generic`; nothing registered → Bevy's
  default font, as browsers do), `font-size` (`px`, `em`/`%`, `rem`, keywords
  with medium = 16px, `smaller`/`larger`), `font-weight` (bold above 500, CSS
  font matching), `font-style`, `pointer-events` (`none`/`auto`; arrives as
  an unknown property — lightningcss has no typed one); blocks:
  `background-color`. `html` is the
  starting point even without `<html>`; nothing declared → white, Bevy's
  default font, 16px. Missing font faces fall back as CSS font matching does
  (style before weight): bold-italic → italic → bold → regular.
- Layout properties (`LayoutDecl` in `cascade.rs`, applied by
  `LayoutDecl::apply_to` in `build.rs` over bevy_markup's defaults): `display`
  (`none`/`block`/`flex`/`grid`), `flex-*`, `justify-content`, `align-*`,
  `justify-items`/`-self`, sizes (px/%/vw…/auto), margins (px/%/auto, over
  `li`'s 12px indent), `gap` both axes, `box-sizing` — on containers and on a
  block's outer node (the text node, or its box wrapper). Containers stay flex
  columns unless `flex-direction`/`display: block`/`display: grid` says
  otherwise; blocks and containers keep `flex_shrink: 0`; nodes default to
  `BoxSizing::ContentBox`. Grid (onto Bevy's `Node` grid fields, laid out
  by taffy): track lists (`grid-template-*`, `grid-template`, `grid`;
  lengths/%/viewport units, `fr`, `auto`, `min-`/`max-content`, `minmax()`,
  `fit-content()`, `repeat()` incl. `auto-fill`/`auto-fit`; one unsupported
  track drops the whole declaration — an earlier rule's tracks stand; a
  `repeat()` count and the computed track/area totals are capped
  (`MAX_GRID_TRACKS` = 1000, saturating arithmetic) against overflow on
  pathological counts like `repeat(40000, [a] 1fr)` — bug_0081, bug_0121),
  `grid-auto-rows`/`-columns`/`-flow`, and line/`span` placement
  (`grid-row`/`-column` + longhands, `grid-area`; `GridLineDecl` pairs →
  `GridPlacement`) by number or by name: named lines (the `[name]`s in the
  parent's track lists, positive and negative spellings) and
  `grid-template-areas` regions resolve at collection against the parent
  grid's `GridNames` table; areas imply the explicit tracks they span (sized
  `auto`). A name the parent doesn't have — or one inside an
  `auto-fill`/`auto-fit` repeat (its count is layout-dependent) or an
  occurrence past the template — places `auto`; `span` anchored at a named
  line is unsupported. `grid-template`/`grid` reset the name table, a later
  nameless track list clears that axis's names. Bare
  `grid-auto-flow: dense` fails lightningcss's parser (UPSTREAM.md U10).
  `position` (`CssPosition`: static/relative/absolute/fixed) + insets
  (`top`/`right`/`bottom`/`left`/`inset`), and `border-radius` corners
  (circular or the CSS `h / v` elliptical syntax), are
  `LayoutDecl` too: `static`/undeclared leaves `PositionType::Relative` with
  insets at `auto` (CSS ignores insets there); Bevy places `absolute` in the
  parent's padding box (every parent acts positioned); `fixed` adds the
  `FixedNode` component (CSS-owned: taken back when the declaration goes).
- Element components (`build.rs` `apply_css_owned`): `border-color` →
  `BorderColor`, `z-index` → `ZIndex`, computed `pointer-events: none` →
  `Pickable::IGNORE` (also on a boxed block's inner text node, and on the
  block's `TextSpan`s: bevy_picking resolves text hits against the span
  entity, so an ignored block's spans must ignore too, bug_0020). An app may
  set these itself, so `CssOwned` records what CSS set and a restyle without
  the declaration resets only that (`BorderColor`/`ZIndex` are `Node`'s
  required components: reset to default, never removed).
- Box properties (`cascade.rs` → `build.rs`): `border-image` (shorthand +
  `-source`/`-slice`/`-repeat`), `border-width`, `padding` (absolute lengths)
  on blocks, containers and the root rule; `background-color` and
  `gap`/`row-gap` on containers and the root rule, `background-color` also on
  blocks.
  `url()` resolves relative to the `.css`; the `Stylesheet` loader loads the
  images as dependencies — never from a network source: `template::is_remote`
  (`http`/`https`) skips such a border-image and fails an include, a Fluent
  resource or a 9-slice image (review_0003 G19). Maps to Bevy's sliced `ImageNode`
  (`VisualBox::BorderBox`) + `Node::border`/`padding`. Slice numbers = image px,
  `%` = of image size (needs the image loaded). `stretch` → Stretch, other
  repeats → `Tile`; one mode for all sides. Bevy always draws the center;
  `border-image-width`/`-outset` ignored (corners at image size). Blocks with
  box properties get a wrapper node (a node can't be both `Text` and
  `ImageNode`); the `HtmlElement` is on the wrapper.
- Root rule (`build.rs` `apply_root`, queued per build/restyle): the `HtmlUi`
  entity is styled as the document's top-level `<html>` element (`root_element`:
  its `id`/`class`; a bare `html` for fragments, outline mode and failures) —
  box, `gap`, the whole `LayoutDecl` on its `Node`, and `background-color`,
  `border-color`, `z-index` (`ZIndex`: Bevy sorts roots by `(GlobalZIndex,
  ZIndex)`), `pointer-events: none` (`Pickable::IGNORE`), `border-image`
  (`ImageNode`) as components. `CssRoot` keeps the app's `Node` (`base`) and
  what was declared; each application first restores the previously declared
  fields from `base` (`LayoutDecl::restore_from`, `restore_sides`), takes the
  result as the new `base`, then applies the new declarations — so undeclared
  fields the app changes (per-frame positioning) are never touched. Components
  go through `claim`: the app's value is saved the first time CSS sets one and
  given back (or removed) when CSS stops. Nested containers' default `gap` is
  the root's effective `row-gap`.
- Demo locales: every locale needs the same message ids; item names arrive as
  English data (`$item`) and non-English bundles map them with an `item-name`
  message. Every visible string in `l10n.html` and the demo shell has a key.

## Gotchas (verified)

- **`RemSize` is global — HtmlUis with different root font-sizes fight.**
  The build syncs Bevy's `RemSize` resource to each UI's `html` rule
  font-size (CSS `rem` semantics); no restyle is queued — Bevy's layout
  re-resolves every `Val::Rem` when the resource changes, and the calc pass
  reads the new size on its next frame.
  Two `HtmlUi`s whose root font-sizes differ would flip `RemSize` on every
  rebuild of the other. Keep root font-sizes equal across UIs, or use
  explicit px/em.


- **Markdown in FTL values: list/heading markers must be on indented
  continuation lines.** FTL reads a `-` at column 0 as an attribute key, a
  `#` as a comment, and a line-initial `*` (even `**bold**`) as a variant
  marker — everything from it to the message's end becomes Junk. So
  `markdown: true` values write their lists as two-space-indented
  continuations (`notes-body = The *bridge* is out.\n\n  - first item`),
  never start a line with `*`, and escape literal braces as `{"{"}` /
  `{"}"}` (a `{{ Tera }}` placeholder shown in the doc is exactly that).
  The blank line makes the newline a Markdown paragraph break. Fluent
  string args are never Markdown or markup: HTML-escaped, and on the
  per-value path (a message the cache can't take) their punctuation and
  line breaks ride through the conversion as Private Use Area stand-ins
  (`l10n.rs` `inert_markdown`; review_0004 H03 — `[x](url)` in a player
  name became a real link). An identifier-shaped value (`two-handed`)
  is left as is, so a select still matches it.

- **App-written `Node` fields don't survive on elements, only on the root.**
  An in-place restyle or update writes an element's whole `Node` from its
  CSS spec (`build.rs` `update_spec`), so a field the app set (`top`,
  `height`, `display`, …) is reset whenever that element's UI restyles —
  including on a `:hover`/`:active` change. Writing per-frame geometry on an
  element that has a `:hover` rule loops: hover → restyle resets the field
  (e.g. height 0) → un-hover → the app writes it back → hover … (the demo's
  first scrollbar thumbs flickered "behind" their track and lost hits). Only
  the root keeps app values for fields CSS doesn't declare (`CssRoot`). For
  per-frame visuals on elements, use template values (`style="…"`, classes).
- **The root node gets an automatic minimum size in Taffy 0.9 (Bevy
  0.20).** CSS says `min-width`/`min-height: auto` compute to zero for a
  root (the automatic minimum is a flex-*item* rule); taffy 0.9 applies the
  content minimum to root nodes too, so a root with content taller than the
  window grows to it instead of clipping — the scrolling recipe (below)
  stopped working until `apply_root` zeroed the undeclared minimums.
- **A scroll panel needs every flex level on its path allowed to shrink.**
  Containers default to `flex-shrink: 0` (browsers: 1), so the browser recipe
  `flex-grow: 1; min-height: 0; overflow-y: scroll` alone does nothing: the
  panel keeps its content height, nothing overflows it (`ScrollArea` has zero
  range) and the root clips the overflow instead. Give the panel *and each
  ancestor between it and the sized root* `flex-shrink: 1; flex-basis: 0;
  min-height: 0` (the demo themes' "Demo chrome" block; bug_0028).

- Bevy `ImageNode` defaults to `VisualBox::ContentBox` (draws inside padding);
  frames need `BorderBox` (`NineSliceFrame` sets it).
- Scrollable panels (demo `shell.rs`): a `.viewport` element is `overflow-y:
  scroll` in CSS (an app write to `Node.overflow` would be undone by the next
  update, which re-inserts the spec's `Node`) and gets `ScrollArea` (wheel
  input) once per element from its `is="content-slot"` definition; its CSS
  also gives it `flex-grow: 1` and `min-height: 0` — else its content sizes
  it and nothing scrolls. Viewport children need `flex_shrink: 0.0` (`HtmlUi` blocks set it).
  For hand-built scrollbars, the `Scrollbar` must be a *sibling* of the
  viewport. A framed node that scrolls itself needs `overflow_clip_margin:
  OverflowClipMargin::content_box()`.
- Every UI `Node` is pickable and blocks pointer input below it. Invisible
  layout-only wrappers must not catch the pointer: `pointer-events: none` in
  CSS (or `Pickable::IGNORE` on app nodes) — a full-window wrapper once broke
  all scrolling and clicking.
- Nested `HtmlUi`s: they survive an ancestor's updates while the element
  holding them is kept (`update_children` leaves app-nested roots alone,
  bug_0017) and go with it when it's replaced — spawn them into slots on
  `HtmlUiBuilt` (the demo's `wire_shell_build`). UIs update deepest first, so
  a nested UI's own commands are queued before an ancestor despawns its slot
  (bug_0016).
- Bevy 0.20: `BorderRadius` corners are `CornerRadius { x, y }` (2D, for
  elliptical corners; `From<Val>` sets `x`, leaves `y` auto); still a `Node`
  field, not a component.
- `tl::VDom` borrows its input; `HtmlDocument` uses `tl::parse_owned` (unsafe
  fn, sound per its docs) → `VDomGuard`, which only hands out shared borrows,
  so the DOM is never mutated; translations live beside it keyed by
  `tl::NodeHandle`.
- `tl` misparses raw Tera (`{% if a < b %}` becomes a `<b>` tag) — hence
  render-then-parse. `tl` keeps character references as written (decode
  with `decode_text` for text, `decode_attribute` for attribute values:
  the spec's two rules, via `htmlize`) and drops `<!DOCTYPE>`.
- Tera 2 (not 1.x): unknown functions/filters fail at template *compile* time,
  so custom ones must be registered in the loader before `add_raw_template`.
  `template::new_tera` builds every environment (the loader, the fuzz
  harness): autoescaping plus the `json` filter. Without the filter Tera
  prints maps with Rust `Debug` string escaping (`\u{200d}`, `\0`), which
  isn't JSON (bug_0128: an emoji name lost its translation), so argument
  attributes (`data-l10n-args`, `data-with`, `data-tooltip-args`) are
  printed with `| json` and parsed as plain JSON
  (`template::parse_attribute_json`).
- Fluent wraps placeables in U+2068/U+2069; the bundle is shared behind an
  `Arc`, so `l10n.rs` strips them. Fluent term arguments only accept literals;
  message references share the caller's variables.
- rustdoc: a bare `[`template`]` link is ambiguous (Bevy has a `template` fn);
  write `[`template`](mod@template)`.
- Iosevka ligatures render `<!--`/`-->` as arrows in outline text.
- CSS generic keywords are case-insensitive: unquoted `font-family: Serif` is
  the generic `serif`, not a family named "Serif" (quote it, or map the
  generic with `FontFamilies::set_generic`).
- Headless apps: `ImagePlugin` only pre-registers its loader; `bevy_render`
  registers the real one. Without rendering, register
  `ImageLoader::new(CompressedImageFormats::empty())` yourself.
- Bevy's `Query::iter_descendants` is breadth-first; use `iter_descendants_depth_first` for document order (`HtmlElements`).
- HTML white-space collapsing covers ASCII whitespace only (space, `\t`, `\n`, `\f`, `\r`); `char::is_whitespace` would also eat NBSP/U+3000.
- Performance guards assert *scaling*, not wall time, where they can:
  best of three runs at N and 4N, the larger at most 8× the smaller
  (`template::scanners_stay_linear_on_pathological_input`). A fixed
  bound passed a quadratic scanner at 1.25 s of 2 s and flakes under
  load (review_0004 H09, review_0003 G37). The remaining wall-clock
  guards and their measured headroom are listed under H09 in review_0004;
  convert one to a scaling test when you touch it.
- Tests: `TestUi::settle()` demands a new build; for a change that legitimately rebuilds nothing (e.g. a `DefaultStylesheet` swap under a ready own `HtmlStylesheet`) use `settle_quiet()`.
- Tests: under `TestUi::with_layout`/`with_pointer` (picking on),
  `update_pseudo_states` rewrites every element's `PseudoState` from the
  hover map each frame, so a hand-inserted hover lasts one frame. Insert
  states by hand only under `TestUi::new`, or hover with the pointer and
  assert the state held (bug_0148: a flip test passed on a hover that
  never held).
- API changes that touch `src/fuzz.rs`'s imports (`Styler`, `cascade`) need
  `cargo check --lib --features fuzzing`: the module only compiles under the
  feature, and plain `cargo test`/`check` don't see it (CI runs the
  fuzzing-feature tests).
- CLDR plural operand `n` is the absolute value: English `-1` selects `one`.
- Offscreen rendering (`tests/golden.rs`): pipelines compile asynchronously and
  a draw whose pipeline isn't ready is silently skipped; wait until the render
  world's `PipelineCache::waiting_pipelines()` is empty (disable
  `PipelinedRenderingPlugin` to inspect it inside `app.update()`). Without
  `App::run`, poll `plugins_state()`, then `finish()` and `cleanup()`.
- wgpu's GL backend isn't usable for software rendering (Bevy 0.20 doesn't
  enable wgpu `gles`; llvmpipe GL can't compile some of Bevy's GLSL): use
  lavapipe (Vulkan).
- `TestUi::settle` never returns for a stylesheet whose `url()` image is
  missing (the sheet loads, its dependency fails); check sheets first (as
  `content_lint` does).
- A pseudo-locale opening marker must not be a text `[` at the start of a
  line (FTL reads a variant key); use a string-literal placeable `{"["}`.
- **Windowed UIs (ADR 0031, revised by 0032)**: only materialized elements have entities.
  `WindowKey`s exist only on windowed UIs' entities (an item is keyed when
  collected with a window; `update_spec` keeps keys in step; a keyless
  cached spec never reuses in a windowed build). A root that isn't laid
  out (zero viewport) windows nothing — the testbench's `--no-render`
  builds everything; use `--offscreen` to see windowing.
  One unit system: logical pixels of the root's **content box** — the
  model's heights (only `ComputedNode`'s measured size is physical;
  margins are already logical), the width anchor
  (`logical_content_width`), and the visible region (`visible_region`:
  the padding box, starting `padding-top` above the content). Comparing
  the physical border box with the logical content width cleared the
  model and rebuilt every frame on any padded/bordered root or HiDPI
  display (bug_0153). The measurement writes into the nearest `HtmlUi`'s
  model (not the parent's: nested content never culled, bug_0152) and
  flags `WindowModel::changed`, which triggers one rebuild (bug_0154).
  The layout harness adds only `ScrollAreaPlugin` (the whole widget
  group's `TextInputPlugin` duplicates the crate's under `text-input`).
  The model's heights are measured per frame from the newly added
  `WindowKey` entities and those whose layout changed
  (`Changed<ComputedNode>`; `measure_window_heights`, `PostLayout`, after
  `Build` — which inserts a missing `WindowModel` the same frame); a
  culled subtree keeps its sticky height. Coalesced spacers carry no
  `WindowKey` (their height = a derived sum; keying them would
  overwrite the run's first member and drift the cursor). A coalesced
  run's height includes the gaps between its members (`Item::Spacer`
  carries its column's gap): one node gets one gap in a column, so
  summing heights alone dropped k − 1 gaps and the band rose above the
  viewport (bug_0151). Test windowing by position against an unwindowed
  build of the same document at the same offset (`review_0006`'s
  `band_errors`), never by element counts. The reuse
  check runs before the cull and requires the cached spec's spacerness
  to agree with the current decision; a coalesced spacer never reuses
  (its run shifts). Translated subtrees are never culled (their
  fragments are windowless); whitespace-only raw roots advance the
  cursor by zero without invalidating it (they produce no item); any
  other unknown-height child invalidates it (culled = nothing below
  culls). Scroll builds pair with the skeleton (an unchanged document;
  the model keeps its keys — remapping would drop anonymous blocks'
  heights) and keep its strike state; a cached subtree reuses only when
  it holds no spacer and lies wholly inside the band (it carries the
  culling of the band it was built under; review_0006 J04). A windowed UI
  rebuilds when the offset leaves the slack the last build recorded
  (`WindowModel::slack`: how far it may move before any element crosses a
  band edge — strictly more flips a culled element, `EDGE` early for a
  materialized one), the viewport's width or height changes, or a height
  does (J05).
  The band follows the offset the root *shows*: `ScrollPosition` clamped
  to the last layout's scroll range (`visible_scroll`; Bevy clamps only
  for its layout, bug_0164). A cull is refused for a subtree that isn't
  clean in this build's pairing — its stored height is stale (bug_0161) —
  and the refusal flags the band stale for one rebuild (bug_0162). A
  windowed UI always pairs (ADR 0021's adaptive gate skips it: the
  pairing also remaps the height model, bug_0167); a remap that drops
  heights, and `WindowModel::clear`, set `remeasure` (every keyed entity
  is measured once, bug_0163/bug_0166). The cached skip refreshes a
  reused root's `WindowKey` with its `DomNode` (bug_0169). `CursorFrame`
  isn't `Clone`: its cells are shared state (bug_0165). `positioned`
  marks only grow. An unmeasured element (a new one) advances the
  cursor by its measured siblings' mean height (`WindowCtx::estimate`,
  ADR 0033); a frame with no measured child, or a `positioned` child,
  still invalidates. Known cost, in `HtmlWindowed`'s docs: rows above
  the band that change height (or an insertion whose height differs
  from the estimate) shift the view for one frame past the margin (bug_0170; also row flips inside containers and a width
  change's first frame). Walked-through elements (`html`, `body`) stay in
  the walk — no root unwrap (bug_0172: it lost `body`'s style for every
  UI); `column_height` advances the cursor by their children, itemless
  nodes (`is_itemless`: ASCII-blank text, comments, `head`) by zero
  (bug_0175). The measurement clears the model when the content width
  moves (bug_0173), reads `%` margins' basis from a plain `ComputedNode`
  query (the root has no `ChildOf`, bug_0174) and measures
  `unrounded_size` (bug_0176). Whether a frame can be tracked is one
  predicate, `children_are_opaque`, on the `Node` a container's declared
  layout produces (anything but a non-wrapping, start-packed column or
  block; reversed directions are opaque) and on the root's `Node` (a row
  root windows nothing) — bug_0177; there are no measured opaque marks. `tests/windowing_model.rs` is the property that
  found these; run it with `PROPTEST_CASES=400 --release` after windowing
  changes.
- Bevy/taffy size the border box by default; bevy_markup nodes set
  `BoxSizing::ContentBox` (CSS initial); `box-sizing` overrides.
- `cargo mutants` copies the source tree and ignores nested `.gitignore`s:
  park the fuzz drivers' `target/`/corpus dirs first and point `TMPDIR` at
  a disk with room (each job builds its own target dir).
- Headless UI layout (no window, no renderer, verified in `tests/common`):
  `UiPlugin` sizes roots from their camera's `computed.target_info`, which
  bevy_render's `camera_system` would fill — set it by hand on a camera with
  `RenderTarget::None { size }` + `IsDefaultUiCamera`. `UiPlugin`'s picking
  and focus systems then need `InputPlugin`, a `WindowPlugin` with
  `primary_window: None`, `DefaultPickingPlugins` and `TextureAtlasPlugin`;
  text measurement needs `TextPlugin`. Missing-resource panics name the
  system only with Bevy's `debug` feature (`--features bevy/debug`).
- Bevy's text measure (`TextMeasure::measure`) ceils every text node to whole
  pixels; browsers keep 1/64 px. With a fractional line height (16px × 1.2 =
  19.2) each text block runs up to 1px taller than in a browser and the
  drift accumulates down the page.

## Testing

How-to guide for writing and running tests and hunting bugs across the
whole harness: `docs/agents/skills/testing.md`. This section is the
reference and history. Bugs found here (or by the fuzzers/property
harnesses below) are filed in `docs/agents/bugs/` — see **Bug reports**.

- **Cascade unit tests** (`src/cascade.rs`, `cargo test --lib`): CSS text →
  declared style for an `HtmlElement` (specificity, compound matching, comma
  lists, importance, unsupported selectors).
- **Test vectors** (`tests/html_ui.rs`, `cargo test --test html_ui`): input
  files (HTML/Tera template, CSS, Fluent bundles) → the Bevy world they
  produce. `TestUi::new(name, files)` writes the files plus `frame.png` into a
  fresh temp asset root and builds `MinimalPlugins + AssetPlugin + ImagePlugin
  + BevyMarkupPlugin` (no window/renderer; fonts are fake `Handle::Uuid`s labelled
  `serif`, `serif-bold`, …, `mono`). `.stylesheet()`, `.locale()`,
  `.spawn(template, context, node)`, then `settle()` updates until all tracked
  assets are loaded and a new build has been stable for 5 frames.
  `assert_dump(expected)` compares a text dump of the `HtmlUi` subtree: one
  line per entity (`tag#id.class` / `-` / `html-ui`, then `border=`,
  `padding=`, `margin=` as t,r,b,l, `gap=`, `bg=`, `slice=file t,r,b,l
  stretch|tile`), one indented line per text run (`"text" face size color`).
  Vectors: Tera structure, CSS cascade + fonts, Fluent, box model, runtime
  changes (context / locale / stylesheet swaps, root box restore). Plain-HTML
  vectors live as files in `tests/vectors/<name>/` (`TestUi::from_vector`).
- Adding a vector: write the inputs, derive the expected dump **by hand from
  HTML/CSS/Fluent semantics** (don't paste actual output), run. When a vector
  fails, decide whether the code or the expectation is wrong before editing
  either. Keep the dump format stable; extend it only for newly mapped
  properties. A vector should fail when its feature is broken (spot-check by
  breaking the code once).
- **Browser oracle** (`browser_oracle` test): every `tests/vectors/*/` with a
  `browser.json` is built and compared with Chromium's computed styles — per
  non-whitespace character as rendered (`text-transform` applied in context, read through a selection per text node; face from CSS font matching over the harness's
  registered families, size, color) and per root/block/container (padding,
  border widths, border-image file/slices/repeat with `%` resolved on the
  32×24 fixture, background, container gap). Regenerate after changing a
  vector's inputs: `scripts/browser_oracle.py [tests/vectors/<name>]`, then
  review the `browser.json` diff (one record per line) and commit it. The page
  is `* { all: unset }` + `BEVY_MARKUP_CSS` (bevy_markup's defaults and layout model as CSS:
  flex-column root and containers, block blocks, Bevy's default font at line
  height 1.2, white text, `li` indent and bullet, `pre` 8px padding — keep in
  sync with `build.rs`) + `style.css` + `page.html`, so vectors must be plain
  HTML (no Tera / `data-l10n-id`; the script refuses them and skips Fluent
  vectors), set `color` on
  `html` (browsers default to black, bevy_markup to white) and `border-style: solid`
  where widths matter. Deliberate differences (root background,
  `border-style`, `li` bullets, the browser's synthesized `tbody`) are
  skipped and listed above `FIXTURE_SIZE` in `tests/html_ui.rs`; add new
  ones there with a reason. The
  oracle's first run found two real bugs (unregistered `font-family` kept the
  inherited family; weight 501–599 wasn't bold).
- **Layout** (`TestUi::with_layout` / `from_layout_vector`, `layout_dump()`):
  Bevy UI really lays out headlessly against a fixed-size camera (see
  Gotchas). Text uses Bevy's embedded default font (FiraMono, 0.6em advance,
  printable ASCII only); no fake families. `layout_column_stacking` pins
  `layout_blocks` with hand-derived rects; the **layout oracle**
  (`layout_oracle`) compares every root/block/container border box of each
  `tests/vectors/layout_*/` with Chromium's `getBoundingClientRect` (recorded
  in `browser.json` by `scripts/browser_oracle.py`: 640px viewport — headless
  Chromium widens narrower windows — and the same FiraMono file, found via
  `cargo metadata`) within 1px per value. Layout vectors: no
  `font-family`, ASCII only (the script refuses others), font sizes whose
  1.2 line height is whole (Bevy ceils text nodes, see Gotchas). Vectors:
  `layout_blocks` (padding, borders, gap, wrapping, `pre`), `layout_mixed`
  (mixed inline content in a container, mixed font sizes on one line,
  `border-image` wrapper block, root border, nested containers). Both agree
  with Chromium to the pixel.
- **Unit tests** (`cargo test --lib`; `--features fuzzing` adds `src/fuzz.rs`):
  besides the cascade tests, `template.rs` (entity decoding: five escapes,
  one level only, others untouched; Tera-autoescape round trip proptest),
  `l10n.rs` (escape/decode identity, `data-l10n-args` typing: numbers stay
  numbers for CLDR plurals, bools → strings, bad JSON → error naming the
  message; only FSI/PDI stripped), `fonts.rs` (face fallback chain
  exhaustively, `resolve` list order / case / generics), `nine_slice.rs`
  (manifest defaults and errors), `html.rs` (`HtmlElements` document order),
  `fuzz.rs` (style cache transparency proptest).
- **Layout properties** (`tests/layout_properties.rs`): flexbox invariants over
  generated sizes on real headless layout, 1px tolerance: `row-reverse` mirrors
  `row`, `margin: auto` centers, zero-basis `flex-grow` splits by factor,
  `space-between` spreads evenly, wrapped items stay inside without overlap.
- **Signals** (`tests/signals.rs`, `cargo test --test signals`): interaction
  end to end over the real picking stack. `TestUi::with_pointer` extends the
  layout harness with a primary `Window` entity and aims the camera at it
  (`RenderTarget::Window(WindowRef::Primary)`; the camera's `computed.target_info`
  is still set by hand, so layout is unchanged). The tests then write
  `WindowEvent::CursorMoved`/`MouseButtonInput` messages — what winit sends —
  one action per frame (`move_pointer`/`press_pointer`/`release_pointer`/
  `click_at`; a click spans three frames, like a real one), and Bevy's full
  chain runs: `PointerInputPlugin` → UI picking backend → `Pointer<Click>`-style
  entity events (bubbling through text spans) → `HoverMap` → bevy_markup's
  observers, `hover_signals` and `update_pseudo_states`. Assertions read the
  drained `ElementSignal`s (`take_signals`) and `PseudoState`s, plus element
  rects (`node_rect`) for click positions. Vectors: click/press/release with
  payload and position, deepest-bound-element-wins (nesting), enter/leave over
  a subtree, the despawn-leave after a rebuild under a stationary cursor,
  hover/active pseudo states, `pointer-events: none` (bug_0020), inline hooks
  staying dead.
  Gotchas learned here: Bevy's message updates are gated on fixed ticks
  (`signal_message_update_system` runs in `FixedPostUpdate`), and at ~1 ms
  test frames those are rare — per-cycle message iteration
  (`iter_current_update_messages`) re-serves old signals, so the harness
  drains both buffers (`Messages::drain`). Cursor-based `MessageReader`s
  (the real picking path) are unaffected. Pressing a focusable element
  focuses it (press-to-focus), so `PseudoState.focused` is set after a click
  even where the test only cares about hover/active.
- **Signal properties** (`tests/signal_properties.rs`, also in the
  coverage script's `signals` layer): random pages of bound containers
  (nested up to two levels, random trigger subsets, random
  `pointer-events: none` subtrees) × random pointer-op sequences
  (move/press/release, 1–12 ops), compared op by op with a reference model
  of the real semantics: deepest non-ignored node owns the hit and the
  deepest chain element bound for a trigger emits it; enter/leave track the
  chain's covered set; presses accumulate per pointer (bevy_picking's
  `pressing` map is never pruned by moves, and a second press elsewhere
  doesn't drop the first), a release clicks when the hovered node was ever
  pressed and then clears the press state, and always runs the hovered
  chain's release hooks. Pages are text-free so hit chains are
  pure rect containment (the text-span path is pinned by `tests/signals.rs`).
  Both model halves were spot-checked by breaking the library (disabling
  `covered_by_deeper`, cutting the hover ancestor walk): the property fails
  under each and passes restored.
- **Content lint** (`tests/content_lint.rs`): checks the examples' content
  in `examples/assets/` (committed, so it runs everywhere, CI included).
  `PAGES` lists every template with
  the examples' context, stylesheets, bundle family and an optional
  `unlocalized` reason (`every_template_is_listed` keeps it complete). Checks:
  locale message/attribute parity with en-US; templates render for every
  context × stylesheet and every `data-l10n-id` resolves in every locale; no
  visible text outside `data-l10n-id` that keeps letters once context strings
  are removed; CSS parses and `url()`s name files; a generated `en-XA`
  pseudo-locale leaves no untranslated text run. New template → add to `PAGES`.
- **Golden images** (`tests/golden.rs`, `scripts/golden.sh`; `#[ignore]`d,
  so plain `cargo test` never needs a GPU): scenes in `tests/golden/<scene>/`
  (`text`: sizes, colors, wrapping, inline runs, background, `pre`; `frame`:
  root/container/block `border-image` stretch/round/% slices; `l10n`: Fluent
  markup, plurals, fallback) are rendered by full Bevy without a window
  (DefaultPlugins minus winit/audio/gilrs/pipelined rendering) into a
  `RenderTarget::Image`, captured with `Screenshot::image` and compared with
  `expected.png`. Committed inputs only (default font, `frame.png`). The
  script pins Mesa lavapipe via `VK_DRIVER_FILES` (an installed
  `lvp_icd*.json`, else Arch's `vulkan-swrast` matching `pacman -Q mesa`,
  fetched once into `target/golden-lavapipe/`, no root): bit-identical run to
  run. Tolerance: a pixel differs above channel delta 8; a scene fails above
  0.05% differing pixels (NVIDIA ≤5, RADV ≤1 delta vs lavapipe; one changed
  letter is ~0.1%). `scripts/golden.sh --update` rewrites references;
  mismatches write `target/tmp/golden/<scene>.{actual,diff}.png`.
- **Fluent oracle** (`fluent_oracle` test): every `tests/vectors/*/` with a
  `fluent.html` must build the same dump localized by bevy_markup (`page.html` +
  `messages.ftl` as the en-US bundle, via `TestUi::from_vector`) as
  unlocalized from `fluent.html` — the DOM Fluent's reference bindings
  (`@fluent/dom` 0.10.2 + `@fluent/bundle` 0.19.1 in jsdom, `useIsolating:
  false` because bevy_markup strips the isolation marks) produce from the same
  inputs, `data-l10n-*` attributes removed. Failures print a line diff
  (`-` fluent-dom, `+` bevy_markup). Regenerate after changing a vector:
  `scripts/fluent_oracle.sh [tests/vectors/<name>]`; fluent-dom's warnings
  (missing messages, sanitizer and name decisions) are recorded as comments
  at the top of `fluent.html`. Vectors: `fluent_basics` (plurals,
  selectors, terms, markup values, entities, missing-message fallback, `li`,
  `pre`), `fluent_overlays` (`data-l10n-name` edge cases, translated
  containers and inline elements). Deliberate differences stay out of
  oracle vectors and are pinned by the hand-written `fluent_permissive_markup`
  and `fluent_localization` vectors. Its first run found two missing
  features, now implemented: `data-l10n-name` overlays, and `data-l10n-id` on
  containers/inline elements (translations were silently dropped).

### Testing TODO

Work through in order. Weakness numbers refer to the current methodology's
known gaps:

1. Expectations come from the same head as the code (shared misreadings of
   CSS/Fluent pass).
2. The dump is a projection: unprinted components (`flex_shrink`,
   `TextLayout::no_wrap`, `Overflow`, entity counts, …) are unchecked.
3. Components, not layout or pixels: positions, wrapping and drawing are
   untested.
4. Fake font handles: font loading, glyph fallback, CJK untested.
5. `settle()` polls with a sleep and a frame cap (possible flakiness).
6. Coverage by example only.

- [x] **1. Real browser as the oracle** (fixes weakness 1): CSS via
  `scripts/browser_oracle.py` + `browser_oracle` (vectors `cascade`,
  `box_model`, `units`); Fluent via `scripts/fluent_oracle.sh` +
  `fluent_oracle` (vectors `fluent_basics`, `fluent_overlays`). See Testing.
  - [ ] More oracle vectors as CSS support grows (every new property gets one).
- [x] **2. Metamorphic and property-based tests** (fixes weakness 6). Check
  relationships that must always hold, over generated inputs (`proptest`,
  `quickcheck`, `arbtest`, `test-strategy` are dev-dependencies; pipeline
  properties live in `tests/properties.rs`, written with `#[proptest]` /
  `#[strategy]` over the shared `tests/common` harness):
  - [x] Shorthand = longhands: `padding: 1px 2px` builds the same world as the
    four longhands; likewise `border-image`. (`border-width` has no longhands.)
  - [x] Unmatched rules don't matter: adding a rule that matches nothing changes
    nothing (even `!important`).
  - [x] Order only breaks ties: swapping a class rule and a type rule never
    changes the result; equal-specificity ties stay covered by unit tests.
  - [x] Round trips: locale A → B → A, theme X → Y → X, context v → w → v end in
    a dump identical to the start; a forced rebuild with no input change is
    idempotent (proxy for "no duplicate children").
  - [x] Stateful sequences (`tests/stateful.rs`, `proptest_stateful`): random
    op chains (theme × {framed, plain, broken}, locale, context, outline
    toggle, `FontFamilies` swap, pointer moves/presses/releases over a bound
    button — via `TestUi::with_pointer`, real `WindowEvent` input) against
    one long-lived `HtmlUi`, model-checked after every op: dump *and*
    `ElementSignal`s (`SignalLog`, a durable `Last`-schedule reader, since
    buffered messages expire across a multi-frame `settle()`). The pointer
    model covers rebuilds and restyles under a stationary cursor: content
    changes replace every element, so a hovered button leaves (from the
    enter snapshot) and its replacement enters; theme swaps shift the root
    box and move the button out from under the pointer; the unstyled button
    is 0×0 and unhittable.
    Found two real bugs the pairwise tests missed: outline *removal* never
    rebuilt (fixed via `RemovedComponents`), and a globally latched failed
    sheet swallowed a later re-select (since replaced by the pending-change
    rule in `src/rebuild.rs`).
  - [x] `quickcheck` over structured inputs: a generated many-rule stylesheet
    checked against a reference CSS-precedence model (`cascade_winner_matches_
    precedence_model`), and arbitrary text round-tripping through Tera
    autoescaping (`context_text_round_trips_through_template`). The cascade
    property catches a specificity mutation that all cascade unit tests miss.
    Note: quickcheck 1.1's `Gen` RNG is private (edition-2024 `gen` keyword) —
    build `Arbitrary` impls from `T::arbitrary(g)` + `g.choose`;
    the `#[quickcheck]` attribute comes from `quickcheck_macros`.
  - [x] More metamorphic/model properties: selector list ≡ expanded rules ≡
    commented/duplicated sheet; color notations (`#rgb`/hex/`rgb()`/named);
    em/%/rem bases; 9-slice manifest sides; quickcheck selector lists with
    per-declaration `!important` over several elements; stateful ops for
    per-entity `HtmlStylesheet` set/remove and `FontFamilies` swaps; random
    pages × pointer-op sequences vs a signal reference model
    (`tests/signal_properties.rs`, over the real picking stack). The stateful
    ops found three rebuild bugs (override removal never rebuilt; a
    failed override ignored default swaps; a re-requested failed sheet's
    one-frame `Loading` ate the swap signal).
  - [ ] Shrink quality: stateful ops shrink only by removal; proptest value
    shrinking inside an op isn't supported by the framework.
- [x] **3. Headless layout checks** (fixes weakness 3, mostly): Bevy UI
  layout runs headlessly (`TestUi::with_layout`); `layout_column_stacking`
  (hand-derived) and `layout_oracle` (Chromium rects) over `layout_*`
  vectors. See Testing.
  - [x] Flex layout, sizes, margins, `box-sizing`: vectors `layout_flex`,
    `layout_sizes` (match Chromium to the pixel); `BEVY_MARKUP_CSS` models bevy_markup's
    `flex-shrink: 0`; `tests/layout_properties.rs` checks flexbox invariants.
    The oracle found Bevy's border-box default (bevy_markup now uses content-box).
  - [x] CSS grid: vector `layout_grid` (fixed/%/`fr`/content-sized tracks,
    spans, explicit and swapped line placement, implicit rows, `auto-fill`
    with `minmax()`, column dense flow, `grid-area`, item alignment, container
    and boxed-block items) matches Chromium to the pixel; the mapping is
    pinned per value by `cascade::tests::grid_*`.
  - [x] Focus: `focus_navigation_scope_and_styles` (focusability incl.
    `tabindex`, `autofocus`, `:focus`/`:focus-visible` + `outline` restyles,
    activation, restore by `id` across a rebuild, modal confinement,
    `HtmlNoFocus`; restore and modal spot-checked by ablation) and
    `html_focus_navigates_by_layout_and_reports_edges` (layout harness; it
    propagates `InheritedVisibility` itself, which Bevy's navigator needs);
    `html_focus_tabs_in_html_order` (Tab order with positive/zero/negative
    `tabindex`, `<button>`, `display: none`, wrapping, modal confinement,
    removal on rebuild, `<button autofocus>`).
  - [x] Positioning: vector `layout_position` (`absolute` by `top`/`left` and
    by `right: %`/`bottom`, `relative` offset, `static` ignoring insets, plus
    `z-index`/`border-radius`/`border-color` that must not move anything)
    matches Chromium to the pixel (spot-checked by dropping the `top`
    mapping); component mapping and the CSS-owned reset are pinned by
    `positioning_radius_border_color_z_index_and_pointer_events`
    (`tests/html_ui.rs`, dump keys `pos=` `inset=` `radius=` `bcolor=` `z=`
    `pick=none`) and `cascade::tests::{position_and_insets,
    radius_border_color_z_index_and_pointer_events}`.
- [x] **4. Lint tests over real content** (`tests/content_lint.rs`, see
  Testing): locale parity, templates render with the examples' data and every
  `data-l10n-id` resolves, no hard-coded text, CSS `url()`s, pseudo-locale run.
  No content bugs found; each check spot-checked by breaking the content.
  - [ ] Overflow: lay out the pseudo-locale (`TestUi::with_layout`) against a
    size budget per panel.
- [x] **5. Fuzzing** (robustness). Four drivers over the same
  `#[doc(hidden)]` `bevy_markup::fuzz` harness (feature `fuzzing`), which calls the
  internal glue directly — a full Bevy app is far too slow per exec.
  Contract for every target: no panic/hang/abort; errors are values.
  Every input also runs through the Markdown path (`md::to_html_template`
  on the same bytes, then the converted template renders); the `css` target
  cascades over elements with ancestor chains and pseudo states (the
  `A B`/`A > B` paths and the match cache — a second round re-runs it), and
  the `ftl` target drives the `markdown: true` cache (`markdown_cache` +
  `substitute`).
  Easiest entry point: `scripts/fuzz-<driver>.sh <html|css|ftl> [seconds]`.
  - `cargo fuzz` (`fuzz/`, libFuzzer/ASan, nightly):
    `cargo +nightly fuzz run <target> -- -max_total_time=60`.
  - `honggfuzz-rs` (`honggfuzz/`, hardware-counter feedback; also nightly):
    `cargo +nightly hfuzz run <target>` — needs
    `HFUZZ_BUILD_ARGS='--manifest-path honggfuzz/Cargo.toml --config target.x86_64-unknown-linux-gnu.linker="cc"'`
    (the repo's `.cargo/config.toml` forces clang+mold, which fails to link
    the hfuzz runtime) and a time bound (`timeout 120`, or
    `HFUZZ_RUN_ARGS="--run_time 60"`; args after the target name reach the
    *target's* argv, not the driver). Data lands in `hfuzz_workspace/`
    (gitignored).
  - `fuzzcheck` (`fuzzcheck/`, its own coverage sensor over `-C
    instrument-coverage`; also nightly, installed via
    `cargo +nightly install cargo-fuzzcheck`):
    `cd fuzzcheck && cargo +nightly fuzzcheck --test <name> fuzz_<name>
    --stop-after-duration 60`. The vendored copy (git master, edition 2024)
    needed: a patched `__llvm_prf_data` parser (LLVM 21+ widened the record
    fields to u64 — NumCounters sits at offset 0x38 of the 72-byte record;
    verified against a `-C instrument-coverage` section dump) and the
    counter-file filter relaxed (cargo passes absolute source paths).
    `fuzzcheck/.cargo/config.toml` overrides the repo's clang+mold linker,
    which cannot resolve `__start___llvm_prf_*` section symbols.
  - `test-fuzz` (`test-fuzz/`, AFLplus via `cargo-afl`; nightly):
    `cargo +nightly test-fuzz tests::fuzz_html` (from `test-fuzz/`). Corpus
    seeds are collected by ordinary `cargo test` runs (the `#[test_fuzz]`
    macro writes arguments to the corpus); AFL needs
    `AFL_I_DONT_CARE_ABOUT_MISSING_CRASHES=1 AFL_SKIP_CPUFREQ=1` on this
    machine (the core-pattern and cpu checks require `sudo`, which we don't
    have). Install with `cargo install cargo-test-fuzz cargo-afl` (both
    nightly). Data lands in `test-fuzz/target/` (gitignored).
  - [x] cargo-fuzz's first `css` run found a real crash: a selector with a
    non-ASCII first character panicked in `Compound::parse` (`&rest[1..]`
    byte-sliced past a multi-byte char). Fixed to skip the rule (bevy_markup idents
    are ASCII-only by design); regression test
    `multibyte_selector_characters_are_skipped`.
  - [x] honggfuzz's first `ftl` run found an *upstream* crash: fluent-syntax
    0.11.1 slices parser source at byte ranges that can land inside a
    multi-byte character (e.g. FTL `u={"\" + U-escape + replacement char`).
    Reachable from any FTL asset; not fixed upstream and the version is
    pinned by `fluent` 0.16 (fluent-bundle 0.15). Fixed in the fluent-syntax
    fork (first vendored as `vendor/fluent-syntax`; see Gotchas); regression
    tests in `src/fuzz.rs`.
  - [x] The arbtest harness (`tests/arbtest.rs`) covers the pipeline end to
    end with arbitrary bytes; its first run found the failed-stylesheet hang
    (fixed in `build.rs`, see Gotchas).
  - [x] Structure-aware generation, HTML: `generated_documents_build_the_html_model`
    (arbtest) builds random well-formed documents and compares the built
    tree with an HTML model. Found `HtmlElements` iterating breadth-first
    (fixed: depth-first), and Unicode-whitespace collapsing of NBSP/U+3000
    (fixed: ASCII whitespace only). CSS/FTL grammars remain open.
- [ ] **6. Mutation testing** (measures vector strength): `scripts/mutants.sh`
  (default: unit + html_ui + stateful, like CI; `--full` adds the property
  suites; `--file src/x.rs` narrows). Results in `target/mutants.out/` (not
  committed, overwritten per run — copy survivors here), reruns skip caught
  mutants (`--iterate`). The script parks the fuzz caches (cargo-mutants
  copies the tree, ignoring nested `.gitignore`s) and restores them on exit.
  Speed (measured): a mutant costs ~4–8 s build + ~0.5 s tests (was 45 s +
  9 s locally, 194 s + 39 s on CI), after a one-time ~150 s dependency build
  per job — a full run is minutes, not hours. Three settings, each measured:
  - Only the tests that run get built: target flags go in `--cargo-arg`.
    `--cargo-test-arg` reaches only the test run; cargo-mutants' build step
    (`cargo test --no-run`) then links every test binary and example (11
    Bevy links per mutant; 28 s → 3.6 s).
  - The `mutants` cargo profile (`Cargo.toml`): no debug info (it dominates
    Bevy link times) and dependencies at `opt-level = 3` (tests ~40%
    faster; Bevy's recommended dev setting, scoped to mutation runs).
  - Bevy linked dynamically (`--cargo-arg=--features=bevy/dynamic_linking`,
    the Bevy guide's biggest fast-compile win): each relink is ~1 s.
  With these, the build is 0.8 s per mutant in a warm tree. The Bevy guide's
  nightly options don't help (measured, same settings, rebuild per mutant):
  stable 1.03 s; nightly 2.03 s; + `-Zshare-generics` 2.08 s; + Cranelift
  for bevy_markup (LLVM for deps) 1.97 s; both 2.03 s. Share-generics only trims
  the one-time build (150 s → 128 s). `-j`/`--minimum-test-timeout` are set by
  the script: use `BEVY_MARKUP_MUTANTS_JOBS` / `CARGO_MUTANTS_MINIMUM_TEST_TIMEOUT`.
  No mold on CI (measured, not worth it): rustc ≥ 1.90 already links
  through its bundled `rust-lld` on x86_64 Linux (`-fuse-ld=lld` + its own
  `gcc-ld` dir), so `rui314/setup-mold` — which swaps `/usr/bin/ld` — never
  takes effect, and forcing mold via `RUSTFLAGS` measured the same as lld
  without debug info (4.6 s vs 4.2 s rebuild of all targets, 68 s vs 65 s
  cold). The main CI job is ~3–4 min with a warm cache (build 37 s, tests
  ~38 s).
  - [ ] Complete a `--full` run locally, or read the CI results (twice weekly)
    (`mutants.yml`: unit + html_ui + stateful, 4 shards; survivors in each
    shard's job summary and the `mutants-shard-N` artifacts). Line numbers
    below are from `e1b910c`.
  - [x] First optimized CI run (2026-10-04, run 37236608548, 4 shards of
    105 mutants: 28–49 min each). Per mutant: build median 4.7–8.0 s
    (was 194 s), tests 0.3–0.7 s (was 39 s) — all mutants of a shard take
    9–19 min. The rest is the one-time baseline build (18–30 min): a
    cache miss, because rust-cache's key includes an environment hash and
    the `env:` block had changed. Since then `cache.yml` builds the
    `mutants` cache once per dependency change and restores it daily
    (beating the 7-day eviction); the shards only restore it (see the CI
    notes below). Survivors: the 6 documented equivalents plus `l10n.rs`
    `localize` `delete !` (turning localization off left stale
    translations; the state machine only caught it by chance): now killed by
    `turning_localization_off_restores_own_content`.
  - [x] Triage the first CI run (2026-10-04, run 37223937948: 420 mutants;
    278 caught, 51 missed, 46 unviable across 8 shards of 25–44 min). All 51
    rerun against the full suite (+ properties, quickcheck,
    layout_properties): 9 were already caught there. New tests kill 36 more:
    - `cascade.rs` property mapping (border sides, flex-flow/-shrink/flex,
      align-content, max-height, margin sides, `display: flex`, `auto`/
      `none` sizes, generic families, weight 500/501 boundary,
      `border-image-source` longhand and `none`): table-driven unit tests
      (`layout_properties_map_to_bevy_values`, `border_sides_…`,
      `font_weight_boundary_is_above_500`,
      `generic_family_keywords_map_one_to_one`,
      `border_image_sources_none_and_longhands`).
    - `same_shape` (nested shape change, frame removed, merged runs):
      `restyles_that_change_shape_match_a_fresh_build`. (`same_shape` was
      later replaced by `update_children`/`same_identity`; the test stays.)
    - `apply_root_box` `had_image` guard: `root_box_keeps_the_apps_own_image`.
    - loader `extensions()` (css/html/htm/slice.ron):
      `loaders_are_found_by_extension` (untyped loads).
    - `HtmlTemplate::name`: `template_is_named_after_its_asset_path`;
      `error_chain`: `error_chain_includes_all_sources`; outline indentation
      and blank-text skipping: `outline_indents_levels_and_shows_translations`;
      `From<tera::Context>`: `template_context_from_tera_keeps_variables`.
    - `apply_nine_slices` reload arm and `!` guard:
      `style_reload_reapplies_to_unchanged_frames` (registered system, so
      change ticks persist).
    Equivalent (documented; 6 left, all behaviour-neutral):
    - `build.rs` `box_of` `delete !` on `if !fill`, `cascade.rs`
      `BorderImageWidth/Outset` arm deleted, `Image::None` arm deleted,
      `unsupported` → `None`: each only changes whether a `debug!` line is
      logged (the fallback arm returns the same value).
    - `style.rs` `ParserOptions { filename }`: only the file name in CSS
      parse-error messages.
    - `build.rs` `same_shape` top check `||` → `&&`: every shape change bevy_markup
      produces also changes the child/span structure checked next, so a
      wrong top-level answer is rejected one level down (redundant check).
      Gone with `same_shape`; rerun mutants against `update_children`.
    Gotcha: under `-j 6` with the full suite, two mutants hit the 60 s test
    timeout from load alone; rerun timeouts alone with
    `CARGO_MUTANTS_MINIMUM_TEST_TIMEOUT=300` (the script already passes the
    flag, which can't be repeated).
  - [x] Triage the survivors of the partial runs (`e1b910c`; reruns with
    unit + html_ui + stateful). Killed by new tests:
    - `BoxStyle::is_empty` `&&`→`||` (background-only block lost its
      background): `background_only_block_keeps_its_background`.
    - `sliced_image` / `apply_nine_slices` dropping `VisualBox::BorderBox`:
      the dump prints `visual-box=` for frames not over the border box;
      `nine_slice::tests::frames_get_the_sliced_image_over_the_border_box`.
    - stylesheet reload arm deleted, and `!=`→`==` in the (since removed) failed-latch
      cleanup (unrelated failed UIs rebuilt on any reload):
      `stylesheet_reload_rebuilds_only_its_users` (per-entity build counts).
    - image reload arm deleted: `image_change_reslices_percent_frames`.
    - `LoadState::Failed` guard → `true` (unstyled flash while loading):
      `loading_stylesheet_defers_the_build`.
    - `||`→`&&` before the outline check: already killed by the stateful
      machine (it was excluded from the first run).
    - `flex_shrink`/`box_sizing`/`margin`/`overflow` field deletions in
      `spawn_item`/`spawn_block`: layout cases in `layout_flex` (overfull
      narrow rows: containers, plain/boxed blocks, unsized text) and
      `layout_sizes` (width + padding blocks, sized `pre`); boxed `li`
      indent in the background test; the dump prints non-default `overflow`.
    The triage also found bug_0014 (text in boxed blocks never wrapped).
    Equivalent (documented, not killable by behaviour):
    - `build.rs` `Styler::box_of` `delete !` on `if !fill`: only decides
      whether a `debug!` line is logged.
    - `style.rs` `ParserOptions { filename }` deleted: only the file name in
      CSS parse-error messages.
  - Note — cargo-mutants 27.1.0 quirk: `--re`/`--exclude-re` don't filter
    struct-field-deletion mutants (they always run); harmless, but a
    targeted rerun takes ~10 min instead of ~2.
  - [ ] After the full run: triage any new survivors the same way.
  - [x] Scheduled in CI: `.github/workflows/mutants.yml`, twice weekly (Sundays and Wednesdays; weekly until the repo went public),
    informational — it never fails on survivors, since the documented
    equivalents always survive.
  - Proptest regression seeds written while planting bugs by hand
    (`tests/layout_properties.proptest-regressions`,
    `proptest-regressions/*.txt`) are already covered: proptest replays them
    first on every run.
- [x] **7. Golden images, sparingly** (fixes the rest of weakness 3 and part of weakness 4: real font rasterization, wrapping and 9-slice drawing; CJK fallback is still untested, since only committed fonts are used). `tests/golden.rs` + `scripts/golden.sh` on Mesa lavapipe; three scenes (`text`, `frame`, `l10n`); see Testing. Brittle across drivers and font versions: a smoke check, not a spec.
  - [ ] A CJK/fallback scene would need a committed CJK font (or `system_fonts`, which isn't reproducible).

## Decision records

Significant design decisions — picking one approach over real alternatives,
changing a contract users rely on, reversing an earlier decision — get an
ADR in `docs/agents/adr/` (index: `README.md`, format and rules:
`docs/agents/skills/adr.md`). 0001–0012 were written retrospectively from the
git history up to `d2c9b63`. ADRs are snapshots: once accepted they're not
edited to match later code; a new ADR supersedes them.

## Bug reports

Bugs found by the automated testing above — or by any other means — are
filed in `docs/agents/bugs/` as `bug_NNNN.md`, numbered sequentially in
discovery order, with an `INDEX.md` title/metadata page linking all reports.
Whole-crate review sessions are filed in `docs/agents/reviews/` as
`review_NNNN.md` (one report per session; see
`docs/agents/reviews/`), a finding there gets its own
`bug_NNNN.md` when its fix lands.
File one for every real defect a fuzzer, property test, vector, or manual
investigation uncovers, *at minimum when the fix lands* (don't let the
analysis live only in commit messages or chat logs).

Per-bug file, whatever is known and useful:

- **Status** (open / fixed / upstream-unfixed) and **severity**; reachable
  from user data = high, even if only panics.
- **Component** and **symptoms** (the observable misbehavior, not the cause).
- **Discovery**: which tool/target found it (fuzz target, stateful op
  sequence, seed, minimizing commit), the repository commit at discovery, the
  agent/model, and the date.
- **Minimal reproduction** — a minimized input or op sequence, not the raw
  fuzzer artifact.
- **Root cause**, the **fix** (with commit), and the **regression test** that
  now guards it; upstream bugs note the affected/pinned versions and where
  the local patch (`[patch.crates-io]` + `vendor/`) is wired.

`INDEX.md` carries the shared metadata: date range, machine, OS/kernel,
toolchain and dependency versions, and the discovery agent. Keep it current
when new bugs are added; see the existing entries (bug_0001–bug_0005) as
templates.

Upstream defects — in dependencies and tools, whether or not they also have
a `bug_NNNN.md` — go in `docs/agents/bugs/UPSTREAM.md` until they're fixed
upstream: project and affected versions, reproduction, the local workaround
and where it lives, a PR sketch, and a status (unreported → reported → PR
open → fixed upstream → workaround removed). Add an entry whenever you
vendor/patch a dependency or work around a tool; when bumping a dependency,
check its entries and drop workarounds upstream has made unnecessary.

## Limits and next steps

The remaining authoring gaps and the order to close them in are the
roadmap ADR: `docs/agents/adr/0028-modern-authoring-gaps-roadmap.md` —
add new gap items there, not here.

Known limits (each skipped/ignored value is logged at `debug`):

- **Layout:** flex and grid layout, sizes, margins and `box-sizing` work on
  blocks, containers and the root (see the CSS subset); `position: sticky`, `order`,
  `place-*` shorthands, `gap` in `%`,
  font-relative `padding`/`border-width`/`gap`, or `ex`/`ch`.
  `em`/`rem` work for sizes, margins, insets, radius and grid tracks
  (Bevy's `Val::Em`/`Val::Rem`: `em` against each node's `EmSize` — the
  build sets it from the cascade's computed font-size on non-text nodes —
  and `rem` against the `RemSize` resource, synced to the root rule's
  font-size; no restyle is queued, Bevy re-resolves `Val::Rem` itself). `absolute` resolves against
  the parent (Bevy has no containing-block search); `fixed` resolves against
  the viewport (`FixedNode`). The `HtmlUi` node's default `Node` is a
  flex *row*: set `flex_direction: Column` on it or `html { flex-direction:
  column }` in CSS, or blocks sit side by side. The root rule has no
  `:hover`/`:focus` states (always `Pseudo::default()`).
- **`flex-shrink` defaults to 0, not 1 (deviation from CSS, open design
  question).** Blocks and containers never shrink below their content unless
  a rule says `flex-shrink: 1`. Copied browser CSS therefore misbehaves
  silently where it relies on shrinking: a scroll panel (`flex-grow: 1;
  min-height: 0; overflow-y: scroll`) never overflows — the tallest content
  sizes the whole chain and the root clips it (bug_0028; recipe in Gotchas) —
  and flex rows overflow instead of squeezing their items. Set in
  `build.rs` (containers, blocks, `li`; its stated reason: a scrolling parent
  overflows instead of squashing its blocks) and mirrored by the layout
  oracle's model CSS (`scripts/browser_oracle.py`, `flex-shrink: 0` on the
  container and block tags), so the oracle vectors assume it too. Kept for
  now because changing it alters every existing layout (examples, p19's
  templates) and those vectors; revisit as a deliberate change, re-deriving
  the vectors against plain browser defaults.
- **Grid placement by name:** names the parent template defines resolve
  (the Layout properties section covers the exact scope). Still open: names
  inside an `auto-fill`/`auto-fit` repeat (its count is layout-dependent —
  names before/at the repeat's start keep their positive spellings and the
  list's final boundary registers as `-1`), an occurrence past the template,
  `span` anchored at a named line, and a CSS cascade of names (the
  `GridNames` table is container-level: `grid-template`/`grid` reset it, a
  nameless track list clears that axis's names).
- **Tables:** the table tags map onto the flex model (ADR 0028): the
  table/sections/caption are columns, a `tr` is a row (the UA default; a
  `flex-direction` declaration wins), the cells are containers, a `th`'s
  text is bold. NOT CSS's table layout: no col/row spans, no column
  sizing.
- **Lists:** `list-style-type` (`disc`/`circle`/`square`/`decimal`/
  `none`, inherited; the UA-sheet defaults per tag — `ol` numbers,
  `ul` uses `disc`), counted by the parent list and restarting per
  list; the marker renders in the `li`'s own style. The `li`'s 12px
  indent is CSS-overridable per side. A nested list inside an `li`
  renders as inline content (containers inside blocks are inline — the
  mixed-content rule).
- **Selectors:** lightningcss parses them and its engine,
  parcel_selectors, matches them (ADR 0034) over `cascade/selector.rs`
  `Node`: an `Element` implementation on the walk's own structures — the
  element, its ancestor chain (nearest first; each `Ancestor` carries a
  `SiblingLink` into its parent's `SiblingList`) and stand-in siblings from
  `Position` where no list is threaded (tests, image runs, the root).
  `Selector::from_parsed`'s allowlist admits: types/`*`, classes, ids;
  `data-*` attribute selectors with every operator and the `i` flag (the
  element model carries only `data-*` attributes, so another attribute
  drops the rule); descendant/child combinators; `+`/`~` adjacent to the
  subject (ADR 0028; a sibling's interaction flip restyles its following
  siblings' matches); `:hover`/`:active`/`:focus`/`:focus-visible`,
  `:focus-within`; `:nth-child()`, `:nth-last-child()`, `:nth-of-type()`,
  `:nth-last-of-type()`, `:first-/last-/only-child`, `:first-/last-/only-of-type`,
  `:empty`; `:is()`/`:where()`/`:not()` with lists and descendant/child
  combinators inside, but no `:not()` within a `:not()` (ADR 0029's
  reactivity answers a pseudo-class by `!in_negation()`, exact only at one
  level). The allowlist also keeps out what parcel's matcher panics on
  (`:nth-child(… of S)`, `:has()`, `&` nesting, `:nth-col()`).
  parcel's `An+B` arithmetic is checked `i32`: `:nth-child(n-2147483647)`
  matches nothing (CSS: everything); no page can tell. The style cache keys on what the
  sheet's selectors read (`SelectorReads`). For the skeleton, a sibling
  list whose shape changes (length, node kind, tag) un-cleans its children
  when the sheet is sibling-dependent; an attribute-only change un-cleans
  the following siblings under `~`, the next one under `+`, nothing under
  a position-only sheet (`SiblingChange`). No other pseudo-classes or pseudo-elements, no
  `:root`, no sibling combinator away from the subject or inside an
  argument (a selector with one skips the rule: it would need restyle
  invalidation across a sibling's subtree, ADR 0034).
- **Properties:** `text-align`/`line-height`/`letter-spacing` and
  `list-style-type` work (ADR 0027); `text-decoration`'s
  `overline`/`blink`/`-thickness`/`-style` are skipped (underline,
  line-through and a declared color work), border styles (every
  border is solid); `overflow: auto` promotes to scrolling one frame
  after layout decides the content overflows (the promotion system;
  the calc resolver's one-frame model). Lengths: px/em/rem/% for `font-size`, absolute only
  for `padding`/`border-width`/`gap`; px/%/viewport units for sizes and margins,
  plus `calc()` in sizes/margins/insets, including `min()`/`max()`/`clamp()`
  (`clamp(min, v, max)` = `max(min, min(v, max))`; single-unit expressions at style
  time, mixed-unit resolved per frame by `resolve_calc` against the
  containing block's content box, the font sizes and the viewport).
- **border-image:** center always drawn, `-width`/`-outset` ignored, one
  repeat mode for all sides (Bevy `TextureSlicer` limits).
- **Inline:** no inline boxes with content — `background-color` paints the
  run (`TextBackgroundColor`) and `text-decoration` (`underline`,
  `line-through`, a declared `text-decoration-color`) draws on the run, but
  borders, frames and padding on inline elements are ignored (Bevy
  `TextSpan` has no box). `<img>` (asset paths) flows inline
  (`InlineBox`/`InlineImage`), sized by the CSS `width`/`height` (px or
  `%` of the containing block's *used* content box — resolved at layout
  time by `size_images`; other units skipped; one dimension preserves the
  aspect ratio, both stretch; undeclared keeps the intrinsic size —
  the `width`/`height` attributes are gone); `<a href>` spans become picked
  entities carrying the bindings — clicking emits the `open` signal with
  the href payload, the deepest-bound-element rule applies (a link inside
  a `data-on-click` block suppresses it), `HtmlElements` finds them, and
  `:hover` works (their spans carry `DomNode` + `PseudoState`); a link
  without an href is plain styled text. Other inline content keeps no
  entity, so `HtmlElements` can't find it (only blocks/containers and
  link spans), and `display: inline-block` with widget children isn't
  supported (Bevy reserves the space but doesn't place content in it).
- **Interactivity:** no `position: sticky` (bevy has no sticky position;
  a layout-time scroll-aware component would be a new feature with its
  own ADR), no `<textarea>` (bevy's `TextInput` widget is single-line
  only), no built-in `<input type="checkbox">`/`<input type="radio">`
  widgets (bevy has no checkbox; build one with the custom-elements
  mechanism — `is="checkbox"` + a click handler). Text input behind the
  `text-input` feature
  (`<input type="text">` — a missing `type` counts as `text`; other types
  don't become fields — → Bevy 0.20's `EditableText` +
  `TextInput`; the value is app state — read the entity's `EditableText`, a
  re-render sets the text only when the `value` attribute changes; the
  input's grid placement (named lines included) and styling are kept on
  updates like any element's; `maxlength` maps to `max_characters`
  (ADR 0023), `placeholder` isn't read).
  Declaratively
  (always): `data-on-<trigger>`/`data-with` emit
  [`ElementSignal`] messages (`signals.rs`; click/auxclick/press/release via
  picking observers — one per event type per element, mapping the pointer
  button to the trigger and reporting it in `SignalSource::Pointer` — enter/leave via hover tracking that survives rebuilds — the
  deepest bound element wins, so buttons can nest; forms: a `<form
  data-on-submit="name">` whose `<button type="submit">` (or a `type`-less
  `<button>`) is clicked — or activated (Enter / A on the focused one; the
  activation is the submit's source) — emits the form's `Submit` signal with
  the named inputs' values as a JSON
  object — unnamed inputs excluded; only the submit button submits — any
  other bound element in the form (`type="button"`, a link, a bound wrapper)
  just clicks, and a click on the submit button suppresses a bound
  ancestor's `click`; the collection reads each input's live
  `EditableText`, so without `text-input` (no input entities) the payload
  is empty; the submit button is a plain block carrying the
  `FormSubmitButton` marker — no bindings of its own, but its click
  submits, so its observers attach for `spec.submit` too, and
  `update_spec` keeps marker/observers/`ElementSignals` in step on
  restyles). What signals *mean* is app code reading the queue (the demo's
  `controls::read_signals`). No built-in reactions. `:hover`/`:active` styling works
  (see the Selectors limit); focus and directional navigation are
  library-side (`focus.rs`), input bindings are the app's. No
  `:focus-within`. Tab order is HTML's (`HtmlFocus::tab` / `tab_order`,
  its own document-order walk: Bevy's `TabNavigation` skips entities whose
  `InheritedVisibility` hasn't propagated, which headless apps never do). App
  components on elements: `is="<name>"` + `data-*` runs the app's
  `define_html_element` system on every spawn (`custom_elements.rs`); no
  disconnected/attribute-changed callbacks, no autonomous custom tags
  (`<my-tag>` is walked through like any unknown tag).
- **Updates:** reconciliation matches by unique `id`, else by position — no
  moves of id-less elements (inserting one before others replaces those after
  it, like keyless React lists); an identical render is skipped. A changed
  span count (e.g. a run merge) respawns the block's spans, not the block.
  Updates write a kept element's components only where they differ
  (`build.rs` `put`, `set_if_neq`; `ImageNode` frames are re-inserted, it has
  no `PartialEq`), so unchanged elements keep their change ticks. ADR 0021
  went further: unchanged subtrees reuse the previous build's specs by
  reference and skip the cascade, the spec construction and the diff visit
  (the skeleton, `SpecNode::Fresh`/`Cached`, `put_ref`) — content-only:
  a pending style-side change (`RebuildState::style_pending`) skips reuse;
  measured
  (criterion, 600 elements, skeleton on/off): changing 2.09 → 1.17 ms,
  markdown 8.75 → 6.27 ms, all-dirty/localized/idle within noise. The
  remaining per-frame costs are the render, the parse, the localize walk
  and Bevy's own schedule/layout/text — none of them the rebuild.
  Nested `HtmlUi`s go with a replaced slot; re-nest
  them on `HtmlUiBuilt`.
- **Text:** `pre` has a fixed 8px padding; `white-space` works (ADR 0028:
  the four kinds; a declaration overrides the `pre` tag's UA default);
  whitespace collapsing doesn't know CJK (wrapped CJK source lines become
  spaces); the legacy non-`;` entity aliases (`&lt`-style) stay verbatim
  (the table holds the semicolon forms); `text-overflow: ellipsis` is
  unsupported (bevy's truncation hard-clips, no ellipsis glyph). Mixed inline content
  directly in a container (`<div>Mixed <b>bold</b> text</div>`, also from a
  translation) becomes one anonymous block per text piece, not one line.
- **Fluent:** numbers format without locale grouping (fluent-rs; the loader
  exposes no configuration for `FluentBundle::set_formatter` yet) — pre-format
  in Tera if needed.
- **Fonts:** no bundled CJK font; Japanese relies on `system_fonts`. Bevy's
  default font covers printable ASCII only, so with no registered family even
  the `li` bullet (`•`) comes from font fallback.

Next steps (roughly in order of value):

1. ~~Flex layout from CSS~~ (done: flex, sizes, margins, `box-sizing`).
2. ~~Descendant/child combinators (`.panel p`, `.panel > p`)~~ (done:
   the walk threads an `Ancestor` chain (element + pseudo) through the
   cascade; specificity sums; the style cache folds a chain hash into its
   key; `tests/vectors/combinators/` is checked against Chrome).
3. ~~`:hover` / `:active` via `Interaction` or picking, re-styling without a
   full rebuild~~ (done: `PseudoState` from the picking hover map +
   pressed entities; a state change is a `Restyle` — in place; `:focus` /
   `:focus-visible` from `InputFocus` since, see `focus.rs`).
4. Lists done properly: `list-style-type`, `ol` numbering, CSS-driven indent.
5. Text properties: `text-align` (`Justify`), `line-height` (`LineHeight`).
6. ~~Keyed reconciliation for content changes: keep entities for unchanged
   elements (key `NodeSpec`s by DOM path) so app-attached state survives
   template/locale changes too and large documents stay cheap. (Restyle in
   place for style-only changes is done.)~~ Done a different way (ADR 0021,
   after 0017's fingerprint variant measured +28%): the skeleton pairs the
   fresh DOM with the previous build by *content*, and unchanged subtrees
   skip the cascade, the spec construction and the diff visit; benchmark
   suites (ADR 0018/0019) guard it.
7. More test vectors alongside each of the above.

## Verification

- CI (`.github/workflows/`): `ci.yml` enforces the rules below on every push
  to `main` and every PR (`RUSTFLAGS`/`RUSTDOCFLAGS=-D warnings`, debug info
  off to fit the runner's disk), and that `tests/vectors/*/fluent.html` is
  what `scripts/fluent_oracle.sh` produces. `nightly.yml` runs the golden
  images on Ubuntu's lavapipe — the runner is the canonical renderer
  (2026-10-08): when its Mesa renders differently from the references
  beyond the tolerance, the job fails and uploads `golden-diffs`; re-record
  the references with the manual `Update golden references (lavapipe)` job
  (workflow_dispatch), download its `golden-references` artifact, copy the
  PNGs over `tests/golden/*/expected.png`, review and commit. It checks
  `browser.json` against the runner's Chrome
  (ignoring the generator line; a failure means Chrome changed or a vector
  is stale), fuzzes each cargo-fuzz target for 15 min (`-rss_limit_mb=4096`) and builds the other
  fuzz drivers. Nightly failures are reports to triage, not merge blockers.
  Fuzz corpora persist across nights as GitHub caches
  (`fuzz-corpus-<target>-<run>`; restored from the newest, saved after
  every run, also after a crash) and are minimized with `cargo fuzz cmin`
  on Sundays. Before this (2026-10-05) every night started from an empty
  corpus: 60 s reached cov 3830 on `html` vs 5572 for the local corpus.
  nightly
  `coverage` job runs `scripts/coverage.py --html`: the per-layer table goes
  to the job summary, the reports to the `coverage` artifact (the fuzz-corpora layer
  replays the committed seeds plus the newest CI corpus caches). Each fuzz
  job also uploads its corpus as the `fuzz-corpus-<target>` artifact (90
  days) so CI's finds can be merged into `fuzz/seeds` (testing guide).
  First local measurement (2026-10-04): 94.3% of 1684 library lines
  covered by some layer. Its two gaps are closed: the template
  *render*-failure path (`template_render_failure_shows_the_error_and_recovers`
  in `html_ui`) and the CSS mapping arms nothing ran
  (`alignment_values_map_to_bevy_values`,
  `units_keywords_and_unsupported_values` in `cascade::tests`) — measured
  98.0% then. Latest measurement (2026-10-05, at `96f2cd3`): merged 88.1%
  of ~3112 lines — the total grew with the interaction/focus work and its
  new uncovered paths. The new `signals` layer (unit-style tests + signal
  properties) covers `src/signals.rs` at 95.1% alone, 99.0% merged (the
  rest is two defensive race guards). The rest of `uncovered.txt` is
  `#[derive]` lines, `debug!` fallbacks, defensive early returns and the
  outline-mode render failure.
  Mutants couldn't flag the mapping gaps (no arm of an exhaustive match can
  be deleted): read coverage and mutation results together.
  GitHub moves `ubuntu-latest` to Ubuntu 26 from 2026-10-19: that can change
  Mesa (golden images: lavapipe rendering vs the references) and the apt
  package names (Bevy/honggfuzz build dependencies). If the nightly breaks
  around then, suspect the image first (`golden-diffs` artifact, apt
  errors); re-record the references with the `Update golden references
  (lavapipe)` job; pinning `runs-on: ubuntu-24.04` is the quick fallback.
  The nightly golden job has been failing since 2026-10-06 on the stale
  references (recorded 2026-10-04 locally, before the renderer moved) —
  re-record with the dispatch job once it lands.
  First CI runs (2026-10-04): CI and nightly golden, browser oracle and
  fuzz-driver builds green; the cargo-fuzz jobs needed
  `fuzz-libfuzzer.sh` to pass `--target` (the prebuilt cargo-fuzz defaults
  to musl, which ASan rejects).
  Shared build caches (2026-10-05): every job used to build its own
  dependencies; the 3 cargo-fuzz jobs rebuilt from scratch daily (a new
  nightly changes rust-cache's key: 11–19 min each for 60 s of fuzzing) and
  the mutants shards missed after a week. Now `cache.yml` builds the
  `mutants` and `fuzz` caches once, CI's `test` job the `dev` cache, and
  consumers only restore them; the fuzz jobs pin `BEVY_MARKUP_NIGHTLY`
  (`nightly-2026-09-09`, the nightly every fuzz driver was verified on
  locally; bump it in nightly.yml and cache.yml together). Verified locally
  in a fresh target dir: after the producer commands, cargo-mutants'
  baseline, `cargo test --test golden` (under CI's env) and `cargo fuzz
  build` → `run` compile no dependencies. Details and the producer/consumer
  table: `docs/agents/skills/testing.md`, CI. A consumer's `::warning::`
  "no exact … build cache" means the pair's env, toolchain or keys drifted.
- Warning-free: `cargo check --lib` (minimal Bevy features), `cargo check
  --lib --features text-input` (that feature's mapping arms compile only
  under it — a plain `--lib` check never sees them), `cargo build
  --all-targets`, `cargo doc --no-deps`. `cargo test` must pass: cascade unit
  tests, the headless test vectors, and the `no_run` doc examples (see
  Testing). Library changes that alter the mapping need a vector.
- The test harness shuffles schedule order (`ScheduleBuildSettings::
  shuffle_seed`, via the dev-dependency `bevy/debug` feature): every
  `cargo test` run gets a fresh seed (printed to the failing test's output),
  so an ordering that only holds by luck — 0.20 made the `UiSystems` sets
  weakly ordered (`chain_weak`) — fails in CI instead of in the wild. If a
  shuffle-dependent failure appears, pin the ordering explicitly (`.before`
  / `.after` in `src/lib.rs`'s `BevyMarkupPlugin` system wiring) rather than
  trusting the default order.
- Visual changes: run an example and capture an in-app screenshot. The
  dev-tools feature serves every example over localhost BRP + MCP
  (`bevy_mcp_harness`, see `examples/shared/harness.rs`): run
  `cargo run --example <name> --features dev-tools`, then
  `game/screenshot` (`{"label":"…"}`) over HTTP POST
  `127.0.0.1:15702` — the full-resolution PNG lands in
  `mcp_harness/screenshots/` (gitignored); downscale to ≤1280 px and
  pngquant + oxipng it into `examples/screenshots/<name>.png`. The same
  surface playtests the examples for real: `game/ui` (the laid-out tree,
  `clickable` flags via the `data-on-click` hook), `click_node`,
  `game/keyboard`/`game/gamepad`/`game/mouse` mocks, `wait_until`/`game_assert`
  (the MCP tools are thin proxies to the BRP methods; the MCP flow
  initializes a session first). The old throwaway screenshot system still
  works when the feature is off:

  ```rust
  app.add_systems(Update, |mut c: Commands, t: Res<Time>, mut done: Local<bool>| {
      if !*done && t.elapsed_secs() > 5.0 {
          *done = true;
          c.spawn(bevy::render::view::screenshot::Screenshot::primary_window())
              .observe(bevy::render::view::screenshot::save_to_disk("/tmp/bevy_markup_shot.png"));
      }
  });
  ```

- DOM outlines (`HtmlDebugOutline`) are logged at `debug`:
  `RUST_LOG=bevy_markup=debug timeout 15 cargo run --example demo`.
- Pointer interaction: write `bevy::window::WindowEvent::{CursorMoved,
  MouseWheel, MouseButtonInput}` messages from a throwaway system (move, then
  press and release in later frames), then check state. Real input sometimes
  reaches the window during runs; prefer setting resources directly when
  clicks aren't what's under test.
