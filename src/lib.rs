//! # bevy_markup — HTML + CSS + Fluent + Tera as Bevy UI
//!
//! Write UI as HTML templates; get Bevy UI nodes.
//!
//! ```text
//! .md template ──pulldown-cmark once at load──▶ .html template (Tera intact)
//! .html/.md (Tera template) ──render(TemplateContext)──▶ HTML ──tl──▶ DOM
//!   ──Fluent (data-l10n-id, ActiveLocale)──▶ localized DOM
//!   ──CSS (DefaultStylesheet / HtmlStylesheet, FontFamilies)──▶ Bevy UI nodes
//! ```
//!
//! ## Quick start
//!
//! ```no_run
//! use bevy::prelude::*;
//! use bevy_markup::prelude::*;
//!
//! fn main() {
//!     App::new()
//!         .add_plugins((DefaultPlugins, BevyMarkupPlugin))
//!         .add_systems(Startup, setup)
//!         .run();
//! }
//!
//! fn setup(
//!     mut commands: Commands,
//!     asset_server: Res<AssetServer>,
//!     mut fonts: ResMut<FontFamilies>,
//! ) {
//!     commands.spawn(Camera2d);
//!
//!     // CSS `font-family: Spectral` → these files.
//!     fonts.insert(
//!         "Spectral",
//!         FontFaces::new(asset_server.load("fonts/Spectral-Regular.ttf"))
//!             .with_bold(asset_server.load("fonts/Spectral-Bold.ttf")),
//!     );
//!     // Applies to every `HtmlUi` without its own `HtmlStylesheet`.
//!     commands.insert_resource(DefaultStylesheet::new(asset_server.load("ui/theme.css")));
//!     // `data-l10n-id` attributes resolve against this Fluent bundle.
//!     commands.insert_resource(ActiveLocale::new(
//!         asset_server.load("locales/en-US/main.ftl.ron"),
//!     ));
//!
//!     // One component: the template. `Node`, `TemplateContext`, … are required
//!     // components; override any of them in the same bundle.
//!     commands.spawn((
//!         HtmlUi::new(asset_server.load("ui/hud.html")),
//!         TemplateContext::new().with("player", "Ada").with("coins", &3),
//!         Node { flex_direction: FlexDirection::Column, ..default() },
//!     ));
//! }
//! ```
//!
//! Mutating [`TemplateContext`](html::TemplateContext) re-renders — and
//! updates the UI only if the rendered HTML changed, in place: elements still
//! in the document (matched by `id`, else by position) keep their entities,
//! interaction state and app components, so templated per-frame values
//! (`style="width: {{ hp }}%"`) are fine. Subtrees whose content is
//! unchanged reuse the previous build's specs outright (no re-styling, no
//! diffing), so a frame's cost tracks what changed, not the document size;
//! swapping
//! [`ActiveLocale`](l10n::ActiveLocale) re-localizes; swapping
//! [`DefaultStylesheet`](style::DefaultStylesheet) restyles — all at runtime.
//! After every content update an [`HtmlUiBuilt`](html::HtmlUiBuilt) event
//! fires on the `HtmlUi` entity, and [`HtmlElements`](html::HtmlElements)
//! finds elements by `id`/`class`. Kept elements keep what was attached to
//! them, so declare behaviour in the template rather than attaching it per
//! build: `data-on-click` signals for interactions, and
//! [custom elements](custom_elements) for components and observers (run
//! once per element). Style-only changes
//! (stylesheets, fonts) restyle the existing children in place and fire
//! [`HtmlUiRestyled`](html::HtmlUiRestyled) instead, keeping what you
//! attached — unless the restyle respawns an element's spans (new span
//! entities need their observers wired, link spans in particular), which
//! reports [`HtmlUiBuilt`](html::HtmlUiBuilt) as any other update.
//!
//! ## Supported subset
//!
//! - **HTML** ([`html`]): blocks `h1`–`h6`, `p`, `li`, `pre`, loose text;
//!   containers `div`, `section`, `ul`, … as nested column nodes; other
//!   elements are inline text (inside a block) or walked through. Each block
//!   becomes a `Text` node with a `TextSpan` per styled run; blocks and
//!   containers carry an [`HtmlElement`](html::HtmlElement) (tag, id, classes).
//! - **CSS** ([`style`]): type, `.class`, `#id` and compound selectors with
//!   specificity, descendant and child combinators, plus the interaction
//!   pseudo-classes `:hover`/`:active`/`:focus`/`:focus-visible` (state
//!   comes from picking and input focus; a change restyles in place);
//!   `color`, `font-family` (per-glyph fallback chains over registered
//!   faces), `font-size` (including `em`/`rem`), `font-weight`,
//!   `font-style`, `pointer-events` (inherited); `border-image` (9-slice),
//!   `border-width`, `padding`, `background-color`, `gap`, flex and grid
//!   layout (including named lines and areas), sizes, margins, `box-sizing`,
//!   `calc()`, `min()`/`max()`/`clamp()` (mixed-unit, re-resolved per
//!   frame), `var()`, `transition`
//!   (color changes), `position` with insets, `z-index`, `border-radius`,
//!   `border-color`, `outline`, `text-decoration` and `opacity` (fades the
//!   subtree) on blocks, containers and
//!   the `HtmlUi` node itself (the `html` rule, or the template's own
//!   `<html class="…">`; what it stops declaring goes back to the app's
//!   values).
//! - **Fluent** ([`l10n`]): `data-l10n-id` / `data-l10n-args` /
//!   `data-l10n-name` (fluent-dom convention) on any element; translations
//!   may contain inline markup.
//! - **Tera** ([`template`](mod@template)): full Tera 2 syntax in `.html` files, rendered with
//!   the entity's [`TemplateContext`](html::TemplateContext). Templates
//!   compose: `{% extends %}` and `{% include %}` take paths relative to the
//!   template's file, and an included file's `{% component %}`s are usable
//!   in the whole template (`{{ <ui.button … /> }}`) — include a component
//!   library to share widgets.
//! - **Markdown** ([`template`](mod@template)): a `.md`/`.markdown` template is
//!   the same Tera machinery; its Markdown converts to HTML once, at asset
//!   load (CommonMark + tables, strikethrough, task lists, footnotes),
//!   with Tera syntax passing through untouched — headings, emphasis,
//!   lists and `![images](…)` come out as ordinary elements carrying
//!   `class="markdown"` (so documents style their Markdown content apart
//!   from raw-HTML `<img>` tags), and the per-frame path is Tera-only.
//!   Conversion is per file: an `.html` partial included from a Markdown
//!   page is never converted, a `.md` partial included from an HTML page
//!   is. Raw HTML passes through, so a Markdown page can embed real UI
//!   (`data-on-click` buttons, containers). Indent Tera blocks and HTML
//!   containers freely: a block's body (`{% if %}`, `{% for %}`, … down to
//!   its `{% end… %}`, or an own-line `<div …>` down to its `</div>`)
//!   loses the indentation its lines share before conversion, so an
//!   indented body is Markdown, not an indented code block; indentation
//!   *within* the body (a nested list, code) keeps its meaning, and
//!   `<pre>`/`<textarea>`/`<script>`/`<style>` keep theirs. (Markdown
//!   inside an HTML element still needs blank lines around it, as in
//!   CommonMark.) Fluent bundles opt their
//!   message values into Markdown with the manifest's `markdown: bool`
//!   (default off): each message's pattern converts once at bundle load
//!   and the formatted arguments substitute into the cached HTML (the
//!   values are escaped once, when the arguments are parsed).
//! - **Interaction signals** ([`signals`]): elements declare hooks with
//!   `data-on-click`/`-auxclick`/`-press`/`-release`/`-enter`/`-leave`
//!   naming an app-side signal (`click` is the primary button, `auxclick`
//!   the others, as in browsers), and `data-with` carries JSON data rendered
//!   with the template's context. Each signal carries its
//!   [`SignalSource`](signals::SignalSource): pointer and button (mouse,
//!   touch, custom pointers such as VR lasers) with its position, the
//!   hovering pointer, or the input that activated the focused element. Interactions arrive as one buffered
//!   [`ElementSignal`](signals::ElementSignal) message. Route them by name
//!   to systems with
//!   [`on_html_click`](signals::HtmlSignalsExt::on_html_click) /
//!   [`on_html_signal`](signals::HtmlSignalsExt::on_html_signal), or drain
//!   the message with `MessageReader`:
//!
//!   ```no_run
//!   # use bevy::prelude::*;
//!   # use bevy_markup::prelude::*;
//!   fn buy(mut signals: MessageReader<ElementSignal>) {
//!       for signal in signals.read() {
//!           if signal.name == "buy" {
//!               info!("buying {:?}", signal.payload["item"]);
//!           }
//!       }
//!   }
//!   ```
//!
//!   Nested hooks: the deepest bound element under the pointer wins.
//! - **Text inputs and forms** ([`signals`], `text-input` feature):
//!   `<input type="text">` (a missing `type` counts as `text`; other types
//!   don't become fields) renders Bevy's editable-text widget (its
//!   `value`/`name`/`maxlength` attributes carry over — `maxlength` is the
//!   widget's `max_characters`, ADR 0023; it is
//!   focusable, and the app binds keyboard input — the widget edits the
//!   focused field's text). `<form data-on-submit="name">` collects its
//!   named inputs' values: clicking the form's `<button type="submit">` —
//!   or activating it while focused (Enter / gamepad A) —
//!   emits the form's `Submit` signal with a JSON object of
//!   `{name: value}` (unnamed inputs excluded; the submit needs the
//!   `text-input` feature's input entities — without it the payload is
//!   empty). `<input>`s elsewhere are walked through.
//! - **Custom elements** ([`custom_elements`]): `is="<name>"` on a block or
//!   container runs the system the app defined with
//!   [`define_html_element`](custom_elements::HtmlCustomElementsExt::define_html_element)
//!   each time the element is spawned, with its `data-*` attributes — app
//!   components (materials, images, markers) declared in the template.
//! - **Anchored overlays** ([`anchor`]): [`HtmlAnchor`](anchor::HtmlAnchor)
//!   keeps a tooltip or popover root beside an element (right, left, above,
//!   below), inside the viewport, on the element's UI camera, and despawns
//!   it with the element; [`HtmlWorldAnchor`](anchor::HtmlWorldAnchor)
//!   keeps one over a 3D entity (nameplates, markers).
//! - **Tooltips** ([`tooltips`]): `data-tooltip="key"` (plus optional
//!   `data-tooltip-args` / `data-tooltip-placement`) shows the app's
//!   [`HtmlTooltips`](tooltips::HtmlTooltips) template beside the hovered
//!   element, like a browser's `title`.
//! - **Focus and navigation** ([`focus`]): `data-on-click`, `<button>`,
//!   text `<input>` and `tabindex="0"` elements are focusable
//!   (`tabindex="-1"` opts out), `autofocus` takes the initial focus, focus
//!   survives rebuilds by `id`, `:focus` / `:focus-visible` and `outline`
//!   style it, Tab order follows `tabindex` as in HTML, and an
//!   [`HtmlModal`](focus::HtmlModal) root confines it. Focus is Bevy's
//!   `InputFocus`; bind your own gamepad/keyboard input and call
//!   [`HtmlFocus::navigate`](focus::HtmlFocus::navigate) /
//!   [`tab`](focus::HtmlFocus::tab) /
//!   [`activate`](focus::HtmlFocus::activate) with the input you saw
//!   (activation emits the same `ElementSignal` as a click, its source that
//!   input — a key, a gamepad button, or `Synthetic` for harnesses).
//! - **9-slice frames**: in CSS via `border-image` (see [`style`]), or for
//!   nodes outside HTML via `*.slice.ron` assets and
//!   [`NineSliceFrame`](nine_slice::NineSliceFrame) ([`nine_slice`]).
//!
//! ## Cargo features
//!
//! - `system_fonts`: fall back to installed system fonts per script (e.g. CJK)
//!   when the chosen font lacks glyphs.
//! - `text-input`: `<input type="text">` maps to Bevy's editable-text widget
//!   ([`EditableText`](bevy::text::EditableText) + `TextInput`); the widget
//!   crate isn't part of the crate's minimal `ui_api` bevy features, so it's
//!   opt-in.
//! - `hot-reload`: Bevy's asset file watcher, for editing an asset file and
//!   restyling without a restart (the library already reacts to asset events).
//! - `fuzzing`: exposes `bevy_markup::fuzz` (`#[doc(hidden)]`, not covered by
//!   semver): internal glue for the fuzz targets in `fuzz/`.
//! - `dev-tools`: QA only — serves the examples over localhost BRP + MCP so
//!   agents can playtest them. Never enabled in a shipped build.
//! - `hotpath`, `hotpath-alloc`, `hotpath-prometheus`, `hotpath-cloud`:
//!   [hotpath](https://hotpath.rs) profiling of the pipeline's systems
//!   (render, localize, build, cascade lookups, signals, …). Enable `hotpath`
//!   on `bevy_markup` *and* put `#[hotpath::main]` on your app's `main` to
//!   get a report on exit; `hotpath-alloc` adds per-function allocations.
//!   Without the features the instrumentation compiles to nothing.

