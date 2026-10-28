//! `.css` stylesheets and how they're assigned.
//!
//! Every [`HtmlUi`](crate::html::HtmlUi) uses its own [`HtmlStylesheet`] if it
//! has one, else the [`DefaultStylesheet`] resource, else no styles (white
//! text, Bevy's default font, 16px). Swapping either handle restyles at
//! runtime (themes); the UI isn't built while its stylesheet is still loading,
//! so it never flashes unstyled.
//!
//! Markdown (ADR 0014) is a *source syntax*, styled by the same CSS: a
//! `.md`/`.markdown` template or a `markdown: true` Fluent bundle converts
//! to HTML first (`# …` → `h1`, `*x*` → `em`, `**x**` → `strong`, `- x` →
//! `li`, `![x](p)` → `img.markdown`, tables → `table`/`tr`/`td`), so
//! rules target the
//! generated tags exactly like hand-written HTML.
//!
//! Supported CSS (parsed with lightningcss; a syntax error fails the load):
//! - selectors: compound selectors — a type (or `*`) plus any `.class` /
//!   `#id` parts: `p`, `.note`, `#title`, `p.note`, `.a.b`, `h1#x.big` — joined
//!   by descendant (a space) or child (`>`) combinators: `.panel p`,
//!   `.panel > p`, `.a .b > .c` (ancestors match by their tag/class/id and
//!   their own pseudo-state; specificity sums over the compounds) — incl.
//!   comma lists, plus the pseudo-classes `:hover`, `:active`, `:focus` and
//!   `:focus-visible` (see [`crate::focus`]) (each adds class-level
//!   specificity; the rule applies while the element's
//!   [`PseudoState`](crate::signals::PseudoState) has the bit set, which the
//!   library maintains from picking — a change restyles in place). Matched
//!   against each element's tag, `id` and `class` attributes, and against
//!   the ancestor chain: descendant (`A B`) and child (`A > B`)
//!   combinators, with the ancestors' own pseudo-classes. Also (ADR 0028):
//!   attribute selectors on `data-*` attributes (`[data-x]`,
//!   `[data-x="v"]`; other attributes and operators skip the rule), the
//!   sibling combinators `+` and `~` next to the subject (`.a + p`, not
//!   `.a + .b p`), and the structural pseudo-classes `:first-child`,
//!   `:last-child`, `:nth-child()`, `:first-of-type`, `:last-of-type`,
//!   `:nth-of-type()`, `:empty`, `:focus-within` and `:not(compound)` —
//!   `:not(:hover)` included. Every compound matches whole wherever it
//!   stands: an ancestor's or sibling's compound tests its classes,
//!   `data-*` attributes, pseudo-state and position like the subject's
//!   (`li:first-child > span`, `[data-mode="dark"] p`, `.tab:hover + .panel`).
//!   Structural pseudo-classes and `:not()` add their specificity as CSS
//!   does. Other pseudo-classes and pseudo-elements skip the rule (logged
//!   at `debug`).
//! - inherited: `color`, `font-family`, `font-size`, `font-weight`,
//!   `font-style`, `pointer-events` (`none` → `Pickable::IGNORE` on the
//!   element and, by inheritance, its descendants; `auto` turns it back on)
//! - box properties on blocks, containers and the root rule (the `HtmlUi`
//!   node itself, see below): `border-image` (+ `-source`, `-slice`, `-repeat`),
//!   `border-width`, `padding` (absolute lengths); `background-color` on
//!   blocks and containers; `gap` / `row-gap` / `column-gap` on containers
//! - on blocks and containers: `outline` (+ `-style`, `-width`, `-color`;
//!   `outline-offset` in px; drawn only with a visible style, `currentColor`
//!   = the text color) → Bevy's `Outline`; `border-color` (+ `-top`/… sides; Bevy's
//!   `BorderColor`, undeclared sides transparent — needs a `border-width`),
//!   `border-radius` (+ the four corner longhands; the CSS `h / v` slash
//!   syntax gives elliptical corners, `%` per-axis against the node's
//!   dimensions), `z-index` (an integer →
//!   `ZIndex` among siblings; `auto` = 0). An app's own `BorderColor` /
//!   `ZIndex` / `Pickable` is left alone unless a rule sets it.
//! - inline elements: `background-color` paints the run (→ Bevy's
//!   `TextBackgroundColor`, no box); `text-decoration` (+ `-line` with
//!   `underline`/`line-through`, `-color`; `overline`/`blink` and
//!   `-thickness`/`-style` skipped) → `Underline`/`Strikethrough`; the
//!   color defaults to the text's (`currentColor`). Other box properties on
//!   inline elements stay ignored.
//! - `<img src="…">`: an image flowing inline with the text
//!   (`InlineBox`/`InlineImage`; the CSS `background`/`border` boxes don't
//!   apply to it). `src` is an asset path relative to the template (like
//!   `{% include %}`; `/` from the asset root; URLs and data URIs
//!   unsupported); the CSS `width`/`height` size it (px, `%` of the
//!   containing block's content box — resolved at layout time; other units
//!   skipped), one unset dimension keeps the aspect ratio; without a
//!   declaration the image keeps its intrinsic size, and its load restyles
//!   the UI (a hot reload re-sizes it).
//! - `opacity` (a number or `%`): CSS group opacity, approximated by
//!   multiplying the alpha of every color in the element's subtree (text,
//!   backgrounds, borders, outlines, `border-image` tint) by the product of
//!   the element's and its ancestors' opacities. App-attached images aren't
//!   faded.
//! - layout on blocks, containers and the root rule: `display` (`none`, `block`, `flex`,
//!   `grid`), `flex-direction`, `flex-wrap`, `flex-flow`, `justify-content`,
//!   `align-items`, `align-content`, `align-self`, `justify-items`,
//!   `justify-self`, `flex-grow`, `flex-shrink`, `flex-basis`, `flex`;
//!   `width`, `height`, `min-*`, `max-*` (px, `%`, `vw`/`vh`/`vmin`/`vmax`,
//!   `em`/`rem`, `auto`/`none`); `margin` (+ sides; px, `%`, `em`/`rem`,
//!   `auto`); `box-sizing`;
//!   `overflow` (+ `-x`/`-y`: `visible`, `hidden`, `clip`, `scroll`; `auto`
//!   scrolls — Bevy has no scroll-if-needed). `visibility` (`hidden`
//!   subtree-hides via Bevy's propagating `Visibility`: the element and its
//!   descendants are invisible but keep their layout space; the root rule's
//!   visibility is left to the app, since `HtmlWorldAnchor` owns it;
//!   `collapse` is skipped). As
//!   in CSS, sizes default to the content box. `position` (`static`,
//!   `relative`, `absolute`, `fixed`; `absolute` is placed in its parent's
//!   padding box, Bevy's rule, i.e. as if every parent were positioned;
//!   `fixed` positions against the viewport — Bevy's `FixedNode` — and does
//!   not inherit the parent's clipping or transform) with `top` /
//!   `right` / `bottom` / `left` / `inset` (px, `%`, viewport units,
//!   `em`/`rem`, `auto`),
//!   which apply only to `relative`/`absolute`/`fixed`, as in CSS;
//!   `sticky` is skipped. Font-relative lengths resolve like CSS: `em`
//!   against the element's own computed font-size (each node's `EmSize`),
//!   `rem` against the root rule's font-size (the `RemSize` resource,
//!   kept in sync; no restyle is queued — Bevy's layout re-resolves every
//!   `Val::Rem` when the resource changes, and the calc pass reads the new
//!   size on its next frame). Grid tracks take them too
//!   (taffy's font-relative track sizing). `ex`/`ch` and font-relative
//!   `padding`/`border-width`/`gap` are skipped.
//! - `var()` in any property's value: `var(--name)` resolves the UI's
//!   `TemplateContext` first (a value inserted there under `--name`;
//!   strings interpolate as written, numbers and bools as their CSS
//!   spelling), then the stylesheet's own `--name:` declarations (any
//!   top-level rule; last wins), then the reference's fallback (whose own
//!   `var()`s resolve). An unresolvable reference without a fallback drops
//!   the declaration (CSS makes it invalid at computed-value time; here
//!   earlier rules' values can show through). Deviations from CSS: the
//!   sheet's custom properties are sheet-level, not per-element inherited
//!   (declare them on `:root` for browser-compatible sheets), and a
//!   context value is opaque data (its own `var()`s don't resolve).
//! - `transition` (the shorthand): animates a property's change on a
//!   restyle (a `:hover`/`:active`/`:focus` state, a new stylesheet, …).
//!   Animatable: `color` (the element's text color — the runs that shared
//!   its old value fade with it; a span that respawns doesn't),
//!   `background-color` and `border-color` (all four sides together).
//!   Durations and delays take seconds or milliseconds; the easing is
//!   `linear`, the named curves (`ease`, `ease-in`, `ease-out`,
//!   `ease-in-out`), `cubic-bezier(a, b, c, d)` or `steps(n, position)`.
//!   The longhands (`transition-property` …) are skipped. Deviations from
//!   CSS: a spawn never animates (no previous style); transitions run on
//!   `Time`'s virtual clock (the UI pauses with the game); colors
//!   interpolate per-channel in sRGB, not premultiplied.
//!
//!   `calc()` works in these size/margin/inset properties: expressions
//!   that resolve to a single unit (all px, all `%`, …) are computed at
//!   style time; mixed-unit ones (`50% + 10px`, `min(2em, 10%)`,
//!   `clamp()`) are resolved per frame — `%` against the containing
//!   block's content box per axis (width for sizes, margins and the
//!   horizontal insets; height for the vertical insets), `em`/`rem`
//!   against the font sizes, viewport units against the UI root's size.
//!   Other math functions (`round()`, `mod()`, …) are skipped.
//!   Containers are flex columns unless `flex-direction` (or `display:
//!   block`) says otherwise — `display: flex` alone keeps the column — and
//!   neither containers nor blocks shrink by default (`flex-shrink: 0`).
//!   Font-relative lengths (`em`, `rem`) work in sizes, margins and
//!   insets (`em` against the node's own cascade font-size, `rem` against
//!   the root rule's); `calc()` too (single-unit expressions resolve at
//!   style time, mixed units at layout).
//! - grid: `grid-template-rows`/`-columns` (`none` or track lists: lengths,
//!   `%`, viewport units, `fr`, `auto`, `min-content`, `max-content`,
//!   `minmax()`, `fit-content()`, `repeat()` with a count, `auto-fill` or
//!   `auto-fit`), `grid-template`, `grid-auto-rows`/`-columns`,
//!   `grid-auto-flow`, `grid`; placement by line number, by line name or
//!   area name (resolved against the parent grid's template at collection;
//!   names in `auto-fill`/`auto-fit` repeats and occurrences past the
//!   template don't resolve — they place `auto`), and `span`:
//!   `grid-row`/`-column` (+ `-start`/`-end`), `grid-area`;
//!   `grid-template-areas` defines the named areas and implies the
//!   explicit tracks it spans (sized `auto`). Bare `grid-auto-flow: dense`
//!   doesn't parse (lightningcss); write `row dense`.
//! - cascade: `!important` beats normal declarations, then higher specificity
//!   (ids, classes, type) wins, then the later rule. `initial`/`inherit`/
//!   `unset`/`revert` clear the field back to the initial value — one
//!   documented deviation: on an *inherited* property (`color`, `font-*`)
//!   all four behave like `inherit` (the cascade keeps no "explicitly
//!   initial" marker, and the build's field default fills from the
//!   parent's computed value); on everything else they reset to the
//!   initial value. A keyword also drops the property's `calc()` entry
//!   and resets only its own `border-radius` corner.
//! - the `style` attribute (any element, the `<html>` root included): its
//!   declarations beat every normal rule and lose to `!important` rules
//!   unless `!important` themselves, as in CSS. Rendered by Tera like any
//!   attribute, so templated values (`style="width: {{ hp }}%"`) update the
//!   element in place.
//! - the root rule styles the `HtmlUi` node itself: the `html` rule, plus
//!   the document's own top-level `<html id="…" class="…">` if it has one
//!   (so each template can style its root: `<html class="hud">` with
//!   `.hud { position: absolute; top: 0; right: 0 }`). It takes everything a
//!   container does — layout, sizes, margins, `position` and insets,
//!   `border-radius`, box properties, `gap`, `background-color`,
//!   `border-color`, `z-index` (`ZIndex`; Bevy orders root nodes by
//!   `GlobalZIndex`, then `ZIndex`), `pointer-events: none` — and its
//!   inherited properties start the cascade, also for fragments. Only what
//!   it declares: the app's other `Node` fields and components stay, and
//!   whatever a later stylesheet or document stops declaring goes back to
//!   the app's value.
//!
//! `font-family` resolves every registered name in the list, in order,
//! against [`FontFamilies`](crate::fonts::FontFamilies), and composes
//! Bevy's per-glyph fallback list over them (ADR 0015); generic keywords
//! map via
//! [`FontFamilies::set_generic`](crate::fonts::FontFamilies::set_generic). A
//! list with no registered name means Bevy's default font (like a browser's
//! default font), not the inherited family. `font-size`:
//! `px`, `em`/`%` (of the inherited size), `rem` (of the root size), keywords
//! (`medium` = 16px), `smaller`/`larger`. `font-weight`: bold above 500 (CSS
//! font matching with regular/bold faces).
//! `font-style`: `italic`/`oblique` vs `normal`.
//!
//! ## 9-slice frames with `border-image`
//!
//! ```css
//! html {
//!   border-image: url("frame.png") 16 fill / 16px stretch;
//!   border-width: 16px;
//!   padding: 12px 20px;
//! }
//! ```
//!
//! The image (relative to the `.css` file; its format must be enabled in the
//! app's Bevy features) is drawn 9-sliced over the node's border box:
//! `border-image-slice` gives the insets in image pixels (or `%` of the image
//! size), `border-image-repeat: stretch` stretches sides and center,
//! `repeat`/`round`/`space` tile them. `border-width` and `padding` become the
//! node's `border`/`padding`, insetting the content. Bevy differences: the
//! center is always drawn (`fill` or not), corners keep their image size
//! (`border-image-width`/`-outset` are ignored), and one repeat mode applies to
//! all sides. Box properties from the root rule (see above) are applied to
//! the `HtmlUi` node and restored when a later stylesheet drops them.

