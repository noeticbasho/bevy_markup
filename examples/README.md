# Examples

Runnable mini-apps, one directory each, each with its own README showing a
screenshot and explaining what it shows and how to drive it. Their
templates, stylesheets, translations and images live in `examples/assets/`
(one `AssetPlugin` root shared by all). Text uses the fonts installed on
your system (CSS `serif`, `sans-serif`, `monospace`), so no font files ship
with the repository.

Every example works with mouse, keyboard and gamepad alike: arrows / D-pad /
left stick move focus, Enter / A press, and each shortcut has a gamepad
button (shown on screen). bevy_markup owns focus and activation; the
examples bind the input (`shared/input.rs`).

Gamepad buttons are named by the Xbox layout (A bottom, B right, X left,
Y top), and the examples bind them by position. If X and Y (or A and B)
seem swapped and the log shows `No mapping found for UUID …`, Bevy's
gamepad backend (gilrs) has no SDL mapping for the pad and guessed its
button order. This happens on a Flydigi Apex 5, for example; a Direwolf 4
maps correctly. Give the pad a mapping line through the
`SDL_GAMECONTROLLERCONFIG` environment variable (SDL's `controllermap`
or the SDL2 Gamepad Tool generates one), or submit the line to
[SDL_GameControllerDB](https://github.com/mdqinc/SDL_GameControllerDB),
which gilrs bundles.

## Start here

| | Example |
|---|---|
| <img src="screenshots/quickstart.png" width="320"> | [`quickstart`](quickstart/) — `cargo run --example quickstart`<br><br>The whole API in one small app: an HTML template + CSS + Fluent become a native UI, with two clickable buttons (`data-on-click`), keyboard/gamepad focus, runtime language switching and Tera-driven in-place updates. Read this one top to bottom — it's deliberately self-contained. |

## Each feature in context

| | Example |
|---|---|
| <img src="screenshots/inputs.png" width="240"> | [`inputs`](inputs/) — `cargo run --example inputs --features text-input`<br><br>Text fields: `<input type="text">` renders Bevy's editable-text widget; the app reads the fields' `EditableText` into the context, so a preview follows every keystroke, and reset buttons write fields back. Without the feature the fields don't render. |
| <img src="screenshots/forms.png" width="240"> | [`forms`](forms/) — `cargo run --example forms --features text-input`<br><br>Form submission: clicking a `<button type="submit">` emits the form's `Submit` signal with a JSON object of the named fields' live values; the template swaps the form for a receipt panel (plus a non-submitting `type="button"`). |
| <img src="screenshots/links.png" width="240"> | [`links`](links/) — `cargo run --example links`<br><br>`<a href>` links: clicks (and keyboard activation) emit the `open` signal with the href, recorded in a status line; ← → ↑ ↓ / LB RB cycle the links and the Language button, with `:focus-visible` styling. |
| <img src="screenshots/grid-names.png" width="240"> | [`grid-names`](grid-names/) — `cargo run --example grid-names`<br><br>Named grid lines and areas: a board grid whose panels place by their area's name (`grid-area: portrait`, one spanning two rows) and a supply list placing between named column lines (`grid-column: left / right`); clicking parts checks them off in place. |
| <img src="screenshots/combinators.png" width="240"> | [`combinators`](combinators/) — `cargo run --example combinators`<br><br>CSS combinators: selecting a group flips its class, and two rules reach different depths — `.selected p` highlights every paragraph inside, `.selected > p` only the direct ones (the nested one stays dim). |
| <img src="screenshots/hot-reload.png" width="240"> | [`hot-reload`](hot-reload/) — `cargo run --example hot-reload --features hot-reload`<br><br>Bevy's asset file watcher: edit a template or stylesheet on disk and the UI restyles without a restart; the app also rewrites its own stylesheet on a keypress, so the demo works without an editor. |
| <img src="screenshots/calc.png" width="240"> | [`calc`](calc/) — `cargo run --example calc`<br><br>CSS `calc()` in layout: a fluid content column (`calc(100% - {{ sidebar }}px - 12px)`), a card row dividing it (`calc(100% / {{ cards }} - 8px)`), and an `em`-based indent — steppers change the numbers and the expressions re-resolve in place. |
| <img src="screenshots/menu.png" width="240"> | [`menu`](menu/) — `cargo run --example menu`<br><br>A settings menu for mouse, keyboard and gamepad: focus with a `:focus-visible` ring, a volume slider, a difficulty stepper, `data-tooltip`, an `HtmlModal` confirm dialog, signals routed to systems and a Tera 2 component library. |
| <img src="screenshots/grid.png" width="240"> | [`grid`](grid/) — `cargo run --example grid`<br><br>CSS grid layout, three ways: responsive `auto-fill` slots with spanning and dense packing, a fixed paper-doll grid placed by line numbers, and equal columns of nested card grids — tabs switch between them. |
| <img src="screenshots/inline.png" width="240"> | [`inline`](inline/) — `cargo run --example inline`<br><br>Inline styling: `background-color` and `text-decoration` on text runs, CSS-sized `<img>` images, elliptical `border-radius`, and a `position: fixed` ribbon. |
| <img src="screenshots/live.png" width="240"> | [`live`](live/) — `cargo run --example live`<br><br>Live game data: a party list rendered from state every frame and updated in place — rows keep their focus and hover while members join and leave; inline styles, opacity fading. |
| <img src="screenshots/markdown.png" width="240"> | [`markdown`](markdown/) — `cargo run --example markdown`<br><br>Localized Markdown documents (ADR 0014): a reader whose pages are `.md` files and whose prose is Fluent values in Markdown (`markdown: true`), with raw-HTML UI embedded in the documents. |
| <img src="screenshots/world.png" width="240"> | [`world`](world/) — `cargo run --example world`<br><br>Styled, localized nameplates over walking 3D units (`HtmlWorldAnchor`), with distance fade and HUD buttons over the 3D scene. |
| <img src="screenshots/demo.png" width="240"> | [`demo`](demo/) — `cargo run --example demo`<br><br>Every feature at once: themes and languages, scrolling panels with styled scrollbars, DOM outlines, 9-slice frames, per-theme chrome — the big one, also the agent/QA playground with `--features dev-tools`. |

## Testing and profiling

| | Example |
|---|---|
| <img src="screenshots/testbench.png" width="240"> | [`testbench`](testbench/) — `cargo run --release --example testbench --features hotpath`<br><br>An interactive torture bench: performance workloads (large and churning documents, restyle storms, Markdown, Fluent, many UIs, `calc()`), security inputs that once crashed or injected, and live correctness comparisons (reuse vs a fresh build, indented vs flat Markdown), each with pass/fail checks; with `hotpath`, per-function timing in the real frame loop (`hotpath console` for the live view). |

## Shared code

- `shared/input.rs` — `ExampleInputPlugin`: arrows / D-pad / left stick move
  focus with hold-to-repeat (leaving a focused text field's caret keys
  alone), Enter / A activate, and `Hotkeys` maps a shortcut key to a
  gamepad button.
- `shared/harness.rs` — the `dev-tools` feature's agent/QA surface
  (`bevy_mcp_harness`) and the examples' launcher (`harness::run`), which
  picks the app composition from the command line; compiles away without
  the feature.

With `--features dev-tools`, every example also serves a localhost agent/QA
tool surface: BRP on 127.0.0.1:15702 and MCP on 127.0.0.1:15710/mcp —
screenshots (`game/screenshot`), the laid-out UI tree (`game/ui`), and
mocked keyboard/gamepad/mouse input. For agent QA without a display (a
windowed run stops ticking when its window is unfocused), two headless
compositions exist:

- `--no-render` — render-less: no window, no winit, no wgpu. UI layout,
  picking, `game/ui` and the input mocks all work; `game/screenshot`
  returns a clean error (nothing renders). The exception is `world`:
  its 3D setup (meshes, `StandardMaterial`) needs render resources
  registered by bevy_render — run it `--offscreen` instead.
- `--offscreen` — windowed rendering minus winit: every camera renders
  into the harness's offscreen texture, so `game/screenshot` captures
  real frames (needs a GPU or lavapipe).

Both drive frames from `ScheduleRunnerPlugin`, so nothing ever waits for
window focus.