use bevy::asset::UnapprovedPathMode;
use bevy::prelude::*;
use bevy::ui::UiSystems;

#[cfg(feature = "fuzzing")]
#[cfg_attr(docsrs, doc(cfg(feature = "fuzzing")))]
#[doc(hidden)]
pub mod fuzz;

pub mod anchor;
mod build;
#[doc(hidden)]
pub use build::{Skeleton, WindowKey, WindowModel, WindowSpacer};
mod cascade;
pub mod custom_elements;
pub mod focus;
pub mod fonts;
pub mod html;
pub mod l10n;
pub(crate) mod md;
pub mod nine_slice;
mod rebuild;
pub mod signals;
pub mod style;
pub mod template;
pub mod tooltips;

/// Dependencies whose types appear in this crate's API.
pub use {fluent, lightningcss, tera, tl};

/// Everything needed to build HTML UIs: `use bevy_markup::prelude::*;`.
pub mod prelude {
    pub use crate::anchor::{
        AnchorPlacement, AnchoredSide, HtmlAnchor, HtmlWorldAnchor, HtmlWorldAnchorView,
    };
    pub use crate::custom_elements::{ElementConnected, HtmlCustomElementsExt};
    pub use crate::focus::{
        ActivateElement, FocusEdge, Focusable, HtmlFocus, HtmlModal, HtmlNoFocus, NavAction,
    };
    pub use crate::fonts::{FontFaces, FontFamilies, GenericFamily};
    pub use crate::html::{
        HtmlDebugOutline, HtmlElement, HtmlElements, HtmlUi, HtmlUiBuilt, HtmlUiRestyled,
        HtmlWindowed, RenderedHtml, TemplateContext,
    };
    pub use crate::l10n::{ActiveLocale, LocaleBundle};
    pub use crate::nine_slice::{NineSlice, NineSliceFrame};
    pub use crate::signals::{
        ActivationInput, ElementSignal, ElementSignals, HtmlSignalsExt, PseudoState, SignalBinding,
        SignalSource, SignalTrigger,
    };
    pub use crate::style::{DefaultStylesheet, HtmlStylesheet, Stylesheet};
    pub use crate::template::HtmlTemplate;
    pub use crate::tooltips::{HtmlTooltip, HtmlTooltips};
    pub use crate::{BevyMarkupPlugin, HtmlUiSystems};
}