use bevy::platform::collections::HashMap;

use bevy::asset::{AssetLoader, LoadContext, io::Reader};
use bevy::prelude::*;
use lightningcss::stylesheet::{ParserOptions, StyleSheet};
use lightningcss::traits::IntoOwned;

/// A parsed `.css` file, plus the images its `border-image-source: url(...)`
/// declarations reference (resolved relative to the `.css` file, loaded as
/// dependencies).
#[derive(Asset, TypePath)]
pub struct Stylesheet {
    pub(crate) sheet: StyleSheet<'static>,
    /// The `var()` names the sheet (transitively, through its own `--`
    /// declarations) reads from the context — computed once at load.
    pub(crate) var_names: Vec<String>,
    /// Every loadable image URL the sheet refers to (its `border-image`
    /// urls and the file ones in its `--` declarations) with the loaded
    /// handle. A map, not parallel vectors: skipped URLs (schemed ones
    /// like `data:`, unresolvable paths) used to shift the indices and
    /// pair the wrong handles (review_0003 G09).
    #[dependency]
    pub(crate) images: HashMap<String, Handle<Image>>,
}

impl Stylesheet {
    /// The lightningcss stylesheet tree.
    pub fn sheet(&self) -> &StyleSheet<'static> {
        &self.sheet
    }

    /// The image a `url(...)` in this sheet refers to (`None`: the URL
    /// was skipped at load — a scheme or an unresolvable path).
    pub(crate) fn image(&self, url: &str) -> Option<&Handle<Image>> {
        self.images.get(url)
    }

    pub(crate) fn images(&self) -> impl Iterator<Item = &Handle<Image>> {
        self.images.values()
    }
}

