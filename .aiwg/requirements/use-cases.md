# Use-Case Briefs

| Field | Value |
|---|---|
| Template | `requirements/use-case-brief-template.md` (one brief per section) |
| Derived from | `AGENTS.md` ("Public API", "Limits and next steps", "Verification"), ADRs 0001–0033, examples |
| Source state | `f6a2d6a`, 2026-10-28 |

Status: **Built** or **Planned**. IDs are stable. Each brief names the example that exercises it (`cargo run --example <name>`).

## Actors

| Actor | Who |
|---|---|
| Developer | Bevy app developer adding `BevyMarkupPlugin` |
| Author | Writes templates, stylesheets and Markdown pages |
| Translator | Writes Fluent (`.ftl.ron`) bundles |
| Player | End user of the game or app, with mouse, touch, keyboard or gamepad |
| Agent | AI coding agent using the `dev-tools` QA surface |

## Built

### UC-001 — Developer shows a template as Bevy UI

- **Main success scenario:** spawn `HtmlUi(template)` with a `TemplateContext`; the pipeline renders, parses, localizes, cascades and builds native UI entities; mutating the context re-renders and rebuilds only if the HTML changed.
- **Notes:** `RenderedHtml` exposes `Pending`/`Ready`/`Failed`; a render failure shows the error and recovers. Example: `quickstart`. Sources: ADR 0001, 0002, 0005.

### UC-002 — Author styles and lays out UI with CSS

- **Main success scenario:** a stylesheet (per entity or `DefaultStylesheet`) styles elements through the cascade; flex, grid (named lines and areas), positioning and `calc()` lay them out; `border-image` draws 9-slice frames; swapping the default sheet changes theme.
- **Notes:** the supported subset and its deviations are documented limits, each skipped value logged at `debug`. Examples: `grid`, `grid-names`, `calc`, `combinators`, `demo`. Sources: ADR 0003, 0007, 0027, 0028, 0030.

### UC-003 — Translator localizes the UI

- **Main success scenario:** elements carry `data-l10n-id` (and `data-l10n-args`); setting `ActiveLocale` translates them, with plurals and `data-l10n-name` overlays; swapping it changes language at runtime.
- **Notes:** numbers format without locale grouping. Examples: `demo`, `menu`. Sources: ADR 0013, bug_0006, bug_0007.

### UC-004 — Author writes pages in Markdown

- **Main success scenario:** a `.md` template is converted to HTML at load (pulldown-cmark) and styled like any template.
- **Notes:** indentation inside Tera blocks or HTML containers is not Markdown; indented code blocks are disabled. Example: `markdown`. Sources: ADR 0014, 0020, 0024–0026.

### UC-005 — Player interacts with the UI

- **Main success scenario:** `data-on-<trigger>` elements emit `ElementSignal`s on click, press, release, enter and leave (the deepest bound element wins); `:hover`/`:active` restyle in place; the app reacts by reading the messages or routing by name (`on_html_click`).
- **Notes:** forms emit a `Submit` signal with the named inputs' values. Examples: `menu`, `forms`, `links`. Sources: ADR 0006, 0011.

### UC-006 — Player navigates with keyboard or gamepad

- **Main success scenario:** the app binds its own input to `HtmlFocus::navigate` (compass directions) and `activate`; focus moves between focusable elements, `:focus-visible` shows it, activation emits the element's click signals; `HtmlModal` confines focus.
- **Notes:** no Tab order (directional only). Example: `menu`. Source: ADR 0008.

### UC-007 — App updates the UI in place

- **Main success scenario:** a context, locale or stylesheet change updates the existing entity tree: identical renders are skipped, elements are matched by `id` or position, unchanged subtrees are reused, style-only changes restyle in place; `HtmlUiBuilt`/`HtmlUiRestyled` report what happened.
- **Notes:** app components attached to kept elements survive; handlers on `HtmlUiBuilt` must be idempotent. Example: `live`. Sources: ADR 0009, 0017, 0021, 0029.

### UC-008 — Developer shows overlays and tooltips

- **Main success scenario:** `data-tooltip` shows a tooltip template beside the hovered element; `HtmlAnchor` keeps an overlay beside an element; `HtmlWorldAnchor` keeps a UI root over a 3D entity.
- **Example:** `world`. Source: ADR 0010.

### UC-009 — Developer attaches app behaviour to elements

- **Main success scenario:** `define_html_element(name, system)` runs the app's system for every spawned `is="<name>"` element with its `data-*` dataset; `HtmlElements` finds elements by id, class or tag.
- **Example:** `inputs`. Source: ADR 0012.

### UC-010 — Player types into a text field

- **Main success scenario:** with the `text-input` feature, `<input type="text">` becomes Bevy's editable text widget; `maxlength` limits characters; the form's submit collects the values.
- **Notes:** single-line only, no `placeholder`. Examples: `forms`, `inputs`. Sources: ADR 0016, 0022, 0023.

### UC-011 — Developer shows a long scrolling document

- **Main success scenario:** a long list in a scroll container builds only the rows in and near the visible band; scrolling builds the newly visible rows.
- **Notes:** a height change above the band can misplace rows for one frame (bug_0170, won't fix). Sources: ADR 0031–0033.

### UC-012 — Author edits assets with hot reload

- **Main success scenario:** with the `hot-reload` feature, editing a template, stylesheet or bundle on disk updates the running UI.
- **Example:** `hot-reload`. Source: `AGENTS.md` Conventions.

### UC-013 — Agent playtests an example

- **Main success scenario:** run an example with `--features dev-tools`; over localhost BRP/MCP the agent reads the UI tree, clicks nodes, mocks keyboard/gamepad/mouse input, asserts and takes screenshots.
- **Notes:** dev only, never in a shipped build. Source: `AGENTS.md` Verification.

## Planned (ADR 0028 tier 4)

### UC-101 — Author uses SVG images

- **Status:** open. Source: ADR 0028.

### UC-102 — Author writes responsive CSS with `@media`

- **Status:** open. Source: ADR 0028.

### UC-103 — Author uses shadows and gradients

- **Status:** open (`box-shadow`, `text-shadow`, gradients as images). Source: ADR 0028.

### UC-104 — Author animates with `@keyframes`

- **Status:** open; must fit the rebuild model. Source: ADR 0028.

Deferred until Bevy offers the capability: `position: sticky`, `<textarea>`, built-in checkbox/radio (ADR 0028).

## Sources

- @AGENTS.md
- @docs/agents/adr/README.md
- @docs/agents/adr/0001-html-tera-fluent-css-onto-bevy-ui.md
- @docs/agents/adr/0006-declarative-interaction-signals-and-pseudo-classes.md
- @docs/agents/adr/0008-focus-and-navigation-in-the-library.md
- @docs/agents/adr/0009-in-place-update-model.md
- @docs/agents/adr/0010-overlays-anchors-and-tooltips.md
- @docs/agents/adr/0012-template-composition-and-examples.md
- @docs/agents/adr/0013-own-the-fluent-bundle-asset.md
- @docs/agents/adr/0016-text-input.md
- @docs/agents/adr/0021-per-element-dirty-tracking.md
- @docs/agents/adr/0028-modern-authoring-gaps-roadmap.md
- @docs/agents/adr/0032-windowed-build-revised.md
- @examples/README.md