/// Adds the asset loaders (`.html`/`.md`, `.css`, `*.slice.ron`, `*.ftl.ron`), the
/// [`DefaultStylesheet`](style::DefaultStylesheet),
/// [`ActiveLocale`](l10n::ActiveLocale) and
/// [`FontFamilies`](fonts::FontFamilies) resources, and the systems that turn
/// [`HtmlUi`](html::HtmlUi) entities into Bevy UI.
#[derive(Default)]
pub struct BevyMarkupPlugin;

/// Pipeline stages, in `PostUpdate` before Bevy UI layout (chained in this
/// order). Order your systems against these to see a stage's output the same
/// frame.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HtmlUiSystems {
    /// Tera renders templates and `tl` parses them into [`html::RenderedHtml`].
    Render,
    /// Fluent resolves `data-l10n-id` elements against [`l10n::ActiveLocale`].
    Localize,
    /// The DOM is styled with CSS and spawned as Bevy UI children (then
    /// [`html::HtmlUiBuilt`] fires), or existing children are restyled in
    /// place ([`html::HtmlUiRestyled`]).
    Build,
}

impl Plugin for BevyMarkupPlugin {
    fn build(&self, app: &mut App) {
        // The loaders read includes and Fluent resources themselves
        // (`read_asset_bytes`, which skips Bevy's unapproved-path check):
        // they enforce the app's mode (bug_0087). `AssetPlugin` is added
        // before any plugin that registers loaders; without it, Bevy's
        // default (`Forbid`).
        let allow_unapproved = app
            .get_added_plugins::<AssetPlugin>()
            .first()
            .is_some_and(|assets| matches!(assets.unapproved_path_mode, UnapprovedPathMode::Allow));
        app.init_asset::<template::HtmlTemplate>()
            .register_asset_loader(template::HtmlTemplateLoader { allow_unapproved })
            .register_asset_loader(template::MarkdownTemplateLoader { allow_unapproved })
            .init_asset::<style::Stylesheet>()
            .init_asset_loader::<style::StylesheetLoader>()
            .init_asset::<nine_slice::NineSlice>()
            .init_asset_loader::<nine_slice::NineSliceLoader>()
            .init_asset::<l10n::LocaleBundle>()
            .register_asset_loader(l10n::LocaleBundleLoader { allow_unapproved })
            .init_resource::<style::DefaultStylesheet>()
            .init_resource::<l10n::ActiveLocale>()
            // CSS `rem` resolves against the root font-size: the build syncs
            // this resource to the root rule's font-size. Registered here so
            // minimal apps (no `TextPlugin`) have it too.
            .init_resource::<RemSize>()
            .init_resource::<fonts::FontFamilies>()
            .init_resource::<custom_elements::CustomElements>()
            // `HtmlFocus`'s navigator reads these; they are normally
            // registered by the navigation plugin in `finish` — an app
            // driving `update()` directly (headless apps, integration
            // tests) never calls it, and the systems would panic with
            // "Resource does not exist". `init_resource` keeps an
            // existing value, so registering them again in `finish`
            // changes nothing.
            .init_resource::<bevy::input_focus::directional_navigation::DirectionalNavigationMap>()
            .init_resource::<bevy::input_focus::directional_navigation::AutoNavigationConfig>();
        #[cfg(feature = "text-input")]
        {
            // The widget's keyboard editing keys off Bevy's
            // `InputFocus`: the focus plugin (already the crate's
            // model) plus the dispatcher for `FocusedInput` events.
            if !app.is_plugin_added::<bevy::input_focus::InputFocusPlugin>() {
                app.add_plugins(bevy::input_focus::InputFocusPlugin);
            }
            if !app.is_plugin_added::<bevy::input_focus::InputDispatchPlugin>() {
                app.add_plugins(bevy::input_focus::InputDispatchPlugin);
            }
            if !app.is_plugin_added::<bevy::ui_widgets::TextInputPlugin>() {
                app.add_plugins(bevy::ui_widgets::TextInputPlugin);
            }
            // The widget's systems read these; a real app has them via
            // `WindowPlugin`/`UiScale`/picking, but headless apps don't —
            // without `PointerState` the autoscroll system failed
            // validation on the first update (review_0003 G11).
            app.init_resource::<bevy::picking::events::PointerState>()
                .add_message::<bevy::input::keyboard::KeyboardInput>()
                .add_message::<bevy::input::gamepad::GamepadButtonChangedEvent>()
                .add_message::<bevy::input::mouse::MouseWheel>()
                .add_message::<bevy::window::Ime>()
                .add_message::<bevy::picking::events::PointerRelease>()
                .add_message::<bevy::picking::events::PointerPress>()
                .add_message::<bevy::picking::events::PointerMove>()
                .init_resource::<bevy::ui::UiScale>();
        }
        app.add_message::<signals::ElementSignal>()
            .add_systems(
                Update,
                (
                    signals::hover_signals,
                    signals::update_pseudo_states,
                    tooltips::show_tooltips,
                ),
            )
            .configure_sets(
                PostUpdate,
                (
                    HtmlUiSystems::Render,
                    HtmlUiSystems::Localize,
                    HtmlUiSystems::Build,
                )
                    .chain()
                    .before(UiSystems::Prepare),
            )
            .add_systems(
                PostUpdate,
                (
                    html::render_templates.in_set(HtmlUiSystems::Render),
                    l10n::localize.in_set(HtmlUiSystems::Localize),
                    build::build_html_ui.in_set(HtmlUiSystems::Build),
                    nine_slice::apply_nine_slices.before(UiSystems::Prepare),
                    signals::dispatch_signals.before(HtmlUiSystems::Render),
                    anchor::place_world_anchored.before(UiSystems::Prepare),
                    // Right after Bevy's layout (like its `Popover`): this
                    // frame's rects, corrected before clipping and render.
                    anchor::place_anchored
                        .in_set(UiSystems::Layout)
                        .after(bevy::ui::ui_layout_system),
                    build::size_images
                        .after(HtmlUiSystems::Build)
                        .before(UiSystems::Prepare),
                    build::resolve_calc
                        .after(HtmlUiSystems::Build)
                        .before(UiSystems::Prepare),
                    build::advance_transitions
                        .after(HtmlUiSystems::Build)
                        .before(UiSystems::Prepare),
                    // ADR 0027: `overflow: auto` promotes after layout
                    // (the laid-out `ComputedNode` decides; the write
                    // re-layouts the next frame, like the calc resolver's
                    // one-frame model).
                    build::promote_auto_scroll.in_set(UiSystems::PostLayout),
                    // ADR 0031: record the materialized heights after
                    // layout (the model the windowed builds cull by).
                    build::measure_window_heights
                        .in_set(UiSystems::PostLayout)
                        .after(HtmlUiSystems::Build),
                ),
            );
        focus::plugin(app);
    }

    fn finish(&self, app: &mut App) {
        // In `finish`, after every plugin's `build`: an app that adds
        // Bevy's `DirectionalNavigationPlugin` itself (anywhere in its own
        // build) is detected here, so the auto-add doesn't hit Bevy's
        // duplicate-plugin error.
        focus::add_navigation_plugin(app);
    }
}