/// Stylesheet for every `HtmlUi` without an [`HtmlStylesheet`]. Swap the
/// handle to switch themes.
#[derive(Resource, Default, Clone, Debug, Reflect)]
#[reflect(Resource, Default)]
pub struct DefaultStylesheet(pub Option<Handle<Stylesheet>>);

impl DefaultStylesheet {
    pub fn new(stylesheet: Handle<Stylesheet>) -> Self {
        Self(Some(stylesheet))
    }
}

/// Per-entity stylesheet, overriding [`DefaultStylesheet`] for this `HtmlUi`.
#[derive(Component, Clone, Debug, Reflect)]
#[reflect(Component)]
pub struct HtmlStylesheet(pub Handle<Stylesheet>);

#[derive(Default, TypePath)]
pub(crate) struct StylesheetLoader;

impl AssetLoader for StylesheetLoader {
    type Asset = Stylesheet;
    type Settings = ();
    type Error = BevyError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        load_context: &mut LoadContext<'_>,
    ) -> Result<Stylesheet, BevyError> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let source = String::from_utf8(bytes)?;

        let options = ParserOptions {
            filename: load_context.path().to_string(),
            ..ParserOptions::default()
        };
        // The parse error borrows `source`; render it before `source` drops.
        // lightningcss's parser recurses per parenthesis with no bound:
        // reject pathological nesting before it (review_0003 G08). The
        // sheet fails its load, like unparseable CSS.
        if crate::cascade::nesting_over_limit(&source) {
            return Err("stylesheet nests too deeply".into());
        }
        let sheet = StyleSheet::parse(&source, options).map_err(|err| err.to_string())?;
        // Border-image URLs: a dependency of the sheet (an unresolvable
        // one fails the load, as before). Custom-property URLs: a
        // `--icon: url(...)` can be anything (a data URI, a scheme) —
        // those are skipped, not the whole sheet.
        let border_urls = crate::cascade::image_urls(&sheet);
        let custom_urls = crate::cascade::custom_image_urls(&sheet);
        let mut images = HashMap::with_capacity(border_urls.len() + custom_urls.len());
        for url in &border_urls {
            let path = load_context.path().resolve_embed_str(url)?;
            // A remote image is skipped like a custom property's schemed
            // URL: the frame isn't drawn, the rest of the sheet applies.
            if crate::template::is_remote(&path) {
                warn!(
                    "stylesheet {}: remote border-image skipped: {url}",
                    load_context.path()
                );
                continue;
            }
            images.insert(url.clone(), load_context.load(path));
        }
        for url in &custom_urls {
            // Anything with a scheme (`data:`, `https:` — Bevy's asset
            // paths never contain `:`) isn't a file in the asset root;
            // the `var()` consumer drops the declaration at substitution
            // time, like any unresolvable reference.
            if url.contains(':') {
                continue;
            }
            let Ok(path) = load_context.path().resolve_embed_str(url) else {
                continue;
            };
            images.insert(url.clone(), load_context.load(path));
        }
        Ok(Stylesheet {
            var_names: crate::cascade::var_names(&sheet),
            sheet: sheet.into_owned(),
            images,
        })
    }

    fn extensions(&self) -> &[&str] {
        &["css"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `data:` URL between two file URLs must not shift the url →
    /// handle pairing: the loader skipped unresolvable/schemed custom
    /// URLs while `image_urls` kept them, and `image()` paired by index —
    /// `--b`'s consumer got `c.png`'s handle and `--c` got none
    /// (review_0003 G09). The map keeps only the loadable URLs.
    #[test]
    fn skipped_custom_urls_do_not_shift_the_pairing() {
        use bevy::MinimalPlugins;
        use bevy::app::App;
        use bevy::asset::{AssetPlugin, AssetServer, LoadState};

        let dir = std::env::temp_dir().join(format!(
            "bevy_markup_urls_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("urls.css"),
            r#"html { border-image: url("frame.png") 4; --a: url("data:image/svg+xml,<svg/>"); --b: url("b.png"); --c: url("c.png") }"#,
        )
        .unwrap();
        // A 1x1 PNG (the nine-slice loader test's fixture).
        for name in ["frame.png", "b.png", "c.png"] {
            std::fs::write(dir.join(name), crate::nine_slice::png_1x1()).unwrap();
        }
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin {
                file_path: dir.to_string_lossy().into_owned().into(),
                ..default()
            },
        ));
        app.init_asset::<Stylesheet>();
        app.init_asset::<Image>();
        app.register_asset_loader(StylesheetLoader);
        let handle = app
            .world()
            .resource::<AssetServer>()
            .load::<Stylesheet>("urls.css");
        // The load runs on the IO task pool: poll until it resolves (a
        // fixed four frames flaked under a loaded full suite — review_0005
        // fix session), bounded so a hang still fails.
        let load_state = |app: &App| {
            app.world()
                .resource::<AssetServer>()
                .get_load_state(handle.id())
                .unwrap()
        };
        let start = std::time::Instant::now();
        while matches!(load_state(&app), LoadState::NotLoaded | LoadState::Loading)
            && start.elapsed() < std::time::Duration::from_secs(10)
        {
            app.update();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let state = load_state(&app);
        assert!(matches!(state, LoadState::Loaded), "{state:?}");

        let sheet = app
            .world()
            .resource::<Assets<Stylesheet>>()
            .get(&handle)
            .unwrap();
        let border = sheet.image("frame.png").expect("the border image");
        let b = sheet.image("b.png").expect("b.png paired");
        let c = sheet.image("c.png").expect("c.png paired");
        assert_ne!(border.id(), b.id(), "the handles must not drift");
        assert_ne!(b.id(), c.id());
        assert!(sheet.image("data:image/svg+xml,<svg/>").is_none());
        std::fs::remove_dir_all(&dir).ok();
    }
}
