//! Rendered + localized DOM → styled Bevy UI children of the `HtmlUi` entity.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use bevy::asset::{AssetEvent, AssetPath, LoadState};
use bevy::ecs::system::SystemParam;
use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;
#[cfg(feature = "text-input")]
use bevy::text::EditableText;
use bevy::text::{InlineBox, InlineBoxKind};
use bevy::text::{LetterSpacing, LineHeight};
#[cfg(feature = "text-input")]
use bevy::ui_widgets::TextInput;
use lightningcss::values::easing::{EasingFunction, StepPosition};

use crate::cascade::{
    self, Ancestor, CalcBasis, CalcExpr, CalcProp, CssPosition, DecorationDecl, ElementState,
    GridNames, HtmlStyles, InlineStyle, LayoutDecl, LetterSpacingSpec, LineHeightSpec,
    ListStyleSpec, OutlineDecl, OverflowAxisDecl, Position, Pseudo, SelectorReads, SiblingLink,
    SiblingList, SliceValue, TextAlignSpec, TextTransformSpec, TransitionDecl, TransitionProp,
    WhiteSpaceSpec, node_field_mut,
};
use crate::custom_elements::{self, ConnectedAs, CustomElement, ElementConnected};
use crate::focus::{self, Focusable};
use crate::fonts::FontFamilies;
use crate::html::{
    HtmlDebugOutline, HtmlElement, HtmlUi, HtmlUiBuilt, HtmlUiRestyled, HtmlWindowed, RenderedHtml,
    TemplateContext,
};
use crate::l10n::LocalizedText;
use crate::rebuild::{Decision, Frame, Phase, RebuildState, Source};
use crate::signals::{self, ElementSignals, PseudoState, SignalBinding, SignalTrigger};
use crate::style::{DefaultStylesheet, HtmlStylesheet, Stylesheet};
use crate::template::{HtmlDocument, HtmlTemplate, decode_attribute, decode_text};

/// Values when no stylesheet rule applies (white suits the dark UIs Bevy
/// apps usually draw on; 16px is the CSS initial size).
const DEFAULT_COLOR: Color = Color::WHITE;
const DEFAULT_FONT_SIZE: f32 = 16.0;

/// Computed (inherited) text style.
#[derive(Clone, PartialEq)]
pub(crate) struct Style {
    pub(crate) color: Color,
    /// The CSS `font-family` chain resolved to registered family indexes
    /// (ADR 0015): the text font composes the chain into Bevy 0.20's
    /// fallback list per weight/style. `None` = Bevy's default font.
    pub(crate) family: Option<Vec<usize>>,
    pub(crate) size: f32,
    pub(crate) bold: bool,
    pub(crate) italic: bool,
    /// `pointer-events: auto` (`false` = `none`); inherited, as in CSS.
    pub(crate) pointer_events: bool,
    /// The product of the element's and its ancestors' `opacity`: CSS group
    /// opacity, applied by fading every color of the subtree ([`faded`]).
    pub(crate) opacity: f32,
    /// `text-align` (ADR 0027): the block's line alignment, inherited.
    pub(crate) align: TextAlignSpec,
    /// `line-height` (ADR 0027), inherited per kind (a multiplier stays
    /// relative to each element's own size).
    pub(crate) line_height: LineHeightSpec,
    /// `letter-spacing` (ADR 0027), resolved to pixels, inherited.
    pub(crate) letter_spacing: f32,
    /// `list-style-type` (ADR 0027), inherited; `None` = nothing on the
    /// chain declared it (the tag's UA-sheet default applies at the
    /// marker). Inert outside list items.
    pub(crate) list_style: Option<ListStyleSpec>,
    /// `text-transform` (ADR 0028), inherited.
    pub(crate) text_transform: TextTransformSpec,
    /// `white-space` (ADR 0028), inherited; `None` = nothing on the chain
    /// declared it (the `pre` tag's UA default applies at the block).
    pub(crate) white_space: Option<WhiteSpaceSpec>,
}

/// `color` with its alpha scaled by `opacity`.
pub(crate) fn faded(color: Color, opacity: f32) -> Color {
    if opacity >= 1.0 {
        color
    } else {
        color.with_alpha(color.alpha() * opacity)
    }
}

/// A stretch of text in one style, or an inline `<img>` (empty `text`).
#[derive(Clone)]
struct Run {
    text: String,
    style: Style,
    /// The owning inline element's `background-color` and `text-decoration`
    /// (not inherited in CSS, but propagated down the inline subtree here;
    /// a nested element's own declaration replaces them).
    extras: RunExtras,
    /// `Some` for an `<img>`: rendered as an inline box in the block's text.
    image: Option<ImageRun>,
}

/// Run-level (non-inherited) properties of an inline element.
#[derive(Clone, Default, PartialEq)]
struct RunExtras {
    background: Option<Color>,
    decoration: DecorationDecl,
    /// The `<a>` this run belongs to: its bindings (the `open` click signal
    /// carrying the href, plus the element's own `data-on-*` hooks), its
    /// element info, and its DOM node (for `:hover` state). Inherited by
    /// the runs inside the element; `None` outside links.
    link: Option<LinkRun>,
}

impl RunExtras {
    /// Fades every color with the element's computed `opacity`.
    fn faded(self, opacity: f32) -> Self {
        Self {
            background: self.background.map(|color| faded(color, opacity)),
            decoration: DecorationDecl {
                color: self.decoration.color.map(|color| faded(color, opacity)),
                ..self.decoration
            },
            link: self.link,
        }
    }
}

/// A link's identity for its spans: the click bindings and the element.
#[derive(Clone, PartialEq)]
struct LinkRun {
    bindings: Vec<SignalBinding>,
    element: HtmlElement,
    handle: tl::NodeHandle,
}

/// An editable text field (ADR 0016): the initial text plus the cascade's
/// text styling (the widget's `EditableText` requires `TextFont`/`TextColor`,
/// whose cascade values the field's text styles with).
#[cfg(feature = "text-input")]
#[derive(Clone)]
struct TextInputSpec {
    initial: String,
    font: TextFont,
    color: Color,
    /// The form identity: submissions collect the value under this name.
    name: Option<String>,
    /// The `maxlength` attribute: the widget ignores edits past it (its
    /// own `set_text` still writes longer text — the app's value wins).
    max_characters: Option<usize>,
}

/// The `value` attribute the field's text was last set from: the field's
/// text is app state (the user types into `EditableText`), so a re-render
/// sets it only when the attribute changed.
#[cfg(feature = "text-input")]
#[derive(Component, Debug, Default, Clone, PartialEq)]
struct InputValue(String);

/// An `<img>`: the loaded image and its `width`/`height` attributes (px;
/// `None` keeps the image's size, one `None` preserves the aspect ratio).
/// Percentages resolve against the containing block's content box at
/// layout time (see [`size_images`]).
#[derive(Clone)]
struct ImageRun {
    image: Handle<Image>,
    width: Option<ImageSize>,
    height: Option<ImageSize>,
}

/// An `<img>`'s `width`/`height` attribute value.
#[derive(Clone, Copy, Debug, PartialEq)]
enum ImageSize {
    /// Logical pixels.
    Px(f32),
    /// A fraction of the containing block's content box (`width="50%"`).
    Percent(f32),
}

impl ImageSize {
    fn px(self) -> Option<f32> {
        match self {
            ImageSize::Px(px) => Some(px),
            ImageSize::Percent(_) => None,
        }
    }
}

/// An `<img>` with a CSS-declared `width`/`height` (px or `%`): its inline
/// box is sized against the containing block's content box (the last
/// frame's layout) by [`size_images`]. The box spawns at 0 and is sized in
/// the same frame, before `UiSystems::Prepare`.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub(crate) struct InlineImageSized {
    /// The declared `width`, if any.
    width: Option<ImageSize>,
    /// The declared `height`, if any.
    height: Option<ImageSize>,
}
impl ImageRun {
    /// The inline box's size: the image's intrinsic size, unless a CSS
    /// `width`/`height` is declared (then [`size_images`] sizes the box at
    /// layout time). `None` also while the image is still loading (its load
    /// restyles the UI, which sizes the box).
    fn box_size(&self, images: &Assets<Image>) -> Option<Vec2> {
        let size = images.get(&self.image)?.size().as_vec2();
        match (self.width, self.height) {
            (None, None) => Some(size),
            _ => None,
        }
    }
}

/// The `<img>` images a UI's current build uses: their loads restyle the UI
/// so the inline boxes take their sizes (Bevy's own `AssetChanged`-driven
/// sizer races the asset events here, so bevy_markup sizes the boxes).
#[derive(Component, Deref, Default)]
pub(crate) struct InlineImages(pub Vec<Handle<Image>>);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum BlockKind {
    Heading,
    Paragraph,
    ListItem,
    Preformatted,
    /// `<button>`: a block carrying its bindings; `type="submit"` submits
    /// the enclosing form.
    Button,
}

#[derive(Clone)]
struct Block {
    kind: BlockKind,
    /// `<button type="submit">`: clicking submits the enclosing form.
    submit: bool,
    /// `None` for anonymous loose text.
    element: Option<HtmlElement>,
    /// The element's `data-on-*` hooks.
    signals: Vec<SignalBinding>,
    /// Focusability from `tabindex` / `data-on-click` / `autofocus`.
    focus: Option<Focusable>,
    /// The `is` definition to run on the spawned element.
    custom: Option<CustomElement>,
    /// The DOM node, for `:hover`/`:active` restyles.
    handle: Option<tl::NodeHandle>,
    /// ADR 0031: the windowing model's key (the element's or the raw
    /// text node's handle) — present even where `handle` is not
    /// (anonymous blocks carry no `DomNode`).
    key: Option<tl::NodeHandle>,
    /// The element's computed (inherited) style: the children's inherited
    /// base (ADR 0029's scoping compares it with the cached spec's).
    style: Style,
    /// ADR 0029: a pseudo-referencing selector could match this element
    /// or its content — an interaction flip on it or an ancestor may
    /// change its style.
    pseudo_reactive: bool,
    /// ADR 0027: the `li`'s marker text (its own computed
    /// `list-style-type` with the parent's counter); `None` for non-list
    /// blocks.
    marker: Option<String>,
    /// ADR 0028: the block's resolved `white-space` (the tag's UA default
    /// applied: the `pre` tag preserves and doesn't wrap).
    white_space: WhiteSpaceSpec,
    boxed: BoxStyle,
    runs: Vec<Run>,
}

/// Elements that become column nodes holding their children's nodes (so CSS
/// box properties and `HtmlElements` reach them).
const CONTAINERS: &[&str] = &[
    "div",
    "section",
    "article",
    "header",
    "footer",
    "main",
    "nav",
    "aside",
    "ul",
    "ol",
    "blockquote",
    "figure",
    "form",
    // ADR 0028: tables map onto the flex model — the table/sections are
    // columns, a row is a row, the cells are containers. Not CSS's table
    // layout (no col/row spans, no column sizing); a documented
    // approximation (the flex rows/columns).
    "table",
    "tbody",
    "thead",
    "tfoot",
    "caption",
    "tr",
    "td",
    "th",
];

/// A spec shared into the diff: built this frame (the entity tree must be
/// synced to it), or reused from the previous build (the entities already
/// match it — the diff skips them; see ADR 0021's C-2).
#[derive(Clone)]
enum SpecNode {
    Fresh(Arc<NodeSpec>),
    Cached(Arc<NodeSpec>),
}

impl SpecNode {
    fn spec(&self) -> &Arc<NodeSpec> {
        match self {
            SpecNode::Fresh(spec) | SpecNode::Cached(spec) => spec,
        }
    }
}

/// A node to spawn: a text block, or a container of further items.
#[derive(Clone)]
enum Item {
    Block(Block),
    /// ADR 0031: a culled subtree stands in as an invisible fixed height.
    Spacer {
        /// The subtree's last measured outer height (vertical margins +
        /// border box). Deliberately no model key: the coalesced run's
        /// height is a derived sum, never a member's.
        height: f32,
        /// The containing column's row gap: coalescing k culled siblings
        /// into one spacer must keep the k − 1 gaps between them (one
        /// node gets one gap in the column, not k — bug_0151: the band
        /// rose by the dropped gaps and left the viewport).
        gap: f32,
    },
    Container {
        element: HtmlElement,
        handle: Option<tl::NodeHandle>,
        signals: Vec<SignalBinding>,
        /// Focusability from `tabindex` / `data-on-click` / `autofocus`.
        focus: Option<Focusable>,
        /// The `is` definition to run on the spawned element.
        custom: Option<CustomElement>,
        /// Box properties, computed with the element's interaction state.
        boxed: BoxStyle,
        /// The element's computed `pointer-events` (inherited).
        pointer_events: bool,
        /// The element's computed font-size (for `EmSize`).
        font_size: f32,
        /// The element's computed style: the children's inherited base
        /// (ADR 0029's scoping compares it with the cached spec's).
        style: Style,
        /// ADR 0029: a pseudo-referencing selector could match this
        /// element or something inside it.
        pseudo_reactive: bool,
        /// ADR 0031: collected for a windowed build — its entity carries a
        /// `WindowKey` (only windowed UIs' entities are measured).
        keyed: bool,
        children: Vec<Item>,
    },
    /// A `<input type="text">` (the `text-input` feature).
    #[cfg(feature = "text-input")]
    TextInput {
        element: HtmlElement,
        handle: Option<tl::NodeHandle>,
        signals: Vec<SignalBinding>,
        focus: Option<Focusable>,
        custom: Option<CustomElement>,
        boxed: BoxStyle,
        pointer_events: bool,
        font_size: f32,
        /// The element's computed style: the children's inherited base
        /// (ADR 0029's scoping compares it with the cached spec's).
        style: Style,
        /// ADR 0029: a pseudo-referencing selector could match this field.
        pseudo_reactive: bool,
        /// ADR 0031: collected for a windowed build (a `WindowKey`).
        keyed: bool,
        value: TextInputSpec,
    },
    /// ADR 0021: a reused subtree — the previous build's spec, handed to
    /// the diff by refcount (its entities already match it).
    Cached(Arc<NodeSpec>),
}

/// Resolved box properties of an element: `border-width`, `padding`,
/// `background-color`, `border-image`, plus layout (flex, sizes, margins).
#[derive(Default, Clone)]
pub(crate) struct BoxStyle {
    pub(crate) border: [Option<f32>; 4],
    pub(crate) border_color: [Option<Color>; 4],
    pub(crate) padding: [Option<f32>; 4],
    pub(crate) background: Option<Color>,
    pub(crate) image: Option<(Handle<Image>, TextureSlicer)>,
    /// ADR 0028: the resolved `background-image` (the sheet's handle by
    /// URL, the tiling mode, the clip's box).
    pub(crate) background_image: Option<ImageNode>,
    /// `row-gap` for containers (not part of `is_empty`: blocks ignore it).
    pub(crate) row_gap: Option<f32>,
    /// `z-index` (`None` for `auto`/undeclared).
    pub(crate) z_index: Option<i32>,
    /// Declared `outline`; resolved against the text color by
    /// [`BoxStyle::outline`].
    pub(crate) outline: OutlineDecl,
    /// The `outline` to draw, resolved with the element's text color while
    /// collecting (`collect_node`).
    pub(crate) drawn_outline: Option<Outline>,
    /// Not part of `is_empty`: a `Text` node takes these itself.
    pub(crate) layout: LayoutDecl,
    /// The `calc()`-valued properties (the layout pass resolves them
    /// per frame; the fields keep their placeholders until then).
    pub(crate) calc: Vec<(CalcProp, CalcExpr)>,
    /// Declared `transition` (the shorthand): what [`update_spec`]'s color
    /// writes animate.
    pub(crate) transitions: Vec<TransitionDecl>,
    /// The `border-image` frame's tint alpha (`opacity`); `None` = opaque.
    pub(crate) image_alpha: Option<f32>,
}

impl BoxStyle {
    /// Applies the element's effective `opacity` to its box colors (the
    /// resolved outline included, so call after [`BoxStyle::outline`]).
    pub(crate) fn fade(&mut self, opacity: f32) {
        if opacity >= 1.0 {
            return;
        }
        if let Some(background) = &mut self.background {
            *background = faded(*background, opacity);
        }
        for color in self.border_color.iter_mut().flatten() {
            *color = faded(*color, opacity);
        }
        if let Some(outline) = &mut self.drawn_outline {
            outline.color = faded(outline.color, opacity);
        }
        self.image_alpha = Some(opacity);
    }

    fn is_empty(&self) -> bool {
        self.border.iter().all(Option::is_none)
            && self.padding.iter().all(Option::is_none)
            && self.background.is_none()
            && self.image.is_none()
            && self.background_image.is_none()
    }

    /// The node's `ImageNode`: the `border-image`'s frame when declared,
    /// else the `background-image` (ADR 0028).
    fn node_image(&self) -> Option<ImageNode> {
        self.sliced_image().or(self.background_image.clone())
    }

    fn sliced_image(&self) -> Option<ImageNode> {
        let (image, slicer) = self.image.as_ref()?;
        Some(ImageNode {
            visual_box: VisualBox::BorderBox,
            color: Color::WHITE.with_alpha(self.image_alpha.unwrap_or(1.0)),
            ..ImageNode::new(image.clone()).with_mode(NodeImageMode::Sliced(slicer.clone()))
        })
    }

    /// The `outline` to draw, if its style is a visible one. `currentColor`
    /// (and an undeclared color) is the element's text `color`.
    fn outline(&self, text_color: Color) -> Option<Outline> {
        let decl = self.outline;
        if decl.visible != Some(true) {
            return None;
        }
        let width = decl.width.unwrap_or(3.0);
        (width > 0.0).then(|| Outline {
            width: Val::Px(width),
            offset: Val::Px(decl.offset.unwrap_or(0.0)),
            color: decl.color.flatten().unwrap_or(text_color),
        })
    }

    /// The declared border colors (undeclared sides transparent), if any.
    fn border_color(&self) -> Option<BorderColor> {
        if self.border_color.iter().all(Option::is_none) {
            return None;
        }
        let side = |color: Option<Color>| color.unwrap_or(Color::NONE);
        Some(BorderColor {
            top: side(self.border_color[0]),
            right: side(self.border_color[1]),
            bottom: side(self.border_color[2]),
            left: side(self.border_color[3]),
        })
    }
}

/// An in-flight CSS transition: one entry per transitioned property whose
/// value changed on the last update, animating from the previous visual to
/// the new value. Created by [`update_spec`]'s color writes; driven by
/// [`advance_transitions`]. Empty = idle.
#[derive(Component, Clone, Default, Debug)]
pub(crate) struct Transitions(pub Vec<ActiveTransition>);

/// One property's animation: `[from, to]` colors per border side (the
/// single-color entries repeat them).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ActiveTransition {
    pub property: TransitionProp,
    pub from: [Color; 4],
    pub to: [Color; 4],
    /// `Time`'s virtual seconds when the change was styled.
    pub start: f32,
    /// Seconds; zero plays instantly.
    pub duration: f32,
    /// Seconds before the animation starts.
    pub delay: f32,
    pub easing: EasingFunction,
}

/// The hash of the context's values for the sheet's `var()` names (as
/// collected transitively at load): the seed input that invalidates
/// collection fingerprints when a variable changes, and the `Frame`'s
/// `vars_changed` signal.
fn context_vars_hash(
    sheet: Option<&Stylesheet>,
    context: Option<&TemplateContext>,
    inline_names: &[String],
) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let no_names: &[String] = &[];
    for name in sheet
        .map(|sheet| &sheet.var_names[..])
        .unwrap_or(no_names)
        .iter()
        .chain(inline_names)
    {
        name.hash(&mut hasher);
        match context.and_then(|context| context.get(name)) {
            Some(value) => hash_css_value(value, &mut hasher),
            None => 0u8.hash(&mut hasher),
        }
    }
    hasher.finish()
}

/// Hashes what `css_value` would make of `value`, without building the
/// string: this runs per tracked name, per UI, per frame (it cloned a
/// `String` each time, review_0003 G22). Distinct `css_value` results hash
/// apart; the hash only signals change.
fn hash_css_value(value: &tera::Value, hasher: &mut impl std::hash::Hasher) {
    use std::hash::Hash;
    if let Some(text) = value.as_str() {
        (1u8, text).hash(hasher);
    } else if let Some(number) = value.as_i64() {
        (2u8, number).hash(hasher);
    } else if let Some(number) = value.as_u64() {
        (3u8, number).hash(hasher);
    } else if let Some(number) = value.as_f64() {
        (4u8, number.to_bits()).hash(hasher);
    } else if let Some(flag) = value.as_bool() {
        (5u8, flag).hash(hasher);
    } else {
        // Arrays, objects, `null`: unresolvable, like a missing value.
        0u8.hash(hasher);
    }
}

/// A `TemplateContext` value as a CSS `var()` replacement: strings as
/// written, numbers and bools as their CSS spelling, anything else (arrays,
/// objects, `null`) unresolvable.
fn css_value(value: &tera::Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        return Some(text.to_owned());
    }
    if let Some(number) = value.as_i64() {
        return Some(number.to_string());
    }
    if let Some(number) = value.as_u64() {
        return Some(number.to_string());
    }
    if let Some(number) = value.as_f64() {
        return Some(format!("{number}"));
    }
    value.as_bool().map(|value| value.to_string())
}

/// `[top, right, bottom, left]` px values over `base`.
fn rect_over(base: UiRect, sides: [Option<f32>; 4]) -> UiRect {
    let side = |value: Option<f32>, base: Val| value.map_or(base, Val::Px);
    UiRect {
        top: side(sides[0], base.top),
        right: side(sides[1], base.right),
        bottom: side(sides[2], base.bottom),
        left: side(sides[3], base.left),
    }
}

/// What the root rule (the `html` rule, or the document's own `<html>`
/// element with its `id`/`class`) set on an `HtmlUi` entity, and the app's
/// values it replaced, so a later stylesheet or document that stops
/// declaring something gives it back. Fields and components the rule never
/// declared stay the app's (an app may move its UI every frame).
/// A node with `calc()`-valued layout properties: [`resolve_calc`]
/// evaluates them per frame against the measured basis (the containing
/// block's content box, the font sizes, the viewport) and writes the
/// concrete pixels.
#[derive(Component, Debug, PartialEq, Clone)]
pub(crate) struct CalcSized {
    /// The element's own font size (`em`'s basis), captured at collection.
    em: f32,
    exprs: Vec<(CalcProp, CalcExpr)>,
}

#[derive(Component)]
pub(crate) struct CssRoot {
    /// The `Node` before CSS: the declared fields hold the app's values.
    base: Node,
    layout: LayoutDecl,
    border: [Option<f32>; 4],
    padding: [Option<f32>; 4],
    row_gap: Option<f32>,
    calc: Vec<(CalcProp, CalcExpr)>,
    owned: RootOwned,
}

/// Components the root rule set: `Some(app's)` while CSS owns one (`None`
/// inside = the app had none, so giving it back removes it).
#[derive(Default)]
struct RootOwned {
    background: Option<Option<BackgroundColor>>,
    border_color: Option<Option<BorderColor>>,
    z_index: Option<Option<ZIndex>>,
    pickable: Option<Option<Pickable>>,
    image: Option<Option<ImageNode>>,
    fixed: Option<Option<FixedNode>>,
}

/// Computes styles from declared CSS + registered fonts.
pub(crate) struct Styler<'a> {
    pub(crate) styles: &'a HtmlStyles<'a>,
    pub(crate) fonts: &'a FontFamilies,
    /// Root font size, for `rem`.
    pub(crate) root_size: f32,
    /// For `border-image` sources.
    pub(crate) sheet: Option<&'a Stylesheet>,
    pub(crate) images: &'a Assets<Image>,
    /// For `<img src>` loads. `None` where assets can't load (the fuzzing
    /// harness): `<img>` runs are then skipped.
    pub(crate) server: Option<&'a AssetServer>,
    /// The template's asset path: `<img src="…">` resolves against it.
    pub(crate) template: Option<AssetPath<'static>>,
}

impl Styler<'_> {
    /// Whether some pseudo-referencing selector could match `element`
    /// given its chain (ADR 0029's reactivity record): only such an
    /// element's style can change when an interaction state flips.
    pub(crate) fn pseudo_reactive(
        &self,
        element: &HtmlElement,
        ancestors: &[Ancestor],
        element_state: &ElementState,
    ) -> bool {
        self.styles
            .pseudo_reactive(element, ancestors, element_state)
    }

    /// `element`'s computed style: its declared values (with its `style`
    /// attribute, `inline`) over `inherited`. `pseudo` is the element's
    /// interaction state (`:hover`/`:active`).
    #[hotpath::measure(impl_type = "Styler")]
    pub(crate) fn style_of(
        &self,
        element: &HtmlElement,
        inherited: &Style,
        pseudo: Pseudo,
        inline: Option<&InlineStyle>,
        ancestors: &[Ancestor],
        element_state: &ElementState,
    ) -> Style {
        let declared = self
            .styles
            .get_with(element, pseudo, inline, ancestors, element_state);
        let bold = declared
            .bold
            .unwrap_or(inherited.bold || element.tag == "th");

        let italic = declared.italic.unwrap_or(inherited.italic);
        let size = declared.font_size.map_or(inherited.size, |size| {
            size.resolve(inherited.size, self.root_size)
        });
        let letter_spacing = declared
            .letter_spacing
            .map_or(inherited.letter_spacing, |spec| {
                match spec {
                    LetterSpacingSpec::Normal => 0.0,
                    LetterSpacingSpec::Px(px) => px,
                    // `em` is against the element's own computed font size.
                    LetterSpacingSpec::Em(em) => em * size,
                    LetterSpacingSpec::Rem(rem) => rem * self.root_size,
                }
            });
        Style {
            color: declared.color.unwrap_or(inherited.color),
            // A declared list with no registered family falls back to the
            // default font (as in browsers), not to the inherited family.
            // The whole chain resolves (not just the first hit): Bevy 0.20
            // composes it into a per-glyph fallback list (ADR 0015).
            family: match declared.font_family.as_deref() {
                Some(list) => Some(self.fonts.resolve_chain(list)),
                None => inherited.family.clone(),
            },
            size,
            bold,
            italic,
            pointer_events: declared.pointer_events.unwrap_or(inherited.pointer_events),
            opacity: inherited.opacity * declared.opacity.unwrap_or(1.0),
            // ADR 0027: the inherited text properties. `line-height`'s
            // `rem` computes to an absolute length at the element (CSS:
            // the computed value inherits); a multiplier stays relative
            // and re-multiplies per element.
            align: declared.text_align.unwrap_or(inherited.align),
            line_height: match declared.line_height.unwrap_or(inherited.line_height) {
                LineHeightSpec::Rem(rem) => LineHeightSpec::Px(rem * self.root_size),
                spec => spec,
            },
            letter_spacing,
            list_style: declared.list_style.or(inherited.list_style),
            text_transform: declared.text_transform.unwrap_or(inherited.text_transform),
            white_space: declared.white_space.or(inherited.white_space),
        }
    }

    /// The inline element's run-level properties (`background-color`,
    /// `text-decoration`); `None` when it declares neither. Not inherited:
    /// `push_runs` keeps the subtree's running values unless an element
    /// declares its own.
    fn run_extras(
        &self,
        element: &HtmlElement,
        state: Pseudo,
        inline: Option<&InlineStyle>,
        ancestors: &[Ancestor],
        element_state: &ElementState,
    ) -> Option<RunExtras> {
        let declared = self
            .styles
            .get_with(element, state, inline, ancestors, element_state);
        if declared.background.is_none() && declared.text_decoration.is_none() {
            return None;
        }
        Some(RunExtras {
            background: declared.background,
            decoration: declared.text_decoration.unwrap_or_default(),
            link: None,
        })
    }

    /// An `<img>`'s handle and CSS `width`/`height`: `src` resolves
    /// relative to the template (like `{% include %}`), `/` from the asset
    /// root. URLs and data URIs are unsupported. Sizes come from the
    /// cascade (px, `%`; other units skipped); without a declaration the
    /// image keeps its intrinsic size.
    fn image_of(
        &self,
        tag: &tl::HTMLTag,
        element: &HtmlElement,
        state: Pseudo,
        ancestors: &[Ancestor],
        element_state: &ElementState,
    ) -> Option<ImageRun> {
        let src = tag
            .attributes()
            .get("src")
            .flatten()
            .map(|src| decode_attribute(src.as_utf8_str()))?;
        let template = match &self.template {
            Some(template) => template,
            None => {
                debug!("html img: {src} skipped (no template path)");
                return None;
            }
        };
        if src.contains("://") || src.starts_with("data:") {
            debug!("html img: {src} — only asset paths are supported; skipped");
            return None;
        }
        let resolved = template
            .resolve_embed_str(&src)
            .inspect_err(|error| {
                debug!("html img: {src} skipped ({error})");
            })
            .ok()?;
        let inline = cascade::inline_style(tag);
        let declared =
            self.styles
                .get_with(element, state, inline.as_ref(), ancestors, element_state);
        let size = |val: Option<Val>, name: &str| {
            val.and_then(|val| match val {
                Val::Px(px) => Some(ImageSize::Px(px)),
                Val::Percent(fraction) => Some(ImageSize::Percent(fraction / 100.0_f32)),
                val => {
                    debug!("html img: {name}: {val:?} — unsupported for images; skipped");
                    None
                }
            })
        };
        Some(ImageRun {
            image: self.server?.load(resolved),
            width: size(declared.layout.width, "width"),
            height: size(declared.layout.height, "height"),
        })
    }

    fn text_font(&self, style: &Style) -> TextFont {
        // The chain composes per weight/style: each family's face for the
        // request, in CSS order — one face stays a single source, several
        // become Bevy 0.20's fallback list (fontique picks per glyph).
        let font = match &style.family {
            Some(chain) => {
                let faces: Vec<FontSource> = chain
                    .iter()
                    .filter_map(|index| self.fonts.faces(*index))
                    .map(|faces| faces.face(style.bold, style.italic))
                    .collect();
                match faces.len() {
                    0 => FontSource::default(),
                    1 => faces.into_iter().next().unwrap(),
                    _ => FontSource::list(faces),
                }
            }
            None => FontSource::default(),
        };
        let mut text_font = TextFont::default().with_font_size(style.size);
        // A file is one face (its weight and style are baked in); a system
        // family is many, and the system picks by the requested weight/style.
        if !matches!(font, FontSource::Handle(_)) {
            if style.bold {
                text_font.weight = FontWeight::BOLD;
            }
            if style.italic {
                text_font.style = FontStyle::Italic;
            }
        }
        text_font.font = font;
        text_font
    }

    /// `element`'s box properties. A `border-image` whose `%` slices need the
    /// image size is skipped until the image has loaded (its load rebuilds).
    pub(crate) fn box_of(
        &self,
        element: &HtmlElement,
        pseudo: Pseudo,
        inline: Option<&InlineStyle>,
        ancestors: &[Ancestor],
        element_state: &ElementState,
    ) -> BoxStyle {
        let declared = self
            .styles
            .get_with(element, pseudo, inline, ancestors, element_state);
        let image = declared.border_image.as_ref().and_then(|decl| {
            let url = decl.source.as_ref()?.as_ref()?;
            let handle = self.sheet?.image(url)?;
            let (offsets, fill) = decl
                .slice
                .unwrap_or(([SliceValue::Fraction(1.0); 4], false));
            if !fill {
                debug!("html css: border-image without `fill` still draws the center in Bevy");
            }
            let size = self.images.get(handle).map(Image::size_f32);
            // [top, right, bottom, left]; % is of the height for top/bottom.
            let mut px = [0.0; 4];
            for (index, offset) in offsets.iter().enumerate() {
                px[index] = match offset {
                    SliceValue::Px(px) => *px,
                    SliceValue::Fraction(fraction) => {
                        let size = size?;
                        fraction * if index % 2 == 0 { size.y } else { size.x }
                    }
                };
            }
            let mode = if decl.tile.unwrap_or(false) {
                SliceScaleMode::Tile { stretch_value: 1.0 }
            } else {
                SliceScaleMode::Stretch
            };
            Some((
                handle.clone(),
                TextureSlicer {
                    border: BorderRect {
                        min_inset: Vec2::new(px[3], px[0]),
                        max_inset: Vec2::new(px[1], px[2]),
                    },
                    center_scale_mode: mode,
                    sides_scale_mode: mode,
                    max_corner_scale: 1.0,
                },
            ))
        });
        let background_image = declared.background_image.as_ref().and_then(|decl| {
            let handle = self.sheet?.image(&decl.url)?;
            let mode = if decl.tiled.0 || decl.tiled.1 {
                // 1.0 = tiles at the image's intrinsic size (bevy divides
                // the node's extent by image size x stretch_value; 0.0
                // would divide by zero).
                NodeImageMode::Tiled {
                    tile_x: decl.tiled.0,
                    tile_y: decl.tiled.1,
                    stretch_value: 1.0,
                }
            } else {
                NodeImageMode::Stretch
            };
            Some(ImageNode {
                visual_box: decl.clip,
                color: Color::WHITE,
                ..ImageNode::new(handle.clone()).with_mode(mode)
            })
        });
        BoxStyle {
            border: declared.border_width,
            border_color: declared.border_color,
            padding: declared.padding,
            background: declared.background,
            image,
            background_image,
            row_gap: declared.row_gap,
            z_index: declared.z_index.flatten(),
            outline: declared.outline,
            drawn_outline: None,
            layout: declared.layout.clone(),
            calc: declared.calc.clone(),
            transitions: declared.transitions.clone(),
            image_alpha: None,
        }
    }
}

/// The root rule's values (`root`: the document's `<html>` element, or a
/// bare `html` for fragments without one), over the defaults.
pub(crate) fn root_style(
    styles: &HtmlStyles,
    fonts: &FontFamilies,
    images: &Assets<Image>,
    server: Option<&AssetServer>,
    root: &HtmlElement,
    inline: Option<&InlineStyle>,
) -> Style {
    let defaults = Style {
        color: DEFAULT_COLOR,
        family: None,
        size: DEFAULT_FONT_SIZE,
        bold: false,
        italic: false,
        pointer_events: true,
        opacity: 1.0,
        align: TextAlignSpec::Left,
        line_height: LineHeightSpec::Normal,
        letter_spacing: 0.0,
        list_style: None,
        text_transform: TextTransformSpec::None,
        white_space: None,
    };
    Styler {
        styles,
        fonts,
        root_size: DEFAULT_FONT_SIZE,
        sheet: None,
        images,
        server,
        template: None,
    }
    .style_of(
        root,
        &defaults,
        Pseudo::default(),
        inline,
        &[],
        &ElementState::default(),
    )
}

/// The element the `HtmlUi` node itself is styled as, with its `style`
/// attribute: the document's top-level `<html>` (with its `id`/`class`), or
/// a bare `html` for fragments.
pub(crate) fn root_element(dom: &tl::VDom) -> (HtmlElement, Option<InlineStyle>) {
    dom.children()
        .iter()
        .filter_map(|handle| handle.get(dom.parser())?.as_tag())
        .find(|tag| tag.name().as_utf8_str().eq_ignore_ascii_case("html"))
        .map_or_else(
            || (element_tag("html"), None),
            |tag| (element_of(tag), cascade::inline_style(tag)),
        )
}

/// The system's asset params, bundled: Bevy's `SystemParam` tuples end at
/// 16, and `build_html_ui` has more.
#[derive(SystemParam)]
pub(crate) struct BuildAssets<'w> {
    sheets: Res<'w, Assets<Stylesheet>>,
    server: Res<'w, AssetServer>,
    images: Res<'w, Assets<Image>>,
    templates: Res<'w, Assets<HtmlTemplate>>,
    rem_size: Res<'w, RemSize>,
}

/// ADR 0031: the windowing params (the system is at Bevy's 16-param
/// limit).
#[derive(SystemParam)]
pub(crate) struct BuildWindow<'w, 's> {
    models: Query<'w, 's, &'static mut WindowModel>,
    tree: Tree<'w, 's>,
    pseudo_changed: Query<'w, 's, Entity, Changed<PseudoState>>,
}

#[hotpath::measure]
pub(crate) fn build_html_ui(
    mut commands: Commands,
    mut sheet_events: MessageReader<AssetEvent<Stylesheet>>,
    mut image_events: MessageReader<AssetEvent<Image>>,
    assets: BuildAssets<'_>,
    default_sheet: Res<DefaultStylesheet>,
    fonts: Res<FontFamilies>,
    mut views: Query<
        (
            Entity,
            Ref<RenderedHtml>,
            Ref<LocalizedText>,
            Option<Ref<HtmlStylesheet>>,
            Option<Ref<HtmlDebugOutline>>,
            &HtmlUi,
            Option<&InlineImages>,
            &mut RebuildState,
            Option<&HtmlWindowed>,
            Option<&ScrollPosition>,
            Option<Ref<ComputedNode>>,
        ),
        With<HtmlUi>,
    >,
    mut window: BuildWindow<'_, '_>,
    roots: Query<(&Node, Option<&CssRoot>), With<HtmlUi>>,
    parents: Query<&ChildOf>,
    contexts: Query<&TemplateContext>,
    mut skeletons: Query<&mut Skeleton>,
    dom_nodes: Query<(&DomNode, Option<&PseudoState>)>,
    mut removed_outlines: RemovedComponents<HtmlDebugOutline>,
    mut removed_sheets: RemovedComponents<HtmlStylesheet>,
) {
    let BuildAssets {
        sheets,
        server,
        images,
        templates,
        rem_size,
    } = assets;
    // Removals aren't `Ref` changes, so they're read separately.
    let removed_outlines: HashSet<Entity> = removed_outlines.read().collect();
    let removed_sheets: HashSet<Entity> = removed_sheets.read().collect();
    let reloaded_sheets: HashSet<AssetId<Stylesheet>> = sheet_events
        .read()
        .filter_map(|event| match event {
            AssetEvent::LoadedWithDependencies { id } | AssetEvent::Modified { id } => Some(*id),
            _ => None,
        })
        .collect();
    let loaded_images: HashSet<AssetId<Image>> = image_events
        .read()
        .filter_map(|event| match event {
            AssetEvent::LoadedWithDependencies { id } | AssetEvent::Modified { id } => Some(*id),
            _ => None,
        })
        .collect();
    let phase = |handle: &Handle<Stylesheet>| match sheets.get(handle) {
        Some(_) => Phase::Ready,
        None if matches!(server.load_state(handle.id()), LoadState::Failed(_)) => Phase::Failed,
        None => Phase::Loading,
    };
    // A sheet reloaded, or one of its `border-image` images (re)loaded.
    let refreshed = |handle: &Handle<Stylesheet>| {
        reloaded_sheets.contains(&handle.id())
            || sheets.get(handle).is_some_and(|css| {
                css.images()
                    .any(|image| loaded_images.contains(&image.id()))
            })
    };
    let default_changed =
        default_sheet.is_changed() || default_sheet.0.as_ref().is_some_and(refreshed);

    // UIs with an element whose `:hover`/`:active` state changed: the state
    // change restyles that UI (the owning `HtmlUi` is the walk's first hit).
    // ADR 0029: the walk also records, per owning UI, the flipped
    // elements' handles and every handle on their path up to the root, in
    // the last build's handle space. The scoping that turns this into the
    // reuse-blocking set runs at the pass (it needs the old document).
    let mut state_restyle: HashMap<Entity, (Vec<tl::NodeHandle>, HashSet<tl::NodeHandle>)> =
        HashMap::new();
    for entity in window.pseudo_changed.iter() {
        let mut path = Vec::new();
        let mut current = Some(entity);
        let mut owner = None;
        while let Some(node) = current {
            if views.contains(node) {
                owner = Some(node);
                break;
            }
            path.push(node);
            current = parents.get(node).ok().map(|parent| parent.0);
        }
        if let Some(owner) = owner {
            let (flips, blocked) = state_restyle.entry(owner).or_default();
            if let Ok((handle, _)) = dom_nodes.get(entity) {
                flips.push(**handle);
            }
            for node in path {
                if let Ok((handle, _)) = dom_nodes.get(node) {
                    blocked.insert(**handle);
                }
                // ADR 0028: the sibling combinators — a flip may change a
                // FOLLOWING sibling's match (`+`/`~`), so the following
                // siblings join the blocked set at every level (their
                // subtrees re-decide from their own reactivity).
                if let Some(parent) = parents.get(node).ok().map(|parent| parent.0)
                    && let Ok((Some(siblings), ..)) = window.tree.get(parent)
                {
                    let mut children = Vec::<Entity>::new();
                    for child in siblings.iter() {
                        children.push(child);
                    }
                    if let Some(index) = children.iter().position(|child| *child == node) {
                        for sibling in &children[index + 1..] {
                            if let Ok((DomNode(handle), _)) = dom_nodes.get(*sibling) {
                                blocked.insert(*handle);
                            }
                        }
                    }
                }
            }
        }
    }

    // First pass: the decision per UI. Nested UIs need the whole set before
    // anything acts.
    let mut decisions: Vec<(Entity, Decision, Frame, bool)> = Vec::new();
    for (
        entity,
        rendered,
        localized,
        own_sheet,
        outline,
        _ui,
        used_images,
        mut rebuild,
        windowed,
        scroll,
        computed,
    ) in &mut views
    {
        // The context's values for the sheet's `var()` names: a change
        // restyles (the values style, they don't render). Hashed in name
        // order, so a per-frame context update to any other key (the calc
        // example's steppers, say) signals nothing. The names come from
        // the sheet the source resolution picks (a loading sheet waits
        // anyway; its frame recomputes once it resolves).
        let (own_phase, default_phase) = (
            own_sheet.as_ref().map(|own| phase(&own.0)),
            default_sheet.0.as_ref().map(phase),
        );
        let names_sheet = match (own_phase, default_phase) {
            (Some(Phase::Ready), _) => own_sheet.as_ref().and_then(|own| sheets.get(&own.0)),
            (Some(Phase::Loading), _) => None,
            (Some(Phase::Failed) | None, Some(Phase::Ready)) => default_sheet
                .0
                .as_ref()
                .and_then(|default| sheets.get(default)),
            (Some(Phase::Failed) | None, _) => None,
        };
        let context = contexts.get(entity).ok();
        let vars_hash = context_vars_hash(names_sheet, context, &rebuild.inline_vars);

        // ADR 0031: a windowed UI rebuilds when its scroll offset moves
        // (the materialized band follows) or its width changes (the
        // heights are wrapping-dependent). The scroll signal is consumed
        // by the build that runs for it, like `vars`.
        // ADR 0031: a windowed root carries the height model from its
        // first build on (the measurement, ordered after this, fills it
        // the same frame).
        if windowed.is_some() && window.models.get(entity).is_err() {
            commands.entity(entity).insert(WindowModel::default());
        }
        // ADR 0031 (review_0006 J05): a windowed UI rebuilds when the band
        // could change — the scroll offset left the slack the last build
        // recorded (how far it may move before any element crosses a band
        // edge), the viewport changed, or the heights did. An exact
        // offset comparison rebuilt on every sub-pixel step of a smooth
        // scroll. A `Wait` keeps the old anchor, so the move outlives it.
        let window_changed = windowed.is_some()
            && window.models.get(entity).is_ok_and(|model| {
                let moved = scroll.zip(computed.as_ref()).zip(model.anchor).is_some_and(
                    |((scroll, node), anchor)| {
                        let delta = visible_scroll(scroll, node) - anchor;
                        delta > model.slack.0 || -delta > model.slack.1
                    },
                );
                // A width change clears the model in the measurement
                // (bug_0173), which flags the rebuild; the viewport's
                // height is the build's own business.
                let resized = computed
                    .as_ref()
                    .is_some_and(|node| (visible_region(node).0 - model.viewport).abs() > 0.5);
                model.changed || moved || resized
            });

        let frame = Frame {
            own: own_phase,
            default: default_phase,
            document_ready: !matches!(*rendered, RenderedHtml::Pending),
            own_changed: removed_sheets.contains(&entity)
                || own_sheet
                    .as_ref()
                    .is_some_and(|own| own.is_changed() || refreshed(&own.0)),
            default_changed,
            fonts_changed: fonts.is_changed(),
            state_changed: state_restyle.contains_key(&entity),
            vars_changed: vars_hash != rebuild.vars,
            images_changed: used_images.is_some_and(|images| {
                images
                    .0
                    .iter()
                    .any(|handle| loaded_images.contains(&handle.id()))
            }),
            content_changed: rendered.is_changed()
                || localized.is_changed()
                || removed_outlines.contains(&entity)
                || outline.as_ref().is_some_and(|outline| outline.is_changed())
                || window_changed,
        };
        let decision = rebuild.decide(frame);
        // The restyled builds record the hash; a `Wait` (a sheet still
        // loading) keeps the old one, so the change outlives the wait —
        // the same rule that keeps pending changes alive.
        if !matches!(decision, Decision::Wait | Decision::Skip) {
            rebuild.vars = vars_hash;
        }
        decisions.push((entity, decision, frame, rebuild.style_pending()));
    }
    // Deepest UIs first: a nested `HtmlUi`'s commands are queued before an
    // ancestor's, so an ancestor update that despawns the element holding it
    // comes after them (and one that keeps it keeps the nested UI updated).
    let depth = |entity: Entity| parents.iter_ancestors(entity).count();
    decisions.sort_by_key(|(entity, ..)| std::cmp::Reverse(depth(*entity)));

    for (entity, decision, frame, style_pending) in decisions {
        let (source, restyle) = match decision {
            Decision::Wait | Decision::Skip => continue,
            Decision::Build(source) => (source, false),
            Decision::Restyle(source) => (source, true),
        };
        let Ok((
            _,
            rendered,
            localized,
            own_sheet,
            outline,
            ui,
            _used_images,
            mut rebuild,
            windowed,
            scroll,
            computed,
        )) = views.get_mut(entity)
        else {
            continue;
        };
        // This update applies the styles: cached specs are current again
        // after it (the same consume point `pending` already uses). An
        // outline build leaves the skeleton frozen, so the
        // outline-removal build must not reuse its stale specs (a theme
        // swap while the outline was shown kept the old theme,
        // review_0003 G26) — the gate below blocks it.
        // ADR 0031: the model guard lives through the collect (the
        // window ctx borrows it).
        let mut model_guard = window.models.get_mut(entity).ok();
        // ADR 0029: read the full-style latch BEFORE the apply-clear
        // consumes it — the pass's `full_style` reads it below.
        let full_style_latch = rebuild.pending_full_style();
        rebuild.style_pending_applied();
        let css = match source {
            Source::Own => own_sheet.as_ref().and_then(|own| sheets.get(&own.0)),
            Source::Default => default_sheet.0.as_ref().and_then(|d| sheets.get(d)),
            Source::Unstyled => None,
        };

        // `var(--name)` resolves against the UI's `TemplateContext` first
        // (values interpolated as written), then the sheet's own `--`
        // properties. Built per pass: the context is constant for one
        // styling run, so the per-element cache stays keyed on identity.
        let context = contexts.get(entity).ok();
        let lookup = |name: &str| {
            context
                .and_then(|context| context.get(name))
                .and_then(css_value)
        };
        let styles = match css {
            Some(css) => HtmlStyles::from_sheet(css.sheet()).with_context(&lookup),
            None => HtmlStyles::default().with_context(&lookup),
        };
        let (root_element, root_inline) = match (&*rendered, &outline) {
            (RenderedHtml::Ready(document), None) => root_element(document.dom()),
            _ => (element_tag("html"), None),
        };
        let root = root_style(
            &styles,
            &fonts,
            &images,
            Some(&server),
            &root_element,
            root_inline.as_ref(),
        );
        let template = match (&*rendered, ui) {
            (RenderedHtml::Ready(_), HtmlUi(template)) => templates
                .get(template)
                .map(|template| AssetPath::parse(&template.name).clone_owned()),
            _ => None,
        };
        let styler = Styler {
            styles: &styles,
            fonts: &fonts,
            root_size: root.size,
            sheet: css,
            images: &images,
            server: Some(&server),
            template,
        };

        let mut root_box = styler.box_of(
            &root_element,
            Pseudo::default(),
            root_inline.as_ref(),
            &[],
            &ElementState::default(),
        );
        root_box.fade(root.opacity);
        let root_grid = root_box.layout.grid_names();
        // Nested containers space their children like the root does.
        let default_gap = match root_box.row_gap {
            Some(gap) => Val::Px(gap),
            None => roots.get(entity).map_or(Val::Auto, |(node, state)| {
                match state.filter(|state| state.row_gap.is_some()) {
                    Some(state) => state.base.row_gap,
                    None => node.row_gap,
                }
            }),
        };
        let root_size = root.size;
        let root_pointer_events = root.pointer_events;
        commands
            .entity(entity)
            .queue(move |mut entity: EntityWorldMut| {
                apply_root(&mut entity, root_box, root_pointer_events, root_size);
            });

        // ADR 0021: pair the fresh document with the previous build's when
        // only the template's rendered content changed; unchanged subtrees
        // reuse their items, so the cascade and run construction skip
        // them. ADR 0029: a restyle pairs too. Its document is the
        // previous one (re-parsed — identical sources parse to identical
        // handle numbering), so every subtree pairs clean content-wise,
        // and the pass scopes which of them may reuse: a `PseudoState`
        // flip only affects the flipped element's subtree (no `:has` —
        // rules match an element by its own and its ancestors' pseudo
        // states), while a stylesheet, font, image or `var()` change can
        // touch any element (`full_style`: no reuse, as before this ADR).
        // A build carrying style changes (`style_pending`) still rebuilds
        // fully: its content pairing would hand out stale styles.
        // A localization change doesn't block reuse: only translated
        // elements depend on it, and those are excluded from the pairing
        // (their items come from per-build fragments).
        const SKELETON: bool = true;
        let document_ready = !matches!(*rendered, RenderedHtml::Pending);
        let skeleton_eligible = SKELETON
            && outline.is_none()
            && !removed_outlines.contains(&entity)
            && document_ready
            // ADR 0031 (review_0006 J04): a windowed UI's scroll build has
            // an unchanged document; it pairs too, so the rows that stay
            // in the band reuse their specs instead of every scroll frame
            // re-collecting the whole band.
            && (rendered.is_changed() || restyle || windowed.is_some())
            && (!style_pending || restyle);
        let (previous_useful, previous_since_try, previous_strike, had_skeleton) = skeletons
            .get(entity)
            .map_or((true, 0, 0, false), |skeleton| {
                (skeleton.useful, skeleton.since_try, skeleton.strike, true)
            });
        let mut pairing = HashMap::default();
        let mut reverse = HashMap::default();
        let mut clean = HashSet::default();
        let mut skeleton_mut = if skeleton_eligible {
            skeletons.get_mut(entity).ok()
        } else {
            None
        };
        let pass = 'pass: {
            let Some(skeleton) = skeleton_mut.as_mut() else {
                break 'pass None;
            };
            let retry_every = 16u32 << skeleton.strike.min(4);
            // A windowed UI always pairs: the pairing also carries its
            // height model across a content change (`WindowModel::remap`).
            // Off, the model cleared and every content build materialized
            // the whole document for a frame — and a windowed build's
            // reuse rate is a band's, so the gate tripped easily
            // (bug_0167).
            if !skeleton.useful && skeleton.since_try < retry_every && windowed.is_none() {
                break 'pass None;
            }
            let Some(old_document) = skeleton.document.as_ref() else {
                break 'pass None;
            };
            let RenderedHtml::Ready(document) = &*rendered else {
                break 'pass None;
            };
            // ADR 0029: which cached subtrees may reuse. A restyle from a
            // sheet, font, image or `var()` change can restyle anything —
            // nothing reuses. A pure `PseudoState` restyle reuses
            // everything outside the flipped elements' paths to the root:
            // a flip restyles its own element (if a rule matches its
            // state) and — through inheritance — its subtree, and the
            // walk must *reach* the flip through its ancestors, so those
            // re-collect; the descendants re-collect only where the
            // re-collected ancestor's inherited style actually changed
            // (the walk compares it against the cached spec's).
            // ADR 0029: a full-page style signal — live this frame, or
            // latched by a `Wait` (the swap frame's flags are consumed by
            // the time the sheet resolves; the latch carries the
            // full-page-ness to this frame).
            let full_style = restyle
                && (frame.own_changed
                    || (frame.own != Some(Phase::Ready) && frame.default_changed)
                    || frame.fonts_changed
                    || frame.images_changed
                    || frame.vars_changed
                    || full_style_latch);
            let (_, blocked) = state_restyle.remove(&entity).unwrap_or_default();
            pair_trees_into(
                old_document.dom(),
                document.dom(),
                &mut pairing,
                &mut reverse,
                &mut clean,
                styles.reads(),
            );
            // The cached specs' handles point into the previous document;
            // rewrite them to the fresh one while the `Arc`s are
            // exclusively held, so reused subtrees (and the entities they
            // spawn) never carry stale `DomNode`s.
            remap_spec_handles(&mut skeleton.specs, &reverse);
            break 'pass Some(SkeletonPass {
                pairing: &pairing,
                clean: &clean,
                specs: &skeleton.specs,
                table: &skeleton.table,
                reused: Cell::new(0),
                full_style,
                flipped: blocked,
                // ADR 0031: the windowing model re-keys by this (the
                // pairing itself is fresh→old).
                reverse: &reverse,
            });
        };

        // The elements' interaction state by DOM node, so the collected
        // items style for `:hover`/`:active`. Nested `HtmlUi`s have their
        // own documents — their handles index those, not this one — so their
        // subtrees are skipped (the starting entity is the UI root itself).
        // A translated element's subtree is fragment-keyed (its spans'
        // `DomNode` handles index the translation fragment, re-parsed per
        // build), so those states are collected per fragment. The map holds
        // the last build's states: a changed translation misses and styles
        // unhovered until the next build's fresh map.
        let mut fragments: HashMap<tl::NodeHandle, HashMap<tl::NodeHandle, Pseudo>> =
            HashMap::new();
        let pseudo = |state: Option<&PseudoState>| {
            state.copied().map_or(Pseudo::default(), |state| Pseudo {
                hover: state.hovered,
                active: state.active,
                focus: state.focused,
                focus_visible: state.focus_visible,
            })
        };
        let mut states = HashMap::new();
        let mut focus_within: HashSet<tl::NodeHandle> = HashSet::new();
        let mut stack = vec![(entity, None)];
        while let Some((current, fragment)) = stack.pop() {
            if let Ok((handle, state)) = dom_nodes.get(current) {
                // ADR 0028: the focused element and every ancestor up the
                // UI's tree hold the focus for `:focus-within`.
                if state.is_some_and(|state| state.focused) {
                    let mut current = Some(current);
                    while let Some(node) = current {
                        if let Ok((DomNode(handle), _)) = dom_nodes.get(node) {
                            focus_within.insert(reverse.get(handle).copied().unwrap_or(*handle));
                        }
                        current = parents.get(node).ok().map(|parent| parent.0);
                    }
                }
                // The entity's `DomNode` may lag the fresh document (a
                // reused subtree's handle is written by the frame it last
                // synced): map it through the pairing, so the keys match
                // what the collection looks up.
                let handle = reverse.get(&**handle).copied().unwrap_or(**handle);
                match fragment {
                    Some(block) => {
                        fragments
                            .entry(block)
                            .or_default()
                            .insert(handle, pseudo(state));
                    }
                    None => {
                        states.insert(handle, pseudo(state));
                    }
                }
            }
            // A translated element starts its own fragment: its subtree's
            // spans are spawned from the re-parsed translation, so their
            // `DomNode` handles index the fragment. Only a document element
            // starts one (fragments aren't localized again).
            let fragment = match fragment {
                Some(fragment) => Some(fragment),
                None => dom_nodes.get(current).ok().and_then(|(handle, _)| {
                    let handle = reverse.get(&**handle).copied().unwrap_or(**handle);
                    localized.0.contains_key(&handle).then_some(handle)
                }),
            };
            let nested_root = current != entity
                && window
                    .tree
                    .get(current)
                    .is_ok_and(|(.., nested_ui)| nested_ui);
            if !nested_root && let Ok((children, ..)) = window.tree.get(current) {
                stack.extend(
                    children
                        .iter()
                        .flat_map(|children| children.iter())
                        .map(|child| (child, fragment)),
                );
            }
        }

        let items = if outline.is_some() {
            let text = match &*rendered {
                RenderedHtml::Pending => continue,
                RenderedHtml::Ready(document) => {
                    let outline = document.outline(&localized);
                    debug!("DOM of {entity}:\n{outline}");
                    outline.trim_end().to_owned()
                }
                RenderedHtml::Failed(message) => format!("failed to render: {message}"),
            };
            let style = styler.style_of(
                &element_tag("pre"),
                &root,
                Pseudo::default(),
                None,
                &[],
                &ElementState::default(),
            );
            vec![Item::Block(Block {
                kind: BlockKind::Paragraph,
                element: None,
                handle: None,
                key: None,
                signals: Vec::new(),
                focus: None,
                custom: None,
                submit: false,
                runs: vec![Run {
                    text,
                    style: style.clone(),
                    extras: RunExtras::default(),
                    image: None,
                }],
                boxed: BoxStyle::default(),
                style,
                // The outline's own style: `pre` with `Pseudo::default()`
                // — no interaction state restyles it (ADR 0029).
                pseudo_reactive: false,
                marker: None,
                white_space: WhiteSpaceSpec::Normal,
            })]
        } else {
            match &*rendered {
                RenderedHtml::Pending => continue,
                RenderedHtml::Ready(document) => {
                    // ADR 0031: the windowing state for this build. The
                    // model rides the pairing (a content change re-keys
                    // it into the fresh document; a same-document build
                    // keeps it), and a width divergence clears it
                    // (wrapping changes heights).
                    // ADR 0031 (review_0006 J05): the band's slack, filled
                    // by the cull decisions, stored into the model below.
                    let window_slack = (
                        std::cell::Cell::new(f32::INFINITY),
                        std::cell::Cell::new(f32::INFINITY),
                    );
                    let mut built_window: Option<(f32, f32)> = None;
                    let window_stale = std::cell::Cell::new(false);
                    let window = windowed
                        .as_ref()
                        .zip(scroll.as_ref())
                        .zip(computed.as_ref())
                        .and_then(|((windowed, scroll), node)| {
                            let model = model_guard.as_deref_mut()?;
                            // This build consumes the measurement's news.
                            model.changed = false;
                            match pass.as_ref() {
                                // A new document re-keys the model; an
                                // unchanged one (a scroll or restyle build)
                                // keeps it — remapping through the pairing,
                                // which holds element pairs only, would
                                // drop the anonymous blocks' heights.
                                Some(pass) if rendered.is_changed() => model.remap(pass.reverse),
                                None if rendered.is_changed() => model.clear(),
                                _ => {}
                            }
                            // The visible region in the cursor's coordinates
                            // (logical, from the content box's top): the
                            // padding box, `scroll` down, starting
                            // `padding-top` above the content.
                            // A root that lays its children out as a row
                            // (the `HtmlUi` default without `html {
                            // flex-direction: column }`), a grid or
                            // reversed can't be cursor-tracked: no window.
                            if roots
                                .get(entity)
                                .is_ok_and(|(root_node, _)| children_are_opaque(root_node))
                            {
                                return None;
                            }
                            let (viewport, padding_top) = visible_region(node);
                            // No layout yet (a zero-height root: before
                            // the first layout, or a render-less app): an
                            // unknown viewport windows nothing — a zero
                            // band would cull the whole document.
                            if viewport <= 0.0 {
                                return None;
                            }
                            let margin = windowed.margin_viewports.max(0.0) * viewport;
                            let scroll = visible_scroll(scroll, node);
                            let offset = scroll - padding_top;
                            let default_gap = match default_gap {
                                Val::Px(gap) => gap,
                                _ => 0.0,
                            };
                            built_window = Some((scroll, viewport));
                            Some(WindowCtx {
                                model,
                                band: (offset - margin, offset + viewport + margin),
                                frames: std::cell::RefCell::new(Vec::new()),
                                default_gap,
                                slack: &window_slack,
                                stale: &window_stale,
                            })
                        });
                    let (items, inline_vars) = collect_items(
                        document.dom(),
                        &localized.0,
                        Some(&states),
                        &fragments,
                        &focus_within,
                        &styler,
                        root,
                        root_grid.as_ref(),
                        pass.as_ref(),
                        window,
                    );
                    // Where this build windowed, and how far the offset may
                    // move before the band changes (`None`: nothing was
                    // windowed — no layout yet; the viewport check
                    // rebuilds once there is one).
                    if windowed.is_some()
                        && let Some(model) = model_guard.as_deref_mut()
                    {
                        match built_window {
                            Some((anchor, viewport)) => {
                                model.anchor = Some(anchor);
                                model.slack = (window_slack.0.get(), window_slack.1.get());
                                model.viewport = viewport;
                                // A refused cull: rebuild once (as for a
                                // measurement's news — bug_0162).
                                model.changed |= window_stale.get();
                            }
                            None => {
                                model.anchor = None;
                                model.viewport =
                                    computed.as_ref().map_or(0.0, |node| visible_region(node).0);
                            }
                        }
                    }
                    // The fresh names weren't in the recorded hash; the
                    // next context change must restyle. A reuse build's
                    // fresh set is missing the reused subtrees' names
                    // (their scan is skipped) — assigning it would force
                    // a spurious restyle, whose full set differs again
                    // and forces a second one (review_0003 G27): union
                    // with the previous set instead. A name that an edit
                    // removed lingers until the next full rebuild resets
                    // the set — the worst case is one harmless
                    // over-restyle on a context change of a dead name.
                    let inline_vars = if pass.as_ref().is_some_and(|pass| pass.reused.get() > 0) {
                        let mut merged = inline_vars;
                        merged.extend(rebuild.inline_vars.iter().cloned());
                        merged
                    } else {
                        inline_vars
                    };
                    if rebuild.inline_vars != inline_vars {
                        rebuild.inline_vars = inline_vars;
                        rebuild.vars = 0;
                    }
                    items
                }
                RenderedHtml::Failed(message) => vec![Item::Block(Block {
                    kind: BlockKind::Paragraph,
                    element: None,
                    handle: None,
                    key: None,
                    signals: Vec::new(),
                    focus: None,
                    custom: None,
                    submit: false,
                    runs: vec![Run {
                        text: format!("failed to render: {message}"),
                        style: root.clone(),
                        extras: RunExtras::default(),
                        image: None,
                    }],
                    style: root,
                    // A failed render's error paragraph never restyles
                    // with an interaction state (ADR 0029).
                    pseudo_reactive: false,
                    marker: None,
                    white_space: WhiteSpaceSpec::Normal,
                    boxed: BoxStyle::default(),
                })],
            }
        };

        // ADR 0031: the roots have no container to fold them, so the
        // top-level runs coalesce at assembly.
        let items = coalesce_spacers(items);
        let specs: Vec<SpecNode> = items
            .into_iter()
            .map(|item| match item {
                Item::Cached(spec) => SpecNode::Cached(spec),
                item => SpecNode::Fresh(Arc::new(item_spec(&styler, &item, default_gap))),
            })
            .collect();
        // Content changes and restyles alike update the existing children in
        // place where they still match, spawning and despawning only what
        // differs.
        let inline_images: Vec<Handle<Image>> = specs
            .iter()
            .flat_map(|spec| spec.spec().images.iter().cloned())
            .collect();
        commands.entity(entity).insert(InlineImages(inline_images));
        // CSS `rem` resolves against the root font-size: keep Bevy's
        // `RemSize` resource in sync. No restyle is queued: Bevy's layout
        // re-resolves every `Val::Rem` when the resource changes, and the
        // calc pass reads the new size on its next frame anyway.
        if **rem_size != root_size {
            commands.insert_resource(RemSize(root_size));
        }
        let mut connected = Vec::new();
        let spawned = update_children(
            &mut commands,
            entity,
            &specs,
            &window.tree,
            entity,
            &mut connected,
            &dom_nodes,
            Some(&pairing),
            windowed.is_some(),
        );
        // ADR 0021: this build's items become the next build's skeleton —
        // the reuse cache. The accepted costs: every build without a
        // pairing walk compares the document by source (a clone + a
        // string compare), and a gate-skipped build re-parses at retry
        // time — both load-bearing for R04/R05's "the skeleton never
        // pairs against the wrong document" (comparing without the
        // clone would need the queued closure to hold a borrow, which
        // commands don't allow). Three things must always correspond: the
        // skeleton's document, its specs, and the entities (which a build
        // that updated them from fresh specs has moved — a full rebuild or
        // restyle, or a gate-skipped one). So every build without a
        // pairing walk replaces the cached specs, and the stored document
        // is the render those specs were collected from — re-parsed when
        // the stored one no longer matches it (a render that changed while
        // a `Wait` swallowed the build, or while the outline froze the
        // skeleton, would otherwise pair against the wrong document).
        // Outline and failed documents leave it frozen.
        if outline.is_none() {
            let current_source: Option<String> = match &*rendered {
                RenderedHtml::Ready(document) => Some(document.source().to_owned()),
                _ => None,
            };
            let parse = |source: String| HtmlDocument::parse(source).ok();
            match pass.as_ref() {
                Some(pass) => {
                    let total = spec_count(&specs);
                    // ADR 0029: a restyle's reuse rate says nothing about
                    // content-pairing utility (a scoped pseudo restyle
                    // reuses most of the page; a sheet-driven one reuses
                    // nothing by design) — a restyle pass preserves the
                    // content path's adaptive state (useful *and* strike)
                    // instead of striking.
                    // A windowed UI's scroll build (an unchanged
                    // document) says nothing about content pairing either:
                    // a long jump reuses nothing, and striking for it turned
                    // the pairing off for the next scrolls (review_0006 J04).
                    let any_build = matches!(decision, Decision::Build(_));
                    let content_build = any_build && (rendered.is_changed() || windowed.is_none());
                    let content_useful = pass.reused.get() > 0 && pass.reused.get() * 2 >= total;
                    let useful = if content_build {
                        content_useful
                    } else {
                        previous_useful
                    };
                    // A useless retry backs the next one off; a useful one
                    // (or the first) restores the short cadence.
                    let strike = if !content_build {
                        previous_strike
                    } else if content_useful {
                        0
                    } else {
                        previous_strike + 1
                    };
                    let table = spec_table(&specs);
                    let current_source = current_source.clone();
                    let reuse = any_build.then(|| (pass.reused.get(), total));
                    // The stored document must be the render the specs
                    // were collected from — but a re-parse per build is
                    // the cost of owning it. Identical sources parse to
                    // identical handle numbering, so an unchanged source
                    // keeps the stored document (the common restyle: the
                    // per-frame `var()` storm re-parses nothing).
                    commands.queue(move |world: &mut World| {
                        let Some(mut skeleton) = world.get_mut::<Skeleton>(entity) else {
                            let document = current_source.and_then(|source| parse(source));
                            world.entity_mut(entity).insert(Skeleton {
                                document,
                                specs,
                                table,
                                useful,
                                since_try: 0,
                                strike,
                                last_reuse: reuse,
                            });
                            return;
                        };
                        skeleton.specs = specs;
                        skeleton.table = table;
                        let unchanged = match (&skeleton.document, &current_source) {
                            (Some(old), Some(new)) => old.source() == new.as_str(),
                            (None, None) => true,
                            _ => false,
                        };
                        if !unchanged {
                            skeleton.document = current_source.and_then(|source| parse(source));
                        }
                        skeleton.useful = useful;
                        skeleton.since_try = 0;
                        skeleton.strike = strike;
                        if reuse.is_some() {
                            skeleton.last_reuse = reuse;
                        }
                    });
                }
                None if !had_skeleton => {
                    let document = current_source.and_then(|source| parse(source));
                    let table = spec_table(&specs);
                    commands.entity(entity).insert(Skeleton {
                        document,
                        specs,
                        table,
                        useful: true,
                        since_try: 0,
                        strike: 0,
                        last_reuse: None,
                    });
                }
                None => {
                    // A full rebuild or restyle ran (the pairing was off),
                    // or the adaptive gate skipped the walk: the entities
                    // were updated from the fresh specs either way. The
                    // retry counter ticks only for a gate-skipped build.
                    let tick = u32::from(skeleton_eligible);
                    let since_try = previous_since_try + tick;
                    let table = spec_table(&specs);
                    let reuse =
                        matches!(decision, Decision::Build(_)).then(|| (0, spec_count(&specs)));
                    commands.queue(move |world: &mut World| {
                        let Some(mut skeleton) = world.get_mut::<Skeleton>(entity) else {
                            // No skeleton yet: store the fresh specs as the
                            // first one.
                            let document = current_source.and_then(|source| parse(source));
                            world.entity_mut(entity).insert(Skeleton {
                                document,
                                specs,
                                table,
                                useful: previous_useful,
                                since_try,
                                strike: previous_strike,
                                last_reuse: reuse,
                            });
                            return;
                        };
                        skeleton.specs = specs;
                        skeleton.table = table;
                        let unchanged = match (&skeleton.document, &current_source) {
                            (Some(old), Some(new)) => old.source() == new.as_str(),
                            (None, None) => true,
                            _ => false,
                        };
                        if !unchanged {
                            skeleton.document = current_source.and_then(|source| parse(source));
                        }
                        skeleton.since_try = since_try;
                        if reuse.is_some() {
                            skeleton.last_reuse = reuse;
                        }
                    });
                }
            }
        }
        // After the spawns, before `HtmlUiBuilt`: its observers see what the
        // definitions attached.
        for element in connected {
            commands.queue(move |world: &mut World| custom_elements::connect(world, element));
        }
        // `HtmlUiRestyled` promises every element (and what the app attached)
        // was kept; anything else is a build.
        if restyle && !spawned {
            commands.trigger(HtmlUiRestyled { entity });
        } else {
            commands.trigger(HtmlUiBuilt { entity });
        }
    }
}

/// What one UI node should be, computed from the item tree, then spawned
/// ([`spawn_spec`]) or applied onto a matching existing entity
/// ([`update_spec`]): one description for builds, updates and restyles.
struct NodeSpec {
    node: Node,
    element: Option<HtmlElement>,
    /// The element's `data-on-*` hooks.
    signals: Vec<SignalBinding>,
    /// Focusability from `tabindex` / `data-on-click` / `autofocus`.
    focus: Option<Focusable>,
    /// The `is` definition to run once spawned (not on restyles).
    custom: Option<CustomElement>,
    /// The DOM node, for `:hover`/`:active` restyles.
    handle: Option<tl::NodeHandle>,
    /// The element's computed (inherited) style: what its subtree's
    /// inheritance base was at collection (ADR 0029's scoping compares a
    /// re-collected element's fresh style with this to decide whether its
    /// children must re-cascade).
    style: Style,
    /// ADR 0029: a pseudo-referencing selector could match this element
    /// or its content (the runs' inline elements included) — an
    /// interaction flip on it or an ancestor may change its style.
    pseudo_reactive: bool,
    /// ADR 0027: the node's `overflow: auto` axes (which the promotion
    /// system watches).
    auto_scroll: [bool; 2],
    background: Option<Color>,
    image: Option<ImageNode>,
    border_color: Option<BorderColor>,
    z_index: Option<ZIndex>,
    outline: Option<Outline>,
    /// `false` for `pointer-events: none` (`Pickable::IGNORE`).
    pickable: bool,
    /// `position: fixed` (`FixedNode` component).
    fixed: bool,
    /// `visibility: hidden` → `Visibility::Hidden` (subtree-hide: Bevy's
    /// visibility propagates to descendants; layout keeps the space).
    visibility: Option<Visibility>,
    /// The element's computed font-size (CSS `em` resolves against it via
    /// Bevy's `EmSize`; `rem` against the synced `RemSize`).
    font_size: f32,
    /// `<button type="submit">`: clicking emits the enclosing form's
    /// `Submit` signal.
    submit: bool,
    /// `<input type="text">` (the `text-input` feature): an editable text
    /// field. `Some` = initial text and the cascade's font/color.
    #[cfg(feature = "text-input")]
    text_input: Option<TextInputSpec>,
    /// The `<img>` handles this subtree's blocks use (aggregated up for the
    /// UI's [`InlineImages`]).
    images: Vec<Handle<Image>>,
    /// `Text` nodes hold spans, never child nodes.
    text: Option<TextSpec>,
    children: Vec<SpecNode>,
    /// ADR 0031: the DOM node the windowing model keys this subtree by —
    /// the element's handle, or a raw text node's for its anonymous block.
    /// Carries into a `WindowKey` component so the measurement can map
    /// entities back to the model without exposing `DomNode` semantics.
    window_key: Option<tl::NodeHandle>,
    /// ADR 0031: a windowing spacer — the entity is an invisible fixed
    /// height standing in for a culled subtree.
    spacer: Option<f32>,
    /// The `calc()`-valued layout properties: the layout pass resolves
    /// them per frame against the measured basis.
    calc: Vec<(CalcProp, CalcExpr)>,
    /// Declared `transition`: which color writes [`update_spec`]/[`apply_css_owned`]
    /// animate.
    transitions: Vec<TransitionDecl>,
}

struct TextSpec {
    /// Before the spans, in the block's own style (the `li` marker).
    prefix: String,
    font: TextFont,
    color: Color,
    no_wrap: bool,
    /// ADR 0027: the block's line alignment (`text-align`).
    justify: Justify,
    /// ADR 0027: the block's line height (`line-height`).
    line_height: LineHeight,
    /// ADR 0027: the block's letter spacing (`letter-spacing`), in px.
    letter_spacing: LetterSpacing,
    spans: Vec<SpanSpec>,
}

/// One inline child of the block's `Text`: a styled run, or an `<img>`
/// (`image.is_some()`; its `text` is empty).
struct SpanSpec {
    text: String,
    font: TextFont,
    color: Color,
    /// The owning inline element's `background-color` → `TextBackgroundColor`.
    background: Option<Color>,
    /// `text-decoration` → `Underline`/`Strikethrough`.
    underline: bool,
    line_through: bool,
    decoration_color: Option<Color>,
    image: Option<ImageRun>,
    /// The `<img>`'s inline box size (see [`ImageRun::box_size`]).
    box_size: Option<Vec2>,
    /// The link this span belongs to (`<a href>`): the span becomes a
    /// picked entity carrying the bindings (ADR: links emit the `open`
    /// signal).
    link: Option<LinkRun>,
}

impl NodeSpec {
    fn new(node: Node) -> Self {
        Self {
            node,
            element: None,
            signals: Vec::new(),
            focus: None,
            custom: None,
            handle: None,
            // Overwritten by every construction site (`item_spec`'s arms
            // and the tests) with the element's computed style; this
            // placeholder only fills the field for the arms that set it
            // through the spread.
            style: Style {
                color: DEFAULT_COLOR,
                family: None,
                size: DEFAULT_FONT_SIZE,
                bold: false,
                italic: false,
                pointer_events: true,
                opacity: 1.0,
                align: TextAlignSpec::Left,
                line_height: LineHeightSpec::Normal,
                letter_spacing: 0.0,
                list_style: None,
                text_transform: TextTransformSpec::None,
                white_space: None,
            },
            pseudo_reactive: false,
            auto_scroll: [false; 2],
            background: None,
            image: None,
            border_color: None,
            z_index: None,
            outline: None,
            pickable: true,
            fixed: false,
            visibility: None,
            submit: false,
            font_size: DEFAULT_FONT_SIZE,
            #[cfg(feature = "text-input")]
            text_input: None,
            images: Vec::new(),
            calc: Vec::new(),
            transitions: Vec::new(),
            text: None,
            children: Vec::new(),
            window_key: None,
            spacer: None,
        }
    }
}

/// Which of an element's components its stylesheet set (and may therefore
/// take back on a restyle): `BorderColor`, `ZIndex`, `Outline`, `Pickable`
/// and `FixedNode` are also things an app sets itself, so a restyle without
/// the declaration resets only what CSS set. `BorderColor`/`ZIndex` are
/// `Node`'s required components: taking them back means resetting them to
/// their defaults.
#[derive(Component, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct CssOwned {
    border_color: bool,
    z_index: bool,
    outline: bool,
    pickable: bool,
    fixed: bool,
    visibility: bool,
}

/// The node's `overflow: auto` axes (ADR 0027): the build clips them and
/// [`promote_auto_scroll`] promotes them to scrolling when the laid-out
/// content overflows. CSS-owned like the `Node` fields: the spec writes
/// it and a restyle without the declaration takes it back.
#[derive(Component, Clone, Copy, Default, PartialEq, Debug)]
pub(crate) struct AutoScroll(pub [bool; 2]);

/// ADR 0027: `overflow: auto` scrolls if needed. The build clips the axis
/// (CSS's `hidden`); this system — running after layout, in
/// `UiSystems::PostLayout` — promotes it to scrolling when the laid-out
/// content overflows the box and demotes it when it doesn't. The write
/// re-layouts the next frame: the same one-frame model the calc resolver
/// uses (the promotion is stable — the content's size doesn't depend on
/// the axis's clip/scroll mode).
///
/// It runs on a `Node` or `AutoScroll` change too: an in-place restyle or
/// update rewrites the element's `Node` from its spec (the axis back to
/// `Hidden`) without changing the layout, so with `ComputedNode` alone a
/// restyled panel — a `:hover` one, say — stopped scrolling for good
/// (review_0005 I09). The axis is written only when it differs (a plain
/// `&mut` would mark the `Node` changed every run).
/// ADR 0031: after layout, record the materialized subtrees' outer
/// heights into the windowing model. A materialized subtree's row
/// updates its `(margin_top, outer)`; a culled one has no entity and
/// keeps its last measured height (corrections are sticky). The flags
/// aggregate up: a subtree holding a positioned (fixed/absolute) element
/// is never culled, and a container whose children the vertical cursor
/// can't track (out-of-flow or auto-margin children; wrap; row or grid
/// direction; non-start main-axis packing) culls nothing inside.
///
/// Heights are layout-frame-stale by one frame, like the calc resolver's
/// model; a new materialization measures on the frame it lands.
/// ADR 0031: after layout, record the materialized entities' outer
/// heights into the windowing model. Newly materialized entities (their
/// [`WindowKey`] was just added) and entities whose layout changed are
/// measured; a culled one has no entity and keeps its last measured
/// height. An unresolvable vertical margin or an out-of-flow
/// (fixed/absolute) entity marks every ancestor key's subtree
/// never-culled.
pub(crate) fn measure_window_heights(
    mut commands: Commands,
    mut models: Query<
        (
            Entity,
            &ComputedNode,
            Option<&'static mut WindowModel>,
            Has<HtmlWindowed>,
        ),
        With<HtmlUi>,
    >,
    // A row whose layout changed re-measures: the first build measures
    // before fonts load (the fallback metrics), and a kept entity is
    // never `Added` again — without this the model keeps the stale
    // heights and the band drifts off the viewport as the user scrolls.
    // `Changed<ComputedNode>` is O(the laid-out changes) per frame.
    added: Query<
        (Entity, &WindowKey, &Node, &ComputedNode, &ChildOf),
        // A kept entity re-keyed by a content change (its node's handle
        // moved) measures under its new key. Defense in depth: every
        // handle shift found so far also changes the entity's
        // `ComputedNode::stack_index` (review_0006's follow-up).
        Or<(Added<WindowKey>, Changed<WindowKey>, Changed<ComputedNode>)>,
    >,
    // Every keyed entity: a cleared model re-measures all of its UI's
    // built entities (a kept entity whose layout didn't change is neither
    // added nor changed — after a clear it was never measured again, and
    // the UI stopped culling for good; bug_0163).
    all_keyed: Query<(Entity, &WindowKey, &Node, &ComputedNode, &ChildOf)>,
    nodes: Query<(&Node, &ComputedNode, &ChildOf, Option<&WindowKey>)>,
    layouts: Query<&ComputedNode>,
) {
    // A new content width rewraps everything: the heights measured at the
    // old width are stale, built or culled. This pass anchors the new
    // width, so the build's own width check never fired once a
    // measurement ran after the resize (bug_0173): clear here, measure
    // every built entity now, and rebuild.
    for (_, computed, model, _) in &mut models {
        if let Some(mut model) = model
            && model.width > 0.0
            && (logical_content_width(computed) - model.width).abs() > 0.5
        {
            model.clear();
            model.changed = true;
        }
    }
    let remeasure = models
        .iter()
        .any(|(_, _, model, _)| model.is_some_and(|model| model.remeasure));
    // `(px, resolvable)`; an unresolvable vertical margin makes the
    // parent's children untrackable.
    let margin = |value: Val, basis: f32| -> (f32, bool) {
        match value {
            Val::Px(px) => (px, true),
            Val::Percent(percent) => (percent / 100.0 * basis, true),
            _ => (0.0, false),
        }
    };
    // `ComputedNode` sizes are physical pixels; the scroll position the
    // band is keyed by is logical — scale by the inverse factor (the
    // docs' own conversion) or a HiDPI display's model is its scale
    // times too tall and the band maps every scroll offset to rows
    // twice as deep (the viewport empties while scrolling).
    let content_width = logical_content_width;
    let candidates: Vec<(Entity, &WindowKey, &Node, &ComputedNode, &ChildOf)> = if remeasure {
        all_keyed.iter().collect()
    } else {
        added.iter().collect()
    };
    for (entity, key, node, computed, parent) in candidates {
        let parent = parent.0;
        // The entity's own UI: the nearest `HtmlUi` ancestor. Every level
        // measures into the root's model — looking the model up on the
        // parent found it only for the root's direct children, so nothing
        // inside a container was ever measured or culled (review_0006
        // J02), and the lookup's fallback put a stray `WindowModel` on
        // every row container.
        let mut root = parent;
        while !models.contains(root) {
            match nodes.get(root) {
                Ok((_, _, child_of, _)) => root = child_of.0,
                Err(_) => break,
            }
        }
        let Ok((_, _, model, windowed)) = models.get_mut(root) else {
            continue;
        };
        // Only a windowed UI keeps a model (every UI's entities carry
        // keys).
        if !windowed {
            continue;
        }
        let Some(mut model) = model else {
            // The build inserts it on the UI's first windowed frame;
            // measure next time.
            commands.entity(root).insert(WindowModel::default());
            continue;
        };
        // `layouts`, not `nodes`: the parent of a top-level element is the
        // `HtmlUi` root, which has no `ChildOf` — through `nodes` the
        // lookup failed and every top-level `%` margin measured as 0
        // (bug_0174: each spacer standing in for one was short by it).
        let parent_width = layouts.get(parent).map(content_width).unwrap_or(0.0);
        let positioned = node.position_type != PositionType::Relative;
        let (margin_top, top_ok) = margin(node.margin.top, parent_width);
        let (margin_bottom, bottom_ok) = margin(node.margin.bottom, parent_width);
        if positioned || !(top_ok && bottom_ok) {
            // Every ancestor key's subtree must refuse to cull: it can't
            // be replaced by a spacer without losing this entity.
            if let Ok((_, _, _, Some(WindowKey(own)))) = nodes.get(entity) {
                model.positioned.insert(*own);
            }
            let mut current = Some(parent);
            while let Some(entity) = current {
                if let Ok((_, _, child_of, Some(WindowKey(ancestor_key)))) = nodes.get(entity) {
                    model.positioned.insert(*ancestor_key);
                    current = Some(child_of.0);
                } else {
                    current = None;
                }
            }
            continue;
        }
        // The margins resolve in the same logical units the scroll
        // uses; only the measured size is physical (scaling the margins
        // too shrank them on a HiDPI display — review_0006 J03).
        let entry = (
            margin_top,
            // The unrounded size: the layout rounds each node, and a
            // spacer summing rounded parts drifted a pixel per few rows
            // below fractional (`%`) margins (bug_0176).
            margin_top + computed.unrounded_size.y * computed.inverse_scale_factor + margin_bottom,
        );
        let previous = model.heights.insert(key.0, entry);
        if previous.is_none_or(|(_, outer)| (outer - entry.1).abs() > 0.5) {
            model.changed = true;
        }
    }
    // The width anchor (a divergence clears the model, above and at the
    // build).
    for (_, computed, mut model, _) in &mut models {
        if let Some(model) = model.as_deref_mut() {
            model.width = content_width(computed);
            model.remeasure = false;
        }
    }
}

/// A node's content width in logical pixels (`ComputedNode` is physical;
/// the scroll offset and the CSS lengths the cursor adds are logical).
fn logical_content_width(computed: &ComputedNode) -> f32 {
    computed.inverse_scale_factor
        * (computed.size.x
            - computed.border.min_inset.x
            - computed.border.max_inset.x
            - computed.padding.min_inset.x
            - computed.padding.max_inset.x)
}

/// The scroll offset the root shows, in logical pixels: `ScrollPosition`
/// clamped to the last layout's scroll range. Bevy clamps only for its
/// layout and keeps the app's value, so a list that shrinks under a
/// bottom-scrolled view (or any overshoot) leaves `ScrollPosition` past
/// the content — a band placed by it culled the whole visible region.
fn visible_scroll(scroll: &ScrollPosition, computed: &ComputedNode) -> f32 {
    let scale = computed.inverse_scale_factor;
    // taffy's `Layout::scroll_height`, from the last layout's fields
    // (`content_size` is the scrollable overflow's bottom; the term is the
    // horizontal scrollbar's height, not the vertical one's width).
    let range = computed.content_size.y + computed.scrollbar_size.y.min(computed.size.y)
        - computed.size.y
        + computed.border.min_inset.y
        + computed.border.max_inset.y;
    scroll.y.clamp(0.0, range.max(0.0) * scale)
}

/// A scrolling root's visible region in logical pixels: the padding box's
/// height (what the viewport shows) and the top padding (the content box
/// starts that far below the visible top at scroll 0).
fn visible_region(computed: &ComputedNode) -> (f32, f32) {
    let scale = computed.inverse_scale_factor;
    let height = computed.size.y - computed.border.min_inset.y - computed.border.max_inset.y;
    (height * scale, computed.padding.min_inset.y * scale)
}

/// Whether the node's children are laid out in a way the vertical cursor
/// can't track: anything but a column (or a block) packed from the start
/// without wrapping — a grid, a row, a reversed direction (`row-reverse`,
/// `column-reverse` stack from the far end), wrapping, non-start packing.
fn children_are_opaque(node: &Node) -> bool {
    let stacked = match node.display {
        Display::Block => true,
        Display::Flex => node.flex_direction == FlexDirection::Column,
        _ => false,
    };
    !stacked
        || node.flex_wrap != FlexWrap::NoWrap
        || !matches!(
            node.justify_content,
            JustifyContent::Default | JustifyContent::FlexStart
        )
}

pub(crate) fn promote_auto_scroll(
    mut nodes: Query<
        (&mut Node, &ComputedNode, &AutoScroll),
        Or<(Changed<ComputedNode>, Changed<Node>, Changed<AutoScroll>)>,
    >,
) {
    for (mut node, computed, auto) in &mut nodes {
        for (axis, (size, content)) in [
            (0usize, (computed.size.x, computed.content_size.x)),
            (1usize, (computed.size.y, computed.content_size.y)),
        ] {
            if !auto.0[axis] {
                continue;
            }
            let want = if content > size {
                OverflowAxis::Scroll
            } else {
                OverflowAxis::Hidden
            };
            let current = match axis {
                0 => node.overflow.x,
                _ => node.overflow.y,
            };
            if current != want {
                match axis {
                    0 => node.overflow.x = want,
                    _ => node.overflow.y = want,
                }
            }
        }
    }
}

/// Sets a node's CSS-owned components; on a restyle, resets the ones CSS set
/// earlier and no longer declares. `transitions` (the element's declared
/// `transition`) animates the border color's change instead of writing it.
fn apply_css_owned(
    target: &mut EntityCommands,
    border_color: Option<BorderColor>,
    z_index: Option<ZIndex>,
    outline: Option<Outline>,
    pickable: bool,
    fixed: bool,
    visibility: Option<Visibility>,
    transitions: &[TransitionDecl],
) {
    let owned = CssOwned {
        border_color: border_color.is_some(),
        z_index: z_index.is_some(),
        outline: outline.is_some(),
        pickable: !pickable,
        fixed,
        visibility: visibility.is_some(),
    };
    let transitions = transitions.to_vec();
    target.queue(move |mut entity: EntityWorldMut| {
        let now = entity.world().resource::<Time>().elapsed_secs();
        let before = entity.get::<CssOwned>().copied().unwrap_or_default();
        // Written only when different (see `put`): unchanged values keep
        // their change ticks.
        match border_color {
            Some(color) => {
                if !transition_color(
                    &mut entity,
                    &transitions,
                    TransitionProp::BorderColor,
                    [color.top, color.right, color.bottom, color.left],
                    now,
                ) {
                    put(&mut entity, Some(color));
                }
            }
            None if before.border_color => put(&mut entity, Some(BorderColor::DEFAULT)),
            None => {}
        }
        match z_index {
            Some(z) => put(&mut entity, Some(z)),
            None if before.z_index => put(&mut entity, Some(ZIndex::default())),
            None => {}
        }
        match outline {
            Some(outline) => put(&mut entity, Some(outline)),
            None if before.outline => put::<Outline>(&mut entity, None),
            None => {}
        }
        if owned.pickable {
            put(&mut entity, Some(Pickable::IGNORE));
        } else if before.pickable {
            put::<Pickable>(&mut entity, None);
        }
        if owned.fixed {
            if !entity.contains::<FixedNode>() {
                entity.insert(FixedNode);
            }
        } else if before.fixed && entity.contains::<FixedNode>() {
            entity.remove::<FixedNode>();
        }
        match visibility {
            Some(visibility) => put(&mut entity, Some(visibility)),
            // `Visibility` is a `Node` required component: reset to CSS's
            // initial `visible`, never removed. (The root `HtmlUi` entity
            // never goes through here: `HtmlWorldAnchor` owns its visibility.)
            None if before.visibility => put(&mut entity, Some(Visibility::Visible)),
            None => {}
        }
        put(&mut entity, (owned != CssOwned::default()).then_some(owned));
    });
}

/// The element's current color for `property` (the animation's start: the
/// visual value, mid-animation included). `None` = not set (a spawn, or
/// another system owns it) — no transition.
fn read_color(entity: &EntityWorldMut, property: TransitionProp) -> Option<[Color; 4]> {
    match property {
        TransitionProp::TextColor => entity.get::<TextColor>().map(|color| [color.0; 4]),
        TransitionProp::BackgroundColor => {
            entity.get::<BackgroundColor>().map(|color| [color.0; 4])
        }
        TransitionProp::BorderColor => entity
            .get::<BorderColor>()
            .map(|color| [color.top, color.right, color.bottom, color.left]),
    }
}

/// Writes the color(s) for `property` (a no-op when unchanged, like `put`).
fn write_color(entity: &mut EntityWorldMut, property: TransitionProp, colors: [Color; 4]) {
    match property {
        TransitionProp::TextColor => put(entity, Some(TextColor(colors[0]))),
        TransitionProp::BackgroundColor => put(entity, Some(BackgroundColor(colors[0]))),
        TransitionProp::BorderColor => put(
            entity,
            Some(BorderColor {
                top: colors[0],
                right: colors[1],
                bottom: colors[2],
                left: colors[3],
            }),
        ),
    }
}

/// Writes `property`'s value, as a transition when the element declares one
/// and the value changes. `true` when this handled the write (the caller
/// skips its own). `now` is `Time`'s virtual seconds at the update.
fn transition_color(
    entity: &mut EntityWorldMut,
    transitions: &[TransitionDecl],
    property: TransitionProp,
    to: [Color; 4],
    now: f32,
) -> bool {
    let Some(decl) = transitions
        .iter()
        .rev()
        .find(|decl| decl.property == property)
    else {
        return false;
    };
    let Some(from) = read_color(entity, property) else {
        return false;
    };
    // One entry per property: a change during an animation replaces it,
    // starting from the current visual.
    let mut active = entity.get::<Transitions>().cloned().unwrap_or_default();
    active.0.retain(|entry| entry.property != property);
    if from == to {
        // The target is the current value: the entry ends now.
        if active.0.is_empty() {
            entity.remove::<Transitions>();
        } else {
            entity.insert(active);
        }
        write_color(entity, property, to);
        return true;
    }
    active.0.push(ActiveTransition {
        property,
        from,
        to,
        start: now,
        duration: decl.duration,
        delay: decl.delay,
        easing: decl.easing.clone(),
    });
    entity.insert(active);
    // The start value shows until the advance system's first write (later
    // in this frame, before render — the target is never drawn early).
    write_color(entity, property, from);
    true
}

/// Advances every in-flight transition (PostUpdate, after [`Build`], before
/// `UiSystems::Prepare`): interpolates from the recorded start value to the
/// target over the entry's duration and easing, and drops finished entries
/// (whose final write is the target — an idle component holds the value the
/// style system would have written).
///
/// [`Build`]: HtmlUiSystems::Build
#[hotpath::measure]
pub(crate) fn advance_transitions(
    time: Res<Time>,
    mut animated: Query<(
        &mut Transitions,
        Option<&mut TextColor>,
        Option<&mut BackgroundColor>,
        Option<&mut BorderColor>,
    )>,
) {
    let now = time.elapsed_secs();
    for (mut transitions, mut text, mut background, mut border) in &mut animated {
        let entries = std::mem::take(&mut transitions.0);
        let mut pending = Vec::new();
        for entry in entries {
            let progress = (now - entry.start - entry.delay) / entry.duration.max(f32::EPSILON);
            let done = progress >= 1.0;
            let value = if progress <= 0.0 {
                entry.from
            } else if done {
                entry.to
            } else {
                let t = eased(progress, &entry.easing);
                std::array::from_fn(|side| mix(entry.from[side], entry.to[side], t))
            };
            match entry.property {
                TransitionProp::TextColor => {
                    if let Some(color) = text.as_mut() {
                        if color.0 != value[0] {
                            color.0 = value[0];
                        }
                    }
                }
                TransitionProp::BackgroundColor => {
                    if let Some(color) = background.as_mut() {
                        if color.0 != value[0] {
                            color.0 = value[0];
                        }
                    }
                }
                TransitionProp::BorderColor => {
                    if let Some(color) = border.as_mut() {
                        let current = color.bypass_change_detection();
                        if current.top != value[0]
                            || current.right != value[1]
                            || current.bottom != value[2]
                            || current.left != value[3]
                        {
                            **color = BorderColor {
                                top: value[0],
                                right: value[1],
                                bottom: value[2],
                                left: value[3],
                            };
                        }
                    }
                }
            }
            if !done {
                pending.push(entry);
            }
        }
        transitions.0 = pending;
    }
}

/// The easing curve at input progress `t` (0..1): linear, the named
/// cubic-Bézier curves, a custom one, or the step functions.
fn eased(t: f32, easing: &EasingFunction) -> f32 {
    match easing {
        EasingFunction::Linear => t,
        EasingFunction::Ease => cubic_bezier(0.25, 0.1, 0.25, 1.0, t),
        EasingFunction::EaseIn => cubic_bezier(0.42, 0.0, 1.0, 1.0, t),
        EasingFunction::EaseOut => cubic_bezier(0.0, 0.0, 0.58, 1.0, t),
        EasingFunction::EaseInOut => cubic_bezier(0.42, 0.0, 0.58, 1.0, t),
        EasingFunction::CubicBezier { x1, y1, x2, y2, .. } => cubic_bezier(*x1, *y1, *x2, *y2, t),
        EasingFunction::Steps { count, position } => {
            let count = (*count).max(1) as f32;
            let value = match position {
                StepPosition::Start => f32::ceil(t * count) / count,
                StepPosition::End => f32::floor(t * count) / count,
                StepPosition::JumpBoth => (f32::floor(t * count) + 1.0) / (count + 1.0),
                StepPosition::JumpNone if count > 1.0 => {
                    f32::floor(t * count).clamp(0.0, count - 1.0) / (count - 1.0)
                }
                StepPosition::JumpNone => f32::floor(t * count) / count,
            };
            value.clamp(0.0, 1.0)
        }
    }
}

/// The cubic Bézier easing's y at input progress `t`: solves `x(s) = t` for
/// the parameter `s` (x is strictly monotone over CSS's allowed control
/// points) and evaluates `y(s)`. Bisection is plenty for a frame's
/// granularity; the endpoints are exact.
fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, t: f32) -> f32 {
    if t <= 0.0 {
        return 0.0;
    }
    if t >= 1.0 {
        return 1.0;
    }
    let at = |s: f32| {
        let u = 1.0 - s;
        3.0 * u * u * s * y1 + 3.0 * u * s * s * y2 + s * s * s
    };
    let x_at = |s: f32| {
        let u = 1.0 - s;
        3.0 * u * u * s * x1 + 3.0 * u * s * s * x2 + s * s * s
    };
    let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
    for _ in 0..24 {
        let mid = (lo + hi) / 2.0;
        if x_at(mid) < t {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    at((lo + hi) / 2.0)
}

/// sRGB per-channel lerp (CSS interpolates sRGB colors in sRGB, not
/// linear, and this crate has no premultiplied-alpha transitions).
fn mix(from: Color, to: Color, t: f32) -> Color {
    let (from, to) = (from.to_srgba(), to.to_srgba());
    Color::srgba(
        from.red + (to.red - from.red) * t,
        from.green + (to.green - from.green) * t,
        from.blue + (to.blue - from.blue) * t,
        from.alpha + (to.alpha - from.alpha) * t,
    )
}

/// Marks an element whose `ImageNode` is its CSS `border-image` frame, i.e.
/// owned by the pipeline. [`update_spec`] removes a frame only when it
/// carries this marker, so an `ImageNode` the app inserted on a built
/// element (an icon, say) is app state like any other component: updates
/// keep it.
#[derive(Component, PartialEq)]
pub(crate) struct CssFrame;

/// Spawns a block's inline children: one entity per run — a `TextSpan`
/// with its `TextBackgroundColor`/decoration components, or an inline
/// `<img>` (`InlineBox` + `InlineImage`; Bevy sizes the box from the image
/// when the attributes don't set one). Spans and images carry the block's
/// `pointer-events: none` (bevy_picking resolves text-section hits against
/// the span entity, so an ignored block must ignore its spans too — or the
/// ignored block stays clickable).
fn spawn_spans(spans: &mut ChildSpawnerCommands<'_>, text: &TextSpec, pickable: bool) {
    for span in &text.spans {
        let mut child = if let Some(image) = &span.image {
            let mut child = spans.spawn((
                InlineBox {
                    kind: InlineBoxKind::InFlow,
                    size: span.box_size.unwrap_or_default(),
                },
                InlineImage {
                    image: image.image.clone(),
                    width: image.width.and_then(ImageSize::px),
                    height: image.height.and_then(ImageSize::px),
                    ..default()
                },
            ));
            // A CSS-declared image is sized against its containing block
            // by the layout pass, not from its attributes.
            if image.width.is_some() || image.height.is_some() {
                child.insert(InlineImageSized {
                    width: image.width,
                    height: image.height,
                });
            }
            child
        } else {
            spans.spawn((
                TextSpan::new(span.text.clone()),
                span.font.clone(),
                TextColor(span.color),
            ))
        };
        // ADR 0027: bevy's measure reads the line height and letter
        // spacing from each *span* (`TextSpan` requires its own), so the
        // block's CSS values are copied onto every run.
        child.insert((text.line_height, text.letter_spacing));
        // A link span is a picked entity: bevy's per-section text picking
        // resolves clicks to it, and its bindings emit the signals (the
        // `open` click carries the href). `HtmlElement`/`DomNode` give it
        // hover state (`a:hover`) and element identity.
        if let Some(link) = &span.link {
            child.insert((
                ElementSignals(link.bindings.clone()),
                link.element.clone(),
                DomNode(link.handle),
            ));
            signals::observe_pointer_signals(&mut child);
        }
        if let Some(background) = span.background {
            child.insert(TextBackgroundColor(background));
        }
        if span.underline {
            child.insert(Underline);
        }
        if span.line_through {
            child.insert(Strikethrough);
        }
        // A declared decoration color applies to whichever lines exist
        // (`currentColor` otherwise).
        if let Some(color) = span.decoration_color {
            child.insert(UnderlineColor(color));
        }
        if let Some(color) = span.line_through.then_some(span.decoration_color).flatten() {
            child.insert(StrikethroughColor(color));
        }
        if !pickable {
            child.insert(Pickable::IGNORE);
        }
    }
}

/// The existing children's structure and identity, for [`update_children`].
type Tree<'w, 's> = Query<
    'w,
    's,
    (
        Option<&'static Children>,
        Has<Text>,
        Has<TextSpan>,
        Option<&'static HtmlElement>,
        Option<&'static ConnectedAs>,
        Has<CssFrame>,
        Has<HtmlUi>,
    ),
>;

/// The spec of an item and everything below it. Iterative (a frame per
/// open container: its partial spec, its remaining child items, the
/// child specs built so far), so the item tree's depth costs heap, not
/// stack.
/// ADR 0027: which of the layout declaration's overflow axes are `auto`.
fn auto_axes(layout: &LayoutDecl) -> [bool; 2] {
    [
        matches!(layout.overflow[0], Some(OverflowAxisDecl::Auto)),
        matches!(layout.overflow[1], Some(OverflowAxisDecl::Auto)),
    ]
}

/// ADR 0029: the item's own reactivity (its subtree's is folded in at
/// its construction; a cached item carries its spec's).
fn item_shell_reactive(item: &Item) -> bool {
    match item {
        Item::Spacer { .. } => false,
        Item::Block(block) => block.pseudo_reactive,
        Item::Container {
            pseudo_reactive, ..
        } => *pseudo_reactive,
        #[cfg(feature = "text-input")]
        Item::TextInput {
            pseudo_reactive, ..
        } => *pseudo_reactive,
        Item::Cached(spec) => spec.pseudo_reactive,
    }
}

fn item_spec(styler: &Styler, item: &Item, default_gap: Val) -> NodeSpec {
    struct Frame<'a> {
        spec: NodeSpec,
        items: std::slice::Iter<'a, Item>,
        children: Vec<SpecNode>,
    }
    impl<'a> Frame<'a> {
        fn new(spec: NodeSpec, items: &'a [Item]) -> Self {
            Self {
                spec,
                items: items.iter(),
                children: Vec::with_capacity(items.len()),
            }
        }
    }
    let (spec, items) = item_shell(styler, item, default_gap);
    let Some(items) = items else {
        return spec;
    };
    let mut stack = vec![Frame::new(spec, items)];
    loop {
        let top = stack.last_mut().expect("the root frame is popped last");
        match top.items.next() {
            Some(Item::Cached(spec)) => top.children.push(SpecNode::Cached(Arc::clone(spec))),
            Some(child) => match item_shell(styler, child, default_gap) {
                (spec, None) => top.children.push(SpecNode::Fresh(Arc::new(spec))),
                (spec, Some(items)) => stack.push(Frame::new(spec, items)),
            },
            None => {
                let Frame {
                    mut spec, children, ..
                } = stack.pop().expect("non-empty");
                spec.images = children
                    .iter()
                    .flat_map(|child| child.spec().images.iter().cloned())
                    .collect();
                spec.children = children;
                match stack.last_mut() {
                    Some(parent) => parent.children.push(SpecNode::Fresh(Arc::new(spec))),
                    None => return spec,
                }
            }
        }
    }
}

/// One item's own spec, without its children: a container's comes back
/// with its child items for [`item_spec`] to build (and its `children`
/// and `images` still empty).
fn item_shell<'a>(
    styler: &Styler,
    item: &'a Item,
    default_gap: Val,
) -> (NodeSpec, Option<&'a [Item]>) {
    if let Item::Spacer { height, .. } = item {
        // ADR 0031: an invisible fixed height in place of a culled
        // subtree. Not pickable, no element, no text; the height is the
        // model's (a fresh measurement re-records it as is — corrections
        // are sticky).
        let node = Node {
            flex_shrink: 0.0,
            height: Val::Px(*height),
            ..default()
        };
        return (
            NodeSpec {
                spacer: Some(*height),
                pickable: false,
                ..NodeSpec::new(node)
            },
            None,
        );
    }
    let (
        element,
        handle,
        signals,
        focus,
        custom,
        boxed,
        pointer_events,
        font_size,
        children,
        style,
        pseudo_reactive,
        auto_scroll,
        keyed,
    ) = match item {
        // A reused subtree's spec comes out by refcount in the caller;
        // this path only builds fresh items.
        Item::Cached(_) => unreachable!("a cached item never reaches item_spec"),
        Item::Spacer { .. } => unreachable!("handled before the tuple extraction"),
        Item::Block(block) => return (block_spec(styler, block), None),
        #[cfg(feature = "text-input")]
        Item::TextInput {
            element,
            handle,
            signals,
            focus,
            custom,
            boxed,
            pointer_events,
            font_size,
            style,
            pseudo_reactive,
            keyed,
            value,
        } => {
            // Returns directly: the other arms feed the shared tuple tail.
            let boxed = boxed.clone();
            let element = element.clone();
            let signals = signals.clone();
            let focus = focus.clone();
            let custom = custom.clone();
            let value = value.clone();
            let font_size = *font_size;
            let handle = *handle;
            let pointer_events = *pointer_events;
            let style = style.clone();
            let pseudo_reactive = *pseudo_reactive;
            let mut node = Node {
                flex_direction: FlexDirection::Column,
                flex_shrink: 0.0,
                box_sizing: BoxSizing::ContentBox,
                border: rect_over(UiRect::DEFAULT, boxed.border),
                padding: rect_over(UiRect::DEFAULT, boxed.padding),
                ..default()
            };
            boxed.layout.apply_to(&mut node);
            return (
                NodeSpec {
                    element: Some(element),
                    signals,
                    focus,
                    custom,
                    handle,
                    window_key: handle.filter(|_| *keyed),
                    border_color: boxed.border_color(),
                    background: boxed.background,
                    image: boxed.node_image(),
                    outline: boxed.drawn_outline,
                    z_index: boxed.z_index.map(ZIndex),
                    pickable: pointer_events,
                    fixed: boxed.layout.position == Some(CssPosition::Fixed),
                    visibility: boxed.layout.visibility.map(|visible| {
                        if visible {
                            Visibility::Visible
                        } else {
                            Visibility::Hidden
                        }
                    }),
                    font_size,
                    style,
                    pseudo_reactive,
                    auto_scroll: auto_axes(&boxed.layout),
                    #[cfg(feature = "text-input")]
                    text_input: Some(value),
                    children: Vec::new(),
                    calc: boxed.calc.clone(),
                    transitions: boxed.transitions.clone(),
                    ..NodeSpec::new(node)
                },
                None,
            );
        }
        Item::Container {
            element,
            handle,
            signals,
            focus,
            custom,
            boxed,
            pointer_events,
            font_size,
            style,
            pseudo_reactive,
            keyed,
            children,
        } => (
            element.clone(),
            *handle,
            signals.clone(),
            focus.clone(),
            custom.clone(),
            boxed.clone(),
            *pointer_events,
            *font_size,
            // Only iterated: borrowing avoids a deep copy of the whole
            // subtree per container.
            children,
            style.clone(),
            *pseudo_reactive,
            auto_axes(&boxed.layout),
            *keyed,
        ),
    };
    let mut node = Node {
        flex_direction: FlexDirection::Column,
        flex_shrink: 0.0,
        // CSS sizes the content box; Bevy defaults to the border box.
        box_sizing: BoxSizing::ContentBox,
        row_gap: boxed.row_gap.map_or(default_gap, Val::Px),
        border: rect_over(UiRect::DEFAULT, boxed.border),
        padding: rect_over(UiRect::DEFAULT, boxed.padding),
        ..default()
    };
    boxed.layout.apply_to(&mut node);
    let spec = NodeSpec {
        element: Some(element),
        signals,
        focus,
        custom,
        handle,
        window_key: handle.filter(|_| keyed),
        background: boxed.background,
        image: boxed.node_image(),
        border_color: boxed.border_color(),
        outline: boxed.drawn_outline,
        z_index: boxed.z_index.map(ZIndex),
        pickable: pointer_events,
        fixed: boxed.layout.position == Some(CssPosition::Fixed),
        visibility: boxed.layout.visibility.map(|visible| {
            if visible {
                Visibility::Visible
            } else {
                Visibility::Hidden
            }
        }),
        font_size,
        style,
        pseudo_reactive,
        auto_scroll,
        calc: boxed.calc.clone(),
        transitions: boxed.transitions.clone(),
        ..NodeSpec::new(node)
    };
    (spec, Some(children.as_slice()))
}

fn block_spec(styler: &Styler, block: &Block) -> NodeSpec {
    let boxed = block.boxed.clone();
    let indent = match block.kind {
        BlockKind::ListItem => UiRect::left(Val::Px(12.0)),
        _ => UiRect::DEFAULT,
    };
    // `pre`'s default padding; CSS padding replaces it per side.
    let padding = rect_over(
        match block.kind {
            BlockKind::Preformatted => UiRect::all(Val::Px(8.0)),
            _ => UiRect::DEFAULT,
        },
        boxed.padding,
    );
    // Blocks keep their height (`flex_shrink: 0`) so a scrolling parent
    // overflows instead of squashing them.
    let text_node = Node {
        flex_shrink: 0.0,
        box_sizing: BoxSizing::ContentBox,
        overflow: match block.kind {
            BlockKind::Preformatted => Overflow::clip_x(),
            _ => Overflow::DEFAULT,
        },
        ..default()
    };
    let text = TextSpec {
        prefix: block.marker.clone().unwrap_or_default(),
        font: styler.text_font(&block.style),
        color: faded(block.style.color, block.style.opacity),
        no_wrap: matches!(
            block.white_space,
            WhiteSpaceSpec::Preserve | WhiteSpaceSpec::NoWrap
        ),
        justify: match block.style.align {
            TextAlignSpec::Left => Justify::Left,
            TextAlignSpec::Right => Justify::Right,
            TextAlignSpec::Center => Justify::Center,
            TextAlignSpec::Justify => Justify::Justified,
        },
        line_height: match block.style.line_height {
            LineHeightSpec::Normal => LineHeight::RelativeToFont(1.2),
            LineHeightSpec::Relative(factor) => LineHeight::RelativeToFont(factor),
            LineHeightSpec::Px(px) => LineHeight::Px(px),
            LineHeightSpec::Rem(_) => {
                unreachable!("rem resolves at compute time (style_of)")
            }
        },
        letter_spacing: LetterSpacing::Px(block.style.letter_spacing),
        spans: block
            .runs
            .iter()
            .map(|run| {
                let color = faded(run.style.color, run.style.opacity);
                match &run.image {
                    Some(image) => SpanSpec {
                        text: String::new(),
                        font: TextFont::default(),
                        color,
                        background: None,
                        underline: false,
                        line_through: false,
                        decoration_color: None,
                        box_size: image.box_size(styler.images),
                        image: Some(image.clone()),
                        link: None,
                    },
                    None => SpanSpec {
                        text: run.text.clone(),
                        font: styler.text_font(&run.style),
                        color,
                        background: run.extras.background,
                        underline: run.extras.decoration.underline,
                        line_through: run.extras.decoration.line_through,
                        decoration_color: run.extras.decoration.color,
                        image: None,
                        box_size: None,
                        link: run.extras.link.clone(),
                    },
                }
            })
            .collect(),
    };
    // For the UI's `InlineImages`: their loads restyle the UI (sizing the
    // inline boxes).
    let images: Vec<Handle<Image>> = text
        .spans
        .iter()
        .filter_map(|span| span.image.as_ref())
        .map(|image| image.image.clone())
        .collect();

    if boxed.is_empty() {
        let mut node = Node {
            margin: indent,
            padding,
            ..text_node
        };
        boxed.layout.apply_to(&mut node);
        return NodeSpec {
            element: block.element.clone(),
            signals: block.signals.clone(),
            focus: block.focus.clone(),
            custom: block.custom.clone(),
            handle: block.handle,
            window_key: block.key,
            border_color: boxed.border_color(),
            outline: boxed.drawn_outline,
            z_index: boxed.z_index.map(ZIndex),
            pickable: block.style.pointer_events,
            fixed: boxed.layout.position == Some(CssPosition::Fixed),
            visibility: boxed.layout.visibility.map(|visible| {
                if visible {
                    Visibility::Visible
                } else {
                    Visibility::Hidden
                }
            }),
            submit: block.submit,
            font_size: block.style.size,
            style: block.style.clone(),
            pseudo_reactive: block.pseudo_reactive,
            auto_scroll: auto_axes(&boxed.layout),
            images,
            text: Some(text),
            calc: boxed.calc.clone(),
            transitions: boxed.transitions.clone(),
            ..NodeSpec::new(node)
        };
    }

    // Box properties go on a wrapper node: a node can't be both `Text` and
    // `ImageNode` (both size it from content). It's a column so the text
    // stretches to the content box and wraps there, like a CSS block (in a
    // row the non-shrinking text kept its max-content width and overflowed).
    let mut node = Node {
        flex_direction: FlexDirection::Column,
        flex_shrink: 0.0,
        box_sizing: BoxSizing::ContentBox,
        margin: indent,
        border: rect_over(UiRect::DEFAULT, boxed.border),
        padding,
        ..default()
    };
    boxed.layout.apply_to(&mut node);
    NodeSpec {
        element: block.element.clone(),
        signals: block.signals.clone(),
        focus: block.focus.clone(),
        custom: block.custom.clone(),
        handle: block.handle,
        window_key: block.key,
        background: boxed.background,
        image: boxed.node_image(),
        border_color: boxed.border_color(),
        outline: boxed.drawn_outline,
        z_index: boxed.z_index.map(ZIndex),
        pickable: block.style.pointer_events,
        fixed: boxed.layout.position == Some(CssPosition::Fixed),
        visibility: boxed.layout.visibility.map(|visible| {
            if visible {
                Visibility::Visible
            } else {
                Visibility::Hidden
            }
        }),
        submit: block.submit,
        font_size: block.style.size,
        style: block.style.clone(),
        pseudo_reactive: block.pseudo_reactive,
        auto_scroll: auto_axes(&boxed.layout),
        images,
        children: vec![SpecNode::Fresh(Arc::new(NodeSpec {
            pickable: block.style.pointer_events,
            // The inner text node's inherited base is the block's: its
            // runs styled from it, and it reacts when the block does
            // (ADR 0029's scoping reads both).
            style: block.style.clone(),
            pseudo_reactive: block.pseudo_reactive,
            text: Some(text),
            ..NodeSpec::new(text_node)
        }))],
        calc: boxed.calc.clone(),
        transitions: boxed.transitions.clone(),
        ..NodeSpec::new(node)
    }
}

/// The entity's DOM node: lets a restyle look up the element's
/// `:hover`/`:active` state.
#[derive(Component, Clone, Copy, Debug, Deref, PartialEq)]
pub(crate) struct DomNode(pub tl::NodeHandle);

/// Spawns `spec` under `parent`, collecting its custom elements (in
/// document order) for [`custom_elements::connect`]. Iterative: a BFS
/// spawn pass (children attach left-to-right, so `Children` order is
/// document order) plus a DFS pass for the custom elements, so the spec
/// tree's depth costs heap, not stack.
fn spawn_spec(
    commands: &mut Commands,
    parent: Entity,
    spec: &NodeSpec,
    root: Entity,
    connected: &mut Vec<ElementConnected>,
) -> Entity {
    /// A spawned node: its entity, its spec, and where its children sit
    /// in `spawned` (they spawn consecutively right after their parent
    /// is processed).
    struct Spawned<'a> {
        entity: Entity,
        spec: &'a NodeSpec,
        children: Vec<usize>,
    }
    // The BFS spawn pass: a node is configured and attached where it is
    // spawned; its children spawn consecutively (left to right, so
    // `Children` order is document order) and go to the back of the
    // queue.
    let mut spawned: Vec<Spawned> = Vec::new();
    let mut queue: std::collections::VecDeque<usize> = std::collections::VecDeque::new();
    let id = spawn_attached(commands, parent, spec);
    spawned.push(Spawned {
        entity: id,
        spec,
        children: Vec::new(),
    });
    queue.push_back(0);
    while let Some(index) = queue.pop_front() {
        let (entity, children) = (
            spawned[index].entity,
            spawned[index].spec.children.as_slice(),
        );
        spawned[index].children = children
            .iter()
            .map(|child| {
                let entity = spawn_attached(commands, entity, child.spec());
                spawned.push(Spawned {
                    entity,
                    spec: child.spec(),
                    children: Vec::new(),
                });
                spawned.len() - 1
            })
            .collect();
        queue.extend(spawned[index].children.iter().copied());
    }
    // The DFS pass: custom elements in document order (pre-order), which
    // is the order `custom_elements::connect` runs them in.
    let mut stack = vec![0usize];
    while let Some(index) = stack.pop() {
        let Spawned { entity, spec, .. } = &spawned[index];
        if let Some(custom) = &spec.custom {
            connected.push(ElementConnected {
                entity: *entity,
                root,
                name: custom.name.clone(),
                dataset: custom.dataset.clone(),
            });
        }
        for child in spawned[index].children.iter().rev() {
            stack.push(*child);
        }
    }
    id
}

/// Spawns one spec's node under `parent` (no children): the node, the
/// element components and, for a text block, its spans.
fn spawn_attached(commands: &mut Commands, parent: Entity, spec: &NodeSpec) -> Entity {
    let mut entity = commands.spawn((spec.node.clone(), ChildOf(parent)));
    // Non-text nodes get their `EmSize` from the cascade (text blocks have
    // a `TextFont`, which Bevy derives it from). CSS `em` lengths on the
    // node resolve against it. (Text inputs: `EditableText` requires a
    // `Node`, and their `EmSize` comes from their `TextFont`.)
    #[cfg(feature = "text-input")]
    let is_text_input = spec.text_input.is_some();
    #[cfg(not(feature = "text-input"))]
    let is_text_input = false;
    if spec.text.is_none() && !is_text_input {
        entity.insert(EmSize(spec.font_size));
    }
    let id = entity.id();
    #[cfg(feature = "text-input")]
    if is_text_input {
        // The editable text field (ADR 0016): the widget's editing behavior
        // keys off Bevy's `InputFocus` — the crate's focus (press-to-focus,
        // navigation, `HtmlModal`) drives it. The field's text styles from
        // the cascade via `TextFont`/`TextColor` (required by
        // `EditableText`), and the box from CSS on the node.
        let input = spec.text_input.clone().unwrap();
        let mut text = EditableText::new(input.initial.clone());
        text.max_characters = input.max_characters;
        entity.insert((
            text,
            InputValue(input.initial),
            input.font,
            TextColor(input.color),
            TextInput,
        ));
        if let Some(name) = input.name {
            entity.insert(signals::InputName(name));
        }
    }
    #[cfg(not(feature = "text-input"))]
    let _ = is_text_input;
    if spec.submit {
        entity.insert(signals::FormSubmitButton);
    }
    if !spec.calc.is_empty() {
        entity.insert(CalcSized {
            em: spec.font_size,
            exprs: spec.calc.clone(),
        });
    }
    if let Some(custom) = &spec.custom {
        entity.insert(ConnectedAs(custom.clone()));
    }
    if let Some(element) = &spec.element {
        entity.insert(element.clone());
    }
    if let Some(handle) = spec.handle {
        entity.insert(DomNode(handle));
    }
    if let Some(key) = spec.window_key {
        entity.insert(WindowKey(key));
    }
    if let Some(height) = spec.spacer {
        entity.insert(WindowSpacer(height));
    }
    if let Some(focusable) = spec.focus {
        entity.insert(focusable);
    }
    // A submit button has no bindings of its own, but its click submits
    // the enclosing form (`on_pointer` checks the marker).
    if !spec.signals.is_empty() || spec.submit {
        signals::observe_pointer_signals(&mut entity);
        entity.insert(ElementSignals(spec.signals.clone()));
    }
    if let Some(background) = spec.background {
        entity.insert(BackgroundColor(background));
    }
    if let Some(image) = &spec.image {
        entity.insert((image.clone(), CssFrame));
    }
    apply_css_owned(
        &mut entity,
        spec.border_color,
        spec.z_index,
        spec.outline,
        spec.pickable,
        spec.fixed,
        spec.visibility,
        // A spawn has no previous style: CSS transitions don't run.
        &[],
    );
    if spec.auto_scroll[0] || spec.auto_scroll[1] {
        entity.insert(AutoScroll(spec.auto_scroll));
    }
    if let Some(text) = &spec.text {
        entity.insert((
            Text::new(text.prefix.clone()),
            text.font.clone(),
            TextColor(text.color),
        ));
        entity.insert(text.line_height);
        entity.insert(text.letter_spacing);
        let layout = if text.no_wrap {
            TextLayout::no_wrap()
        } else {
            TextLayout::default()
        };
        entity.insert(layout.with_justify(text.justify));
        // bevy_picking resolves text-section hits against the span
        // entity, so the block's `pointer-events: none` must be on the
        // spans too (or the ignored block stays clickable).
        entity.with_children(|spans| {
            spawn_spans(spans, text, spec.pickable);
        });
    }
    id
}

/// Whether the existing child `entity` can become `spec` in place: the same
/// node kind (text block or not) and the same element (tag, `id`, and `is`
/// with its dataset). Spans and app-nested `HtmlUi` roots never match.
fn same_identity(spec: &NodeSpec, entity: Entity, tree: &Tree) -> bool {
    let Ok((_, has_text, has_span, element, connected, _, nested_ui)) = tree.get(entity) else {
        return false;
    };
    if nested_ui || has_span || has_text != spec.text.is_some() {
        return false;
    }
    let same_element = match (element, &spec.element) {
        (None, None) => true,
        (Some(old), Some(new)) => old.tag == new.tag && old.id == new.id,
        _ => false,
    };
    same_element && connected.map(|connected| &connected.0) == spec.custom.as_ref()
}

/// Updates `parent`'s pipeline-owned children to `specs`. An element with
/// an `id` unique among its old and its new siblings is matched by it, the
/// rest by position among themselves (like keyed list diffing; a duplicated
/// `id` can't key anything). A match with the same
/// identity ([`same_identity`]) is updated in place ([`update_spec`]) —
/// keeping its entity, its interaction state and whatever the app attached;
/// the rest are despawned and spawned, then everything is put in document
/// order. Children the app nested (`HtmlUi` roots) are kept, after the
/// pipeline's own. Returns whether anything below `parent` was spawned.
///
/// Iterative: a level's matching is a flat loop (`update_spec` returns the
/// children to process instead of recursing); a level's finalization
/// (despawn the stale, put the kept ones in order) runs after all the
/// deeper levels matched, deepest first.
#[hotpath::measure]
fn update_children(
    commands: &mut Commands,
    parent: Entity,
    specs: &[SpecNode],
    tree: &Tree,
    root: Entity,
    connected: &mut Vec<ElementConnected>,
    dom_nodes: &Query<'_, '_, (&DomNode, Option<&PseudoState>)>,
    pairing: Option<&HashMap<tl::NodeHandle, tl::NodeHandle>>,
    // ADR 0031: only a windowed UI replaces count-mismatched elements —
    // its band's children move every scroll; a static UI's counts never
    // change, and a mismatch there means positional pairing paired a
    // spec with a foreign entity, which must keep the old
    // skip-the-update behavior.
    windowed: bool,
) -> bool {
    /// A level waiting for its finalization (after all deeper levels
    /// matched).
    struct Pending {
        parent: Entity,
        owned: Vec<Entity>,
        nested: Vec<Entity>,
        current: Vec<Entity>,
        kept: HashSet<Entity>,
        desired: Vec<Entity>,
    }
    let mut spawned = false;
    let mut levels: Vec<Pending> = Vec::new();
    let mut work: Vec<(Entity, &[SpecNode])> = vec![(parent, specs)];
    while let Some((parent, specs)) = work.pop() {
        let current: Vec<Entity> = tree
            .get(parent)
            .ok()
            .and_then(|(children, ..)| children)
            .map_or_else(Vec::new, |children| children.to_vec());
        let (owned, nested): (Vec<Entity>, Vec<Entity>) = current
            .iter()
            .partition(|child| !tree.get(**child).is_ok_and(|(.., nested_ui)| nested_ui));
        let id_of = |entity: Entity| {
            tree.get(entity)
                .ok()
                .and_then(|(_, _, _, element, ..)| element?.id.clone())
        };
        let spec_id = |spec: &SpecNode| {
            spec.spec()
                .element
                .as_ref()
                .and_then(|element| element.id.clone())
        };
        let mut counts: HashMap<String, (usize, usize)> = HashMap::new();
        for id in owned.iter().filter_map(|&child| id_of(child)) {
            counts.entry(id).or_default().0 += 1;
        }
        for id in specs.iter().filter_map(spec_id) {
            counts.entry(id).or_default().1 += 1;
        }
        let key = |id: Option<String>| id.filter(|id| counts.get(id) == Some(&(1, 1)));
        let mut keyed: HashMap<String, Entity> = HashMap::new();
        let mut unkeyed = Vec::new();
        for &child in &owned {
            match key(id_of(child)) {
                Some(id) => {
                    keyed.insert(id, child);
                }
                None => unkeyed.push(child),
            }
        }
        let mut unkeyed = unkeyed.into_iter();
        let mut kept = HashSet::new();
        let mut desired = Vec::with_capacity(specs.len() + nested.len());
        for spec in specs {
            let candidate = match key(spec_id(spec)) {
                Some(id) => keyed.remove(&id),
                None => unkeyed.next(),
            };
            match candidate {
                Some(child) if same_identity(spec.spec(), child, tree) => {
                    // A reused spec may only skip when it landed on the entity
                    // it was built for: its content is what that entity already
                    // carries. Positional matching can hand it a foreign
                    // entity (an id-less list reordered); there the cached
                    // content — provably what a fresh build produces for this
                    // subtree — is written through `update_spec` like a fresh
                    // spec's.
                    let own_entity = match spec {
                        SpecNode::Cached(spec) => {
                            spec.handle
                                .and_then(|handle| pairing.and_then(|pairing| pairing.get(&handle)))
                                .copied()
                                == dom_nodes.get(child).ok().map(|(dom, _)| **dom)
                        }
                        SpecNode::Fresh(_) => false,
                    };
                    match (spec, own_entity) {
                        // ADR 0021 C-2: a reused subtree's entities already
                        // match its specs (every write since they were built
                        // compared equal, and the gates drop the reuse on any
                        // style-side change). Skip the visit; the one thing
                        // that changed across frames is the `DomNode` handle
                        // (earlier insertions shift the indices) — and the
                        // spec's handle was remapped to the fresh document.
                        (SpecNode::Cached(spec), true) => {
                            // The skip leaves the descendants' `DomNode`s at
                            // the handles they were spawned with; the state
                            // walk keys `:hover`/`:active` by the current
                            // document's handles, so refresh every level (the
                            // spec's handles were remapped up front) — but
                            // only when the root's handle actually moved
                            // (an unchanged document remaps every handle to
                            // itself; the per-element queue used to fire
                            // anyway).
                            let moved =
                                spec.handle != dom_nodes.get(child).ok().map(|(dom, _)| dom.0);
                            if let Some(handle) = spec.handle.filter(|_| moved) {
                                // The window key moves with the handle: a
                                // stale one is measured under a handle that
                                // now names another node (its height and
                                // opaque mark landed there — bug_0169).
                                let key = spec.window_key;
                                commands
                                    .entity(child)
                                    .queue(move |mut entity: EntityWorldMut| {
                                        put(&mut entity, Some(DomNode(handle)));
                                        put(&mut entity, key.map(WindowKey));
                                    });
                            }
                            if moved {
                                refresh_cached_handles(commands, child, spec, tree);
                            }
                        }
                        (spec, _) => {
                            // ADR 0031: a windowed band's children move as
                            // it scrolls, so a matched element's laid-out
                            // children often no longer count-match the
                            // spec's. `update_spec` reconciles in place
                            // only — when the counts diverge, replace the
                            // element (the app's nested UIs re-nest on
                            // `HtmlUiBuilt`, their contract).
                            let child_owned = tree
                                .get(child)
                                .ok()
                                .and_then(|(children, ..)| children)
                                .map(|children| {
                                    children
                                        .iter()
                                        .filter(|grand| {
                                            !tree.get(*grand).is_ok_and(|(.., nested)| nested)
                                        })
                                        .count()
                                })
                                .unwrap_or(0);
                            let expected_children = spec.spec().children.len()
                                // A boxed text block's entity owns one more
                                // child than the spec: its own text node.
                                + usize::from(spec.spec().text.is_some());
                            if windowed && child_owned != expected_children {
                                // Not kept: the level's finalization despawns
                                // it (recursively) as stale.
                                let id = spawn_spec(commands, parent, spec.spec(), root, connected);
                                desired.push(id);
                                spawned = true;
                                continue;
                            }
                            let (spawned_spans, children) =
                                update_spec(commands, child, spec.spec(), tree);
                            spawned |= spawned_spans;
                            if let Some(children) = children {
                                work.push((child, children));
                            }
                        }
                    }
                    kept.insert(child);
                    desired.push(child);
                }
                _ => {
                    let id = spawn_spec(commands, parent, spec.spec(), root, connected);
                    desired.push(id);
                    spawned = true;
                }
            }
        }
        levels.push(Pending {
            parent,
            owned,
            nested,
            current,
            kept,
            desired,
        });
    }
    // The finalizations run deepest first (a level pushed later is a
    // descendant of an earlier one): every level's children are matched
    // (and its stale grandchildren despawned) by the time it finalizes.
    for Pending {
        parent,
        owned,
        nested,
        current,
        kept,
        desired,
    } in levels.into_iter().rev()
    {
        for &stale in owned.iter().filter(|child| !kept.contains(*child)) {
            commands.entity(stale).despawn();
        }
        // The non-`owned` children are exactly the nested ones: membership in
        // the (small) nested set instead of a linear scan per current child.
        let nested_set: HashSet<Entity> = nested.iter().copied().collect();
        let desired = {
            let mut desired = desired;
            desired.extend(nested);
            desired
        };
        let remaining: Vec<Entity> = current
            .iter()
            .copied()
            .filter(|child| kept.contains(child) || nested_set.contains(child))
            .collect();
        if desired != remaining {
            commands.entity(parent).replace_children(&desired);
        }
    }
    spawned
}

/// Refreshes a reused (skipped) subtree's `DomNode`s below its root. The
/// entities keep the handles they were spawned with, but the state walk
/// (`:hover`/`:active`) keys on the *current* document's handles; the
/// spec's handles were already remapped (see `remap_spec_handles`), so
/// they are written through positionally — the subtree was spawned from
/// this spec tree and its content is unchanged. App-nested `HtmlUi` roots
/// are absent from the spec and keep their own state; a mismatching child
/// count bails (the next full visit repairs it).
#[hotpath::measure]
fn refresh_cached_handles(commands: &mut Commands, entity: Entity, spec: &NodeSpec, tree: &Tree) {
    let mut stack = vec![(entity, spec)];
    while let Some((entity, spec)) = stack.pop() {
        let Ok((children, .., nested_ui)) = tree.get(entity) else {
            continue;
        };
        if nested_ui {
            continue;
        }
        let Some(children) = children else {
            continue;
        };
        let entities: Vec<Entity> = children.iter().collect();
        if let Some(text) = &spec.text {
            // A text block's children are its spans, in order; only link
            // spans carry a `DomNode`.
            if entities.len() != text.spans.len() {
                continue;
            }
            for (&child, span) in entities.iter().zip(&text.spans) {
                let Some(link) = &span.link else {
                    continue;
                };
                let handle = link.handle;
                commands
                    .entity(child)
                    .queue(move |mut entity: EntityWorldMut| {
                        put(&mut entity, Some(DomNode(handle)));
                    });
            }
            continue;
        }
        // The spec owns the non-nested children in order: an app-nested
        // HtmlUi root sits among them (the count then disagreed and the
        // whole subtree's handles went stale).
        let owned: Vec<Entity> = entities
            .iter()
            .copied()
            .filter(|&child| !tree.get(child).is_ok_and(|(.., nested)| nested))
            .collect();
        if owned.len() != spec.children.len() {
            continue;
        }
        for (&child, child_spec) in owned.iter().zip(&spec.children) {
            let child_spec = child_spec.spec();
            if let Some(handle) = child_spec.handle {
                commands
                    .entity(child)
                    .queue(move |mut entity: EntityWorldMut| {
                        put(&mut entity, Some(DomNode(handle)));
                    });
            }
            if let Some(key) = child_spec.window_key {
                commands
                    .entity(child)
                    .queue(move |mut entity: EntityWorldMut| {
                        put(&mut entity, Some(WindowKey(key)));
                    });
            }
            if let Some(height) = child_spec.spacer {
                commands
                    .entity(child)
                    .queue(move |mut entity: EntityWorldMut| {
                        put(&mut entity, Some(WindowSpacer(height)));
                    });
            }
            stack.push((child, child_spec));
        }
    }
}

/// Updates `entity` (matched by [`same_identity`]) to `spec` in place: the
/// entity and anything the app attached stay; components the spec no longer
/// has are removed. Returns whether elements were spawned below it.
///
/// The spec is shared (a reused subtree's spec is the previous build's):
/// the queued commands capture it by refcount, and every write compares
/// against the current component *before* cloning (`put_ref`), so an
/// unchanged element — the common case for a reused subtree — is visited
/// without copying anything.
fn update_spec<'a>(
    commands: &mut Commands,
    entity: Entity,
    spec: &'a Arc<NodeSpec>,
    tree: &Tree,
) -> (bool, Option<&'a [SpecNode]>) {
    let (children, has_frame) = tree
        .get(entity)
        .map(|(children, _, _, _, _, has_frame, _)| {
            (
                children.map_or_else(Vec::new, |children| children.to_vec()),
                has_frame,
            )
        })
        .unwrap_or_default();
    let mut target = commands.entity(entity);
    // Only what differs is written: an unchanged component keeps its change
    // tick, so layout, text and style systems skip unchanged elements.
    let is_text = spec.text.is_some();
    // A submit button's click submits the enclosing form even though it
    // has no bindings of its own (`on_pointer` checks the marker).
    let submit = spec.submit;
    if !spec.signals.is_empty() || submit {
        signals::observe_pointer_signals(&mut target);
    }
    let auto_scroll = spec.auto_scroll;
    let queued = Arc::clone(spec);
    #[cfg(feature = "text-input")]
    let is_text_input = queued.text_input.is_some();
    #[cfg(not(feature = "text-input"))]
    let is_text_input = false;
    target.queue(move |mut entity: EntityWorldMut| {
        put_ref(&mut entity, Some(&queued.node));
        // ADR 0031: the windowing key and spacer follow the spec — a kept
        // entity from a build without a window (before the first layout)
        // gains its key here, or it would never be measured (review_0006).
        put(&mut entity, queued.window_key.map(WindowKey));
        put(&mut entity, queued.spacer.map(WindowSpacer));
        // Non-text nodes get their `EmSize` from the cascade (text blocks
        // have a `TextFont`, which Bevy derives it from; a text input's
        // `EmSize` comes from its `TextFont` too, as on spawn).
        if !is_text && !is_text_input {
            put(&mut entity, Some(EmSize(queued.font_size)));
        }
        put(
            &mut entity,
            (!queued.calc.is_empty()).then(|| CalcSized {
                em: queued.font_size,
                exprs: queued.calc.clone(),
            }),
        );
        // The editable text field's styling follows the cascade, and its
        // text is set only when the `value` attribute it was built from
        // changed (the text itself is app state: the user types into
        // `EditableText`).
        #[cfg(feature = "text-input")]
        if let Some(input) = &queued.text_input {
            put(&mut entity, Some(input.font.clone()));
            put(&mut entity, Some(TextColor(input.color)));
            let stored = entity.get::<InputValue>().map(|value| value.0.as_str());
            if stored != Some(input.initial.as_str()) {
                let mut text = EditableText::new(input.initial.clone());
                text.max_characters = input.max_characters;
                entity.insert((text, InputValue(input.initial.clone())));
            } else if entity.get::<EditableText>().map(|text| text.max_characters)
                != Some(input.max_characters)
            {
                // The maxlength attribute changed; the typed text is
                // app state and stays.
                entity.get_mut::<EditableText>().map(|mut text| {
                    text.max_characters = input.max_characters;
                });
            }
            match (&input.name, entity.get::<signals::InputName>()) {
                (Some(name), Some(current)) if current.0 == *name => {}
                (Some(name), _) => {
                    entity.insert(signals::InputName(name.clone()));
                }
                (None, Some(_)) => {
                    entity.remove::<signals::InputName>();
                }
                (None, None) => {}
            }
        }
        // An element that stopped being a text input doesn't exist (a
        // non-text `type` builds no item and the entity is despawned), so
        // there is no widget state to strip here - and a blanket remove
        // would strip components an app's custom element attached
        // (violating the "kept elements keep what the app attached"
        // promise). Nothing to do.
        if queued.element.is_some() {
            put_ref(&mut entity, queued.element.as_ref());
        }
        put(&mut entity, queued.handle.map(DomNode));
        put(&mut entity, queued.focus);
        put_ref(
            &mut entity,
            (!queued.signals.is_empty() || queued.submit)
                .then(|| ElementSignals(queued.signals.clone()))
                .as_ref(),
        );
        if queued.submit {
            entity.insert(signals::FormSubmitButton);
        } else {
            entity.remove::<signals::FormSubmitButton>();
        }
        let now = entity.world().resource::<Time>().elapsed_secs();
        match queued.background {
            Some(color) => {
                if !transition_color(
                    &mut entity,
                    &queued.transitions,
                    TransitionProp::BackgroundColor,
                    [color; 4],
                    now,
                ) {
                    put(&mut entity, Some(BackgroundColor(color)));
                }
            }
            None => put(&mut entity, None::<BackgroundColor>),
        }
        match &queued.image {
            // `ImageNode` has no `PartialEq`; frames are rare.
            Some(image) => {
                entity.insert((image.clone(), CssFrame));
            }
            None if has_frame => {
                entity.remove::<(ImageNode, CssFrame)>();
            }
            None => {}
        }
    });
    apply_css_owned(
        &mut target,
        spec.border_color,
        spec.z_index,
        spec.outline,
        spec.pickable,
        spec.fixed,
        spec.visibility,
        &spec.transitions,
    );
    let Some(text) = &spec.text else {
        // The children are the caller's next level (the update walk is
        // iterative; see `update_children`).
        return (false, Some(&spec.children));
    };
    // `TextLayout` is required by `Text`: replace, never remove.
    let layout = if text.no_wrap {
        TextLayout::no_wrap()
    } else {
        TextLayout::default()
    }
    .with_justify(text.justify);
    let (prefix, color) = (text.prefix.clone(), text.color);
    let font = text.font.clone();
    let line_height = text.line_height;
    let letter_spacing = text.letter_spacing;
    let text_transitions = spec.transitions.clone();
    target.queue(move |mut entity: EntityWorldMut| {
        put(&mut entity, Some(Text::new(prefix)));
        put_ref(&mut entity, Some(&font));
        let now = entity.world().resource::<Time>().elapsed_secs();
        if !transition_color(
            &mut entity,
            &text_transitions,
            TransitionProp::TextColor,
            [color; 4],
            now,
        ) {
            put(&mut entity, Some(TextColor(color)));
        }
        // `TextLayout` has no `PartialEq`: compare its fields.
        let same = entity.get::<TextLayout>().is_some_and(|current| {
            current.justify == layout.justify && current.linebreak == layout.linebreak
        });
        if !same {
            entity.insert(layout);
        }
        // ADR 0027: `line-height`/`letter-spacing` — replace, never
        // remove (both are `Text`'s required components, and CSS has no
        // "absent" state: `normal` is a value).
        put(&mut entity, Some(line_height));
        put(&mut entity, Some(letter_spacing));
    });
    // ADR 0027: the `overflow: auto` axes (the promotion system's marker);
    // a restyle without the declaration takes it back.
    target.queue(move |mut entity: EntityWorldMut| {
        put(
            &mut entity,
            (auto_scroll[0] || auto_scroll[1]).then_some(AutoScroll(auto_scroll)),
        );
    });
    // Spans carry the block's `pointer-events: none` (see
    // `spawn_spec`); an update without it takes it back. An image
    // span (an `InlineBox` child) can't become a `TextSpan` in
    // place: any kind mismatch respawns every span.
    let kinds_match = children.len() == text.spans.len()
        && children.iter().zip(&text.spans).all(|(child, span)| {
            tree.get(*child)
                .is_ok_and(|(_, _, is_span, ..)| is_span == span.image.is_none())
        });
    if kinds_match {
        let pickable = spec.pickable;
        for (index, span) in children.iter().enumerate() {
            // A span that becomes a link needs the click observers
            // (idempotent: the marker check skips existing ones).
            if text.spans[index].link.is_some() {
                signals::observe_pointer_signals(&mut commands.entity(*span));
            }
            let shared = Arc::clone(&spec);
            commands
                .entity(*span)
                .queue(move |mut span: EntityWorldMut| {
                    // The span's spec, read through the shared block spec.
                    let Some(spec) = shared.text.as_ref() else {
                        return;
                    };
                    // The block's values, before `spec` shadows into the
                    // span's spec (ADR 0027: copied onto every span).
                    let (spec_line_height, spec_letter_spacing) =
                        (spec.line_height, spec.letter_spacing);
                    let spec = &spec.spans[index];
                    if spec.image.is_none() {
                        let content = spec.text.as_str();
                        // `TextSpan` has no `PartialEq`: compare the text.
                        if span
                            .get::<TextSpan>()
                            .is_none_or(|current| **current != *content)
                        {
                            span.insert(TextSpan::new(content));
                        }
                        put_ref(&mut span, Some(&spec.font));
                        // A span that shared the block's color (the
                        // inherited case) follows the block's color
                        // transition, so runs fade with their block.
                        let old_color = span.get::<TextColor>().map(|color| color.0);
                        let followed = span
                            .get::<ChildOf>()
                            .and_then(|child_of| span.world().get::<Transitions>(child_of.0))
                            .and_then(|transitions| {
                                transitions
                                    .0
                                    .iter()
                                    .find(|entry| entry.property == TransitionProp::TextColor)
                                    .cloned()
                            });
                        match followed {
                            Some(entry)
                                if old_color == Some(entry.from[0])
                                    && entry.from[0] != entry.to[0] =>
                            {
                                let mut active =
                                    span.get::<Transitions>().cloned().unwrap_or_default();
                                active
                                    .0
                                    .retain(|entry| entry.property != TransitionProp::TextColor);
                                active.0.push(ActiveTransition {
                                    property: TransitionProp::TextColor,
                                    from: [old_color.unwrap(); 4],
                                    to: [spec.color; 4],
                                    start: entry.start,
                                    duration: entry.duration,
                                    delay: entry.delay,
                                    easing: entry.easing.clone(),
                                });
                                span.insert(active);
                                put(&mut span, Some(TextColor(old_color.unwrap())));
                            }
                            _ => put(&mut span, Some(TextColor(spec.color))),
                        }
                        // ADR 0027: the block's line height/letter spacing
                        // on every span (bevy's measure reads the span's
                        // own components); replace, never remove — both
                        // are `TextSpan` required components.
                        put(&mut span, Some(spec_line_height));
                        put(&mut span, Some(spec_letter_spacing));
                    } else {
                        let image = spec.image.as_ref().unwrap();
                        put(
                            &mut span,
                            Some(InlineBox {
                                kind: InlineBoxKind::InFlow,
                                size: spec.box_size.unwrap_or_default(),
                            }),
                        );
                        put(
                            &mut span,
                            Some(InlineImage {
                                image: image.image.clone(),
                                width: image.width.and_then(ImageSize::px),
                                height: image.height.and_then(ImageSize::px),
                                ..default()
                            }),
                        );
                        put(
                            &mut span,
                            (image.width.is_some() || image.height.is_some()).then(|| {
                                InlineImageSized {
                                    width: image.width,
                                    height: image.height,
                                }
                            }),
                        );
                    }
                    put_ref(&mut span, spec.background.map(TextBackgroundColor).as_ref());
                    put_marker::<Underline>(&mut span, spec.underline);
                    put_ref(
                        &mut span,
                        spec.decoration_color.map(UnderlineColor).as_ref(),
                    );
                    put_marker::<Strikethrough>(&mut span, spec.line_through);
                    put(
                        &mut span,
                        spec.line_through
                            .then_some(spec.decoration_color)
                            .flatten()
                            .map(StrikethroughColor),
                    );
                    // Link maintenance: bindings/identity on link
                    // spans, a clean removal when a run stops
                    // being a link.
                    match &spec.link {
                        Some(link) => {
                            put(&mut span, Some(ElementSignals(link.bindings.clone())));
                            put_ref(&mut span, Some(&link.element));
                            put(&mut span, Some(DomNode(link.handle)));
                        }
                        None => {
                            put::<ElementSignals>(&mut span, None);
                            put::<HtmlElement>(&mut span, None);
                            put::<DomNode>(&mut span, None);
                        }
                    }
                    put(&mut span, (!pickable).then_some(Pickable::IGNORE));
                });
        }
    } else {
        for span in children {
            commands.entity(span).despawn();
        }
        commands.entity(entity).with_children(|spans| {
            spawn_spans(spans, text, spec.pickable);
        });
        // New span entities: the restyle's "everything was kept" promise
        // no longer holds — report a build so `HtmlUiBuilt` fires and
        // observers re-wire the replacements (link spans, in particular).
        return (true, None);
    }
    (false, None)
}

/// Resolves `calc()`-valued layout properties per frame: `em`/`rem`
/// against the font sizes, `%` against the containing block's content box
/// per axis, viewport units against the UI root's size. Writes the
/// concrete pixels into the node's fields (the values keep their
/// placeholders until the first resolution; later builds overwrite the
/// fresh spec's placeholder each frame until the values are unchanged).
#[hotpath::measure]
pub(crate) fn resolve_calc(
    mut sized: Query<(Entity, Option<&ChildOf>, &mut Node, &CalcSized)>,
    boxes: Query<&ComputedNode>,
    parents: Query<&ChildOf>,
    rem: Res<RemSize>,
    cameras: Query<&Camera>,
    default_camera: DefaultUiCamera,
    target_cameras: Query<&UiTargetCamera>,
) {
    // The viewport of a UI tree: the camera its top-level root targets
    // (`UiTargetCamera`), else the default UI camera (`IsDefaultUiCamera`,
    // else the window camera Bevy UI renders with — not the last camera
    // spawned, review_0003 G10). A root's own box is *not* the viewport:
    // an auto-sized root shrinks to its content, and a tooltip or HUD
    // root is smaller than the window (review_0004 H08). Without any
    // camera (a headless edge) the root's own box is the only reference.
    let viewport_of = |top: Entity| {
        target_cameras
            .get(top)
            .map(|target| target.0)
            .ok()
            .or_else(|| default_camera.get())
            .and_then(|camera| cameras.get(camera).ok())
            .and_then(Camera::logical_viewport_size)
            .or_else(|| boxes.get(top).ok().map(|box_| box_.size))
            .unwrap_or_default()
    };
    for (entity, child_of, mut node, calc) in &mut sized {
        let (content, viewport) = match child_of {
            Some(child_of) => {
                let parent = child_of.parent();
                let Ok(parent_box) = boxes.get(parent) else {
                    continue;
                };
                let inset = parent_box.border + parent_box.padding;
                let content = Vec2::new(
                    (parent_box.size.x - inset.min_inset.x - inset.max_inset.x).max(0.0),
                    (parent_box.size.y - inset.min_inset.y - inset.max_inset.y).max(0.0),
                );
                // The tree's top-level root (the parent itself for a
                // root's direct child — requiring a grandparent skipped
                // those every frame, review_0004 H07); nested `HtmlUi`
                // roots are walked past.
                let mut top = parent;
                while let Ok(ancestor) = parents.get(top).map(|p| p.0) {
                    top = ancestor;
                }
                (content, viewport_of(top))
            }
            // A top-level `HtmlUi` root has no parent: its containing
            // block is the viewport.
            None => {
                let size = viewport_of(entity);
                (size, size)
            }
        };
        let basis = CalcBasis {
            em: calc.em,
            rem: rem.0,
            viewport,
            content,
        };
        for (prop, expr) in &calc.exprs {
            let Some(value) = expr.resolve(&basis, prop.horizontal()) else {
                continue;
            };
            let value = Val::Px(value);
            // Compare through the immutable deref first: taking `&mut`
            // through a `Mut<Node>` marks it changed even for an equal
            // write, and bevy_ui re-upserts every changed node into
            // taffy — every element using `calc()` dirtied layout every
            // frame (review_0003 G15).
            let unchanged = match prop {
                CalcProp::Width => node.width == value,
                CalcProp::Height => node.height == value,
                CalcProp::MinWidth => node.min_width == value,
                CalcProp::MinHeight => node.min_height == value,
                CalcProp::MaxWidth => node.max_width == value,
                CalcProp::MaxHeight => node.max_height == value,
                CalcProp::FlexBasis => node.flex_basis == value,
                CalcProp::MarginTop => node.margin.top == value,
                CalcProp::MarginRight => node.margin.right == value,
                CalcProp::MarginBottom => node.margin.bottom == value,
                CalcProp::MarginLeft => node.margin.left == value,
                CalcProp::Left => node.left == value,
                CalcProp::Top => node.top == value,
                CalcProp::Right => node.right == value,
                CalcProp::Bottom => node.bottom == value,
            };
            if !unchanged {
                *node_field_mut(&mut node, *prop) = value;
            }
        }
    }
}

/// The borrow-and-compare form of [`put`]: the value is only cloned when it
/// differs, so a shared spec's unchanged component costs one comparison.
fn put_ref<C: Component<Mutability = bevy::ecs::component::Mutable> + PartialEq + Clone>(
    entity: &mut EntityWorldMut,
    value: Option<&C>,
) {
    match (value, entity.get_mut::<C>()) {
        (Some(value), Some(mut current)) => {
            if *current != *value {
                *current = C::clone(value);
            }
        }
        (Some(value), None) => {
            entity.insert(C::clone(value));
        }
        (None, Some(_)) => {
            entity.remove::<C>();
        }
        (None, None) => {}
    }
}

fn put<C: Component<Mutability = bevy::ecs::component::Mutable> + PartialEq>(
    entity: &mut EntityWorldMut,
    value: Option<C>,
) {
    match (value, entity.get_mut::<C>()) {
        (Some(value), Some(mut current)) => {
            current.set_if_neq(value);
        }
        (Some(value), None) => {
            entity.insert(value);
        }
        (None, Some(_)) => {
            entity.remove::<C>();
        }
        (None, None) => {}
    }
}

/// Sets or removes a marker component: an unchanged marker keeps its change
/// tick. (Markers like `Underline` have no `PartialEq` for [`put`].)
fn put_marker<C: Component + Default>(entity: &mut EntityWorldMut, set: bool) {
    if set == entity.contains::<C>() {
        return;
    }
    if set {
        entity.insert(C::default());
    } else {
        entity.remove::<C>();
    }
}

/// Applies the root rule to the `HtmlUi` entity itself: its box
/// (`border-image`, `border-width`, `padding`, `gap`), layout (flex/grid,
/// sizes, margins, `position` and insets, `border-radius`) on the `Node`,
/// and `background-color`, `border-color`, `z-index` (`ZIndex`: among
/// sibling roots too, as Bevy sorts roots by `GlobalZIndex` then `ZIndex`)
/// and `pointer-events: none` as components. What it no longer declares goes
/// back to the app's value ([`CssRoot`]).
#[hotpath::measure]
fn apply_root(entity: &mut EntityWorldMut, boxed: BoxStyle, pointer_events: bool, root_size: f32) {
    let state = entity.take::<CssRoot>();
    let Some(current) = entity.get::<Node>().cloned() else {
        return;
    };
    // Back to the app's values, then the new declarations over them.
    let mut base = current.clone();
    if let Some(state) = &state {
        state.layout.restore_from(&state.base, &mut base);
        restore_sides(&mut base.border, state.base.border, state.border);
        restore_sides(&mut base.padding, state.base.padding, state.padding);
        if state.row_gap.is_some() {
            base.row_gap = state.base.row_gap;
        }
        // A calc()'d property's field holds the last resolution, not a
        // declaration: when the new style drops the calc, restore the
        // app's own value.
        for (prop, _) in &state.calc {
            let value = match prop {
                CalcProp::Width => state.base.width.clone(),
                CalcProp::Height => state.base.height.clone(),
                CalcProp::MinWidth => state.base.min_width.clone(),
                CalcProp::MinHeight => state.base.min_height.clone(),
                CalcProp::MaxWidth => state.base.max_width.clone(),
                CalcProp::MaxHeight => state.base.max_height.clone(),
                CalcProp::FlexBasis => state.base.flex_basis.clone(),
                CalcProp::MarginTop => state.base.margin.top.clone(),
                CalcProp::MarginRight => state.base.margin.right.clone(),
                CalcProp::MarginBottom => state.base.margin.bottom.clone(),
                CalcProp::MarginLeft => state.base.margin.left.clone(),
                CalcProp::Left => state.base.left.clone(),
                CalcProp::Top => state.base.top.clone(),
                CalcProp::Right => state.base.right.clone(),
                CalcProp::Bottom => state.base.bottom.clone(),
            };
            *node_field_mut(&mut base, *prop) = value;
        }
    }
    let mut node = base.clone();
    boxed.layout.apply_to(&mut node);
    // CSS: `min-width`/`min-height: auto` compute to zero for the root (the
    // automatic minimum is a flex-*item* rule) — content overflows the
    // root, it doesn't size it. Taffy 0.9 applies the content minimum to
    // root nodes too, which would grow the root to its content instead of
    // letting the window clip it (a Bevy 0.19 → 0.20 behavior change; the
    // scrolling-panel recipe depends on it). Zero it unless the root rule
    // declares a minimum.
    if boxed.layout.min_width.is_none() {
        node.min_width = Val::Px(0.0);
    }
    if boxed.layout.min_height.is_none() {
        node.min_height = Val::Px(0.0);
    }
    node.border = rect_over(node.border, boxed.border);
    node.padding = rect_over(node.padding, boxed.padding);
    if let Some(gap) = boxed.row_gap {
        node.row_gap = Val::Px(gap);
    }
    if node != current {
        entity.insert(node);
    }
    // The root's font-size in concrete form: CSS `em` lengths on the root
    // resolve against it. (The root has no `TextFont`, so Bevy wouldn't
    // derive one.)
    entity.insert(EmSize(root_size));

    let mut owned = state.map(|state| state.owned).unwrap_or_default();
    claim(
        entity,
        boxed.background.map(BackgroundColor),
        &mut owned.background,
    );
    claim(entity, boxed.border_color(), &mut owned.border_color);
    claim(entity, boxed.z_index.map(ZIndex), &mut owned.z_index);
    claim(
        entity,
        (!pointer_events).then_some(Pickable::IGNORE),
        &mut owned.pickable,
    );
    claim(
        entity,
        (boxed.layout.position == Some(CssPosition::Fixed)).then_some(FixedNode),
        &mut owned.fixed,
    );
    claim(entity, boxed.node_image(), &mut owned.image);

    let declares_node = boxed
        .border
        .iter()
        .chain(&boxed.padding)
        .any(Option::is_some)
        || boxed.row_gap.is_some()
        || !boxed.calc.is_empty()
        || boxed.layout != LayoutDecl::default();
    let owns_component = owned.background.is_some()
        || owned.border_color.is_some()
        || owned.z_index.is_some()
        || owned.pickable.is_some()
        || owned.fixed.is_some()
        || owned.image.is_some();
    if declares_node || owns_component {
        entity.insert(CssRoot {
            base,
            layout: boxed.layout,
            border: boxed.border,
            padding: boxed.padding,
            row_gap: boxed.row_gap,
            calc: boxed.calc.clone(),
            owned,
        });
    }
    // The root's own calc()'d properties: the pass resolves them per frame.
    if !boxed.calc.is_empty() {
        entity.insert(CalcSized {
            em: root_size,
            exprs: boxed.calc.clone(),
        });
    } else {
        // The declaration went: the old calc must stop resolving.
        entity.remove::<CalcSized>();
    }
}

/// Undoes [`rect_over`]: the `declared` sides of `rect` back to `base`'s.
fn restore_sides(rect: &mut UiRect, base: UiRect, declared: [Option<f32>; 4]) {
    let sides = [
        (&mut rect.top, base.top),
        (&mut rect.right, base.right),
        (&mut rect.bottom, base.bottom),
        (&mut rect.left, base.left),
    ];
    for ((side, base), declared) in sides.into_iter().zip(declared) {
        if declared.is_some() {
            *side = base;
        }
    }
}

/// Sets `css` on `entity` while declared, remembering the app's value in
/// `owned` the first time; when no longer declared, gives the app's value
/// back (or removes the component if the app had none).
fn claim<C: Component<Mutability = bevy::ecs::component::Mutable> + Clone>(
    entity: &mut EntityWorldMut,
    css: Option<C>,
    owned: &mut Option<Option<C>>,
) {
    match css {
        Some(value) => {
            if owned.is_none() {
                *owned = Some(entity.get::<C>().cloned());
            }
            entity.insert(value);
        }
        None => match owned.take() {
            Some(Some(app)) => {
                entity.insert(app);
            }
            Some(None) => {
                entity.remove::<C>();
            }
            None => {}
        },
    }
}

/// Walk context shared by every node.
struct Ctx<'a, 'p, 'buf> {
    parser: &'p tl::Parser<'buf>,
    /// Translations by node of `parser`'s document (empty inside a
    /// translation: its markup isn't localized again).
    localized: &'a HashMap<tl::NodeHandle, Result<String, String>>,
    /// The elements' `:hover`/`:active` state by DOM node (`None` inside a
    /// translation: fragment handles index the fragment, not the document).
    states: Option<&'a HashMap<tl::NodeHandle, Pseudo>>,
    /// The translated fragments' element states, keyed by the translated
    /// element's own DOM handle: a fragment re-parses per build, so its
    /// spans' `DomNode` handles index the fragment, not the document.
    fragments: Option<&'a HashMap<tl::NodeHandle, HashMap<tl::NodeHandle, Pseudo>>>,
    styler: &'a Styler<'a>,
    /// Inside a translation: the source element's `data-l10n-name`
    /// descendants, each usable once (fluent-dom's named overlays).
    named: Option<&'a RefCell<Vec<(String, HtmlElement)>>>,
    /// ADR 0021: the previous build's tree, when this build may reuse its
    /// clean subtrees. `None` inside a translation (fragments are rebuilt).
    skeleton: Option<&'a SkeletonPass<'a>>,
    /// ADR 0028: the handles holding or containing the focus
    /// (`:focus-within`). Empty inside a translation.
    focus_within: &'a HashSet<tl::NodeHandle>,
    /// The `var(--name)` names every parsed inline `style` references
    /// (the main walk's accumulator; fragments create their own and
    /// discard it — translations are rebuilt every frame, so their
    /// inline vars are never stale). Feeds the frame's vars hash: a
    /// context change must restyle elements whose *inline* styles use
    /// the variable.
    inline_vars: std::cell::RefCell<std::collections::BTreeSet<String>>,
    /// ADR 0031: the windowing state — `None` inside a translation
    /// (translated subtrees are never culled; their items come from a
    /// per-build fragment whose heights aren't in the model).
    window: Option<WindowCtx<'a>>,
}

/// ADR 0031: the height model of a windowed `HtmlUi` — per DOM handle of
/// the *current* document, the subtree's resolved vertical margin and
/// outer height as last measured (materialized subtrees update it; culled
/// ones keep theirs — corrections are sticky), plus the flags the cull
/// decisions read. Rebuilt into the fresh document's handle space by the
/// build's pairing remap; cleared when the widths diverge or a document
/// changes without a pairing.
#[doc(hidden)]
#[derive(Component, Default)]
pub struct WindowModel {
    /// `(margin_top, outer_height)` per DOM handle.
    heights: HashMap<tl::NodeHandle, (f32, f32)>,
    /// The subtree holds a positioned (fixed/absolute) element: never
    /// culled (it would vanish with the subtree).
    positioned: HashSet<tl::NodeHandle>,
    /// The root's content width the heights were measured at: a
    /// divergence clears the model (wrapping changes heights).
    width: f32,
    /// The scroll offset the last build windowed at (`None`: it windowed
    /// nothing — no layout yet).
    anchor: Option<f32>,
    /// How far the offset may move down (`.0`) and up (`.1`) from the
    /// anchor before some element crosses a band edge: inside it the band
    /// is the same, so a scroll there rebuilds nothing (review_0006 J05).
    slack: (f32, f32),
    /// The visible height the last build windowed for.
    viewport: f32,
    /// The model was cleared (or a remap dropped heights): the next
    /// measurement visits every built entity of the UI, not just the added
    /// or re-laid-out ones (bug_0163, bug_0166).
    remeasure: bool,
    /// The measurement learned a new or different height since the last
    /// build: the band may cull differently, so the UI rebuilds once. The
    /// first build materializes everything (no heights yet); without this
    /// nothing culled until the user scrolled (review_0006 J08).
    changed: bool,
}

impl WindowModel {
    /// The model's health for the testbench's probe: `(width, entries,
    /// total height)`. Not covered by semver.
    #[doc(hidden)]
    pub fn probe(&self) -> (f32, usize, f32) {
        (
            self.width,
            self.heights.len(),
            self.heights.values().map(|(_, outer)| outer).sum(),
        )
    }

    fn remap(&mut self, pairing: &HashMap<tl::NodeHandle, tl::NodeHandle>) {
        let remap = |map: &mut HashMap<tl::NodeHandle, (f32, f32)>| {
            let old = std::mem::take(map);
            *map = old
                .into_iter()
                .filter_map(|(handle, entry)| pairing.get(&handle).map(|&fresh| (fresh, entry)))
                .collect();
        };
        let before = self.heights.len();
        remap(&mut self.heights);
        // A dropped height (the pairing doesn't reach a changed
        // container's children) belongs to a kept entity whose key may be
        // unchanged — its handle didn't shift — and whose size didn't
        // change: no `Changed` filter revisits it, and the frame it sits
        // in never culls again (bug_0166). Re-measure every keyed entity.
        if self.heights.len() < before {
            self.remeasure = true;
        }
        let rekey = |set: &mut HashSet<tl::NodeHandle>| {
            let old = std::mem::take(set);
            *set = old
                .into_iter()
                .filter_map(|handle| pairing.get(&handle).copied())
                .collect();
        };
        rekey(&mut self.positioned);
    }

    fn clear(&mut self) {
        self.heights.clear();
        self.positioned.clear();
        self.width = 0.0;
        self.changed = false;
        self.remeasure = true;
    }
}

/// The DOM handle a materialized entity's heights are keyed by (ADR
/// 0031): the element's own, or a raw text node's for its anonymous
/// block — carried separately from [`DomNode`], which stays element-only.
#[doc(hidden)]
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct WindowKey(pub tl::NodeHandle);

/// The entity is a windowing spacer standing in for a culled subtree;
/// the height is the model's (ADR 0031). Read by the dump.
#[doc(hidden)]
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct WindowSpacer(pub f32);

/// ADR 0031: one container's vertical cursor while collecting.
///
/// Not `Clone`: its cells are the walk's shared state — a write to a copy
/// is lost (an unknown height didn't invalidate a nested frame,
/// bug_0165).
struct CursorFrame {
    /// The next child's margin-box top, in the root's content
    /// coordinates.
    y: std::cell::Cell<f32>,
    /// The container's resolved row gap, in px.
    gap: f32,
    /// The cursor tracks real positions (an unknown height, a wrap, a
    /// non-column main axis or a non-px margin invalidates it from
    /// there on; invalidated cursors cull nothing).
    valid: std::cell::Cell<bool>,
}

/// ADR 0031: the walk's windowing state: the model's heights against the
/// band the root's scroll exposes.
struct WindowCtx<'a> {
    model: &'a WindowModel,
    /// The materialized band in content coordinates.
    band: (f32, f32),
    /// One frame per culleable container, in collection order; tasks
    /// reference their container's frame by index.
    frames: std::cell::RefCell<Vec<CursorFrame>>,
    /// Nested containers without their own `row-gap` space their children
    /// like the root does (the crate's default gap), in px.
    default_gap: f32,
    /// The offset slack the cull decisions allow (down, up): see
    /// [`WindowModel::slack`].
    slack: &'a (std::cell::Cell<f32>, std::cell::Cell<f32>),
    /// A cull was refused for a stale height ([`Ctx::cull`]): rebuild once.
    stale: &'a std::cell::Cell<bool>,
}

impl WindowCtx<'_> {
    /// The subtree `key` stands outside the band when its margin box at
    /// `y` doesn't intersect it: culled, replaced by its measured height.
    /// Culled: the spacer item standing in for the subtree (its measured
    /// outer height, and its column's gap for coalescing).
    fn culled(&self, frame: usize, y: f32, key: tl::NodeHandle) -> Option<Item> {
        let (valid, gap) = self
            .frames
            .borrow()
            .get(frame)
            .map(|frame| (frame.valid.get(), frame.gap))?;
        if !valid {
            return None;
        }
        let &(_, outer) = self.model.heights.get(&key)?;
        if self.model.positioned.contains(&key) {
            return None;
        }
        let (top, bottom) = (y, y + outer);
        let culled = bottom <= self.band.0 || top >= self.band.1;
        // How far the band may move (strictly more than the slack flips
        // a decision) before this one flips: moving down by `d` shifts
        // both edges by `d`. A culled element materializes only past its
        // distance; a materialized one culls *at* it, hence the epsilon.
        const EDGE: f32 = 1e-3;
        if bottom <= self.band.0 {
            self.narrow(f32::INFINITY, self.band.0 - bottom);
        } else if top >= self.band.1 {
            self.narrow(top - self.band.1, f32::INFINITY);
        } else {
            self.narrow(bottom - self.band.0 - EDGE, self.band.1 - top - EDGE);
        }
        culled.then_some(Item::Spacer { height: outer, gap })
    }

    fn narrow(&self, down: f32, up: f32) {
        self.slack.0.set(self.slack.0.get().min(down.max(0.0)));
        self.slack.1.set(self.slack.1.get().min(up.max(0.0)));
    }

    /// A subtree reused whole (it lies inside the band): its descendants
    /// weren't visited, so the slack keeps the band clear of its top and
    /// bottom — conservative (a child culls no earlier than its parent's
    /// top leaves the band). Without it the worst case is bounded
    /// over-building (rows past the band stay built until the next
    /// rebuild), never a missing visible row. It binds only when nothing
    /// sits between the band's edge and the subtree in its column (a
    /// neighbour there is tighter by one gap) — e.g. the first child of a
    /// container whose padding holds the edge
    /// (`a_reused_section_culls_its_rows_once_they_leave_the_band`).
    fn note_reused(&self, frame: usize, y: f32, key: tl::NodeHandle) {
        let valid = self
            .frames
            .borrow()
            .get(frame)
            .is_some_and(|frame| frame.valid.get());
        if let (true, Some(&(_, outer))) = (valid, self.model.heights.get(&key)) {
            self.narrow(y - self.band.0 - 1e-3, self.band.1 - (y + outer) - 1e-3);
        }
    }

    /// Whether the subtree `key` lies wholly inside the band (so nothing
    /// in it culls): `true` where the cursor can't track it (an invalid
    /// frame culls nothing below it either).
    fn inside(&self, frame: usize, y: f32, key: tl::NodeHandle) -> bool {
        if !self
            .frames
            .borrow()
            .get(frame)
            .is_some_and(|frame| frame.valid.get())
        {
            return true;
        }
        match self.model.heights.get(&key) {
            Some(&(_, outer)) => y >= self.band.0 && y + outer <= self.band.1,
            None => true,
        }
    }

    /// ADR 0033: the outer height an unmeasured child is assumed to have —
    /// the mean of its measured siblings' (`None` when none is measured: the
    /// first build, or a cleared model). A new element's height exists only
    /// after its first layout; invalidating the cursor at it built every
    /// row after an insertion (bug_0168). The estimate places the rows
    /// after it for that one frame; the measurement corrects it.
    fn estimate(&self, children: &[tl::NodeHandle]) -> Option<f32> {
        let (sum, count) = children
            .iter()
            .filter(|child| !self.model.positioned.contains(*child))
            .filter_map(|child| self.model.heights.get(child))
            .fold((0.0, 0usize), |(sum, count), &(_, outer)| {
                (sum + outer, count + 1)
            });
        (count > 0).then(|| sum / count as f32)
    }

    /// The cursor step an unmeasured child is estimated at, if it may be:
    /// never for one the measurement marked untrackable (out of flow or an
    /// `auto` margin — it takes no space, or none the cursor knows).
    fn estimate_for(&self, child: tl::NodeHandle, estimate: Option<f32>, gap: f32) -> Option<f32> {
        if self.model.positioned.contains(&child) {
            return None;
        }
        estimate.map(|outer| outer + gap)
    }

    /// A new cursor frame for a container's children, when the container
    /// is one the vertical cursor can track.
    fn frame_for(&self, parent_frame: usize, top: f32, gap: f32, opaque: bool) -> Option<usize> {
        let mut frames = self.frames.borrow_mut();
        let parent = frames.get(parent_frame)?;
        let valid = parent.valid.get() && !opaque;
        frames.push(CursorFrame {
            y: std::cell::Cell::new(top),
            gap,
            valid: std::cell::Cell::new(valid),
        });
        Some(frames.len() - 1)
    }
}

impl Ctx<'_, '_, '_> {
    /// ADR 0031: cull `handle`'s subtree at `y` — only when its stored
    /// height still holds. A subtree whose content changed (not clean in
    /// the pairing) keeps its old height in the model until it
    /// materializes; culled by it, an above-band change left the spacer
    /// wrong and displaced every visible row (bug_0161, found by the
    /// windowing property). It builds for one frame instead, is measured,
    /// and the measurement's rebuild culls it again.
    /// The refused cull flags the band stale: the next frame rebuilds
    /// once (its document is unchanged, everything is clean) and culls
    /// it — a refused subtree whose height didn't change gives the
    /// measurement no news, and would otherwise stay built.
    fn cull(&self, win: &WindowCtx, frame: usize, y: f32, handle: tl::NodeHandle) -> Option<Item> {
        let spacer = win.culled(frame, y, handle)?;
        if let Some(skeleton) = self.skeleton
            && !skeleton
                .pairing
                .get(&handle)
                .is_some_and(|old| skeleton.clean.contains(old))
        {
            win.stale.set(true);
            return None;
        }
        Some(spacer)
    }

    /// The element `tag` is styled as. In a translation, a `data-l10n-name`
    /// element takes the source element of that name (tag, id, classes);
    /// `None` when there is no unused one of the same tag — its content is
    /// then plain text, as in fluent-dom.
    fn element(&self, tag: &tl::HTMLTag) -> Option<HtmlElement> {
        let element = element_of(tag);
        let (Some(named), Some(name)) =
            (self.named, tag.attributes().get("data-l10n-name").flatten())
        else {
            return Some(element);
        };
        let name = decode_attribute(name.as_utf8_str());
        let mut named = named.borrow_mut();
        let index = named.iter().position(|(source, _)| *source == name)?;
        if named[index].1.tag != element.tag {
            return None;
        }
        Some(named.remove(index).1)
    }
}

/// Walks the markup of `source`'s translation with `walk` (a fragment
/// context with `source`'s named elements). `false` when `source` has no
/// translation, or it doesn't parse: the caller walks the own content.
fn walk_translation(
    ctx: &Ctx,
    handle: tl::NodeHandle,
    source: &tl::HTMLTag,
    walk: impl FnOnce(&Ctx, &[tl::NodeHandle]),
) -> bool {
    let Some(Ok(translation)) = ctx.localized.get(&handle) else {
        return false;
    };
    let Ok(fragment) = tl::parse(translation, tl::ParserOptions::default()) else {
        return false;
    };
    let named: Vec<(String, HtmlElement)> = source
        .children()
        .all(ctx.parser)
        .iter()
        .filter_map(|node| {
            let tag = node.as_tag()?;
            let name = tag.attributes().get("data-l10n-name").flatten()?;
            Some((
                decode_attribute(name.as_utf8_str()).into_owned(),
                element_of(tag),
            ))
        })
        .collect();
    let named = RefCell::new(named);
    // Fragment handles index the fragment, not the source document — the
    // fragment gets its own states (collected per translated element by
    // `build_html_ui`), and no translations of its own.
    let unlocalized = HashMap::default();
    let fragment_ctx = Ctx {
        parser: fragment.parser(),
        localized: &unlocalized,
        states: ctx.fragments.and_then(|fragments| fragments.get(&handle)),
        fragments: None,
        styler: ctx.styler,
        named: Some(&named),
        focus_within: &EMPTY_FOCUS_WITHIN,
        skeleton: None,
        // Discarded: translations are rebuilt every frame, so their
        // inline vars are never stale.
        inline_vars: std::cell::RefCell::default(),
        // Translated subtrees are never culled (their items come from a
        // per-build fragment the model doesn't track).
        window: None,
    };
    walk(&fragment_ctx, fragment.children());
    true
}

/// ADR 0021: the previous build's item tree and the document it was built
/// from. A content change pairs the fresh DOM with it (pair_trees); every
/// subtree whose tag, attributes and text are unchanged reuses the cached
/// items verbatim (handles remapped to the fresh document), skipping the
/// cascade and the run construction for it. Style-side inputs (stylesheet,
/// fonts, `:hover`/`:active`, `var()` values) invalidate the whole reuse
/// for that build, and translated elements are always rebuilt (their
/// content comes from per-build fragments).
#[doc(hidden)]
#[derive(Component, Default)]
pub struct Skeleton {
    /// The document the specs were built from.
    document: Option<HtmlDocument>,
    /// The previous build's spec tree. Clean subtrees are handed to the
    /// diff through `Arc` clones (refcount bumps, no copies); the handles
    /// in it are kept current by the per-build remap below.
    specs: Vec<SpecNode>,
    /// A node that produced a spec → its position in `specs` (a path of
    /// container-child indices).
    table: HashMap<tl::NodeHandle, Vec<usize>>,
    /// The last reuse run saved at least half the specs; an all-dirty
    /// frame (a `rebuilding` workload) turns the walk off until a retry.
    useful: bool,
    /// Builds since the last reuse attempt, for the retry cadence.
    since_try: u32,
    /// Consecutive useless retries: the retry interval doubles per strike
    /// (16, then 32, 64, 128, capped at 256 with `min(4)`) so a
    /// permanently all-dirty workload pays almost nothing while one that
    /// becomes reusable is still found.
    strike: u8,
    /// The last content build's `(reused subtrees, collected specs)`:
    /// `(0, total)` when it ran without a pairing walk. `None` before any.
    last_reuse: Option<(usize, usize)>,
}

impl Skeleton {
    /// The source the skeleton's specs were built from. Exposed for the
    /// skeleton-reuse guards (a gate-skipped build must have replaced the
    /// skeleton, document included); not covered by semver.
    #[doc(hidden)]
    pub fn document_source(&self) -> Option<&str> {
        self.document.as_ref().map(|document| document.source())
    }

    /// The adaptive-retry state as `(useful, strike)`: a restyle pass must
    /// preserve it (a full-page restyle's zero reuse says nothing about
    /// content-pairing utility — ADR 0029). Exposed for the guards; not
    /// covered by semver.
    #[doc(hidden)]
    pub fn pairing_health(&self) -> (bool, u8) {
        (self.useful, self.strike)
    }

    /// The last content build's `(reused subtrees, collected specs)` —
    /// `(0, total)` when it ran without a pairing walk, `None` before the
    /// first content update. `pairing_health` starts at `(true, 0)`, so it
    /// can't tell "reuse works" from "reuse never ran" (the testbench's
    /// reuse check passed with bug_0150 put back); this can. Exposed for
    /// the guards and the testbench; not covered by semver.
    #[doc(hidden)]
    pub fn last_reuse(&self) -> Option<(usize, usize)> {
        self.last_reuse
    }
}

/// The per-build pairing state: where the fresh DOM meets the previous
/// one, and which old subtrees are unchanged.
struct SkeletonPass<'a> {
    /// Fresh handle → the paired node of the previous document.
    pairing: &'a HashMap<tl::NodeHandle, tl::NodeHandle>,
    /// Old handles whose whole subtree content matches.
    clean: &'a HashSet<tl::NodeHandle>,
    specs: &'a [SpecNode],
    table: &'a HashMap<tl::NodeHandle, Vec<usize>>,
    /// How many specs the build reused.
    reused: Cell<usize>,
    /// ADR 0029: a style-side change that scopes to nothing (a sheet,
    /// font, image or `var()` change): no cached subtree's style is known
    /// unchanged, so nothing reuses and every element re-cascades.
    full_style: bool,
    /// ADR 0029: old-document handles whose `PseudoState` changed this
    /// build — each flipped element, its ancestors up to the UI root, and
    /// its descendants. A flip restyles exactly the flipped element's
    /// subtree (no `:has`), so a node in that set must not reuse;
    /// everything else's style is unchanged.
    flipped: HashSet<tl::NodeHandle>,
    /// ADR 0031: old → fresh, for the windowing model's re-key (the
    /// heights transfer through content changes — a changed subtree's
    /// height is its last measured one until it materializes again).
    reverse: &'a HashMap<tl::NodeHandle, tl::NodeHandle>,
}

/// Pairs the two documents' trees and records which old subtrees are
/// unchanged (tag, attributes, own text, recursively — translations are
/// excluded: their content is re-parsed per build).
#[hotpath::measure]
fn pair_trees_into(
    old: &tl::VDom,
    fresh: &tl::VDom,
    pairing: &mut HashMap<tl::NodeHandle, tl::NodeHandle>,
    reverse: &mut HashMap<tl::NodeHandle, tl::NodeHandle>,
    clean: &mut HashSet<tl::NodeHandle>,
    reads: SelectorReads,
) {
    let mut shifted = Vec::new();
    let pairs = align_siblings(
        old.parser(),
        old.children(),
        fresh.parser(),
        fresh.children(),
        &mut shifted,
    );
    for (old_child, fresh_child) in pairs {
        pair_node(
            old.parser(),
            old_child,
            fresh.parser(),
            fresh_child,
            pairing,
            reverse,
            clean,
            0,
            reads,
        );
    }
    for handle in shifted {
        clean.remove(&handle);
    }
    let change = sibling_change(
        old.parser(),
        old.children(),
        fresh.parser(),
        fresh.children(),
    );
    unclean_siblings(old.parser(), old.children(), &change, clean, reads);
}

/// How a sibling list changed, as a selector sees it (review_0005 I05).
#[derive(Clone, Default)]
enum SiblingChange {
    /// The same length, node kinds, tags and attributes (a text change
    /// elsewhere leaves a list alike).
    #[default]
    Same,
    /// The same shape, but these children's attributes (class, id,
    /// `data-*`, …) changed: only what a sibling combinator reads.
    Attributes(Vec<usize>),
    /// The length, a node kind (element or not) or a tag name changed:
    /// positions may have moved.
    Shape,
}

fn sibling_change(
    old_parser: &tl::Parser,
    old: &[tl::NodeHandle],
    fresh_parser: &tl::Parser,
    fresh: &[tl::NodeHandle],
) -> SiblingChange {
    if old.len() != fresh.len() {
        return SiblingChange::Shape;
    }
    let mut changed = Vec::new();
    for (index, (old, fresh)) in old.iter().zip(fresh).enumerate() {
        match (old.get(old_parser), fresh.get(fresh_parser)) {
            (Some(tl::Node::Tag(old)), Some(tl::Node::Tag(fresh))) => {
                if !old
                    .name()
                    .as_utf8_str()
                    .eq_ignore_ascii_case(&fresh.name().as_utf8_str())
                {
                    return SiblingChange::Shape;
                }
                if !tag_equal(old, fresh) {
                    changed.push(index);
                }
            }
            (Some(tl::Node::Tag(_)), _) | (_, Some(tl::Node::Tag(_))) => {
                return SiblingChange::Shape;
            }
            _ => {}
        }
    }
    if changed.is_empty() {
        SiblingChange::Same
    } else {
        SiblingChange::Attributes(changed)
    }
}

/// A sibling list changed (review_0005 I05): when the sheet's matches can
/// depend on siblings, an unchanged child's cached spec may hold a stale
/// `+`/`~`/structural match, so the affected children leave the clean
/// set and re-collect. A shape change moves positions: every child goes
/// (and, when an ancestor compound reads a position — `li:first-child p`
/// — its whole subtree). An attribute change only matters to sibling
/// combinators, which look back: the next child after each changed one
/// for `+`, every child after the first changed one for `~`. A
/// re-collected child's descendants still reuse where their inherited
/// style is unchanged.
fn unclean_siblings(
    old_parser: &tl::Parser,
    children: &[tl::NodeHandle],
    change: &SiblingChange,
    clean: &mut HashSet<tl::NodeHandle>,
    reads: SelectorReads,
) {
    let roots: Vec<tl::NodeHandle> = match change {
        SiblingChange::Same => return,
        SiblingChange::Shape if reads.sibling_dependent() => children.to_vec(),
        SiblingChange::Shape => return,
        SiblingChange::Attributes(changed) => {
            let mut roots = Vec::new();
            if reads.general_siblings
                && let Some(first) = changed.first()
            {
                roots.extend(&children[first + 1..]);
            } else if reads.adjacent_siblings {
                // The next element after each changed one (text nodes in
                // between don't count as siblings for `+`).
                for index in changed {
                    let next = children[index + 1..]
                        .iter()
                        .find(|child| matches!(child.get(old_parser), Some(tl::Node::Tag(_))));
                    roots.extend(next);
                }
            }
            roots
        }
    };
    let deep = reads.context_position && matches!(change, SiblingChange::Shape);
    let mut stack = roots;
    while let Some(handle) = stack.pop() {
        clean.remove(&handle);
        if deep && let Some(tl::Node::Tag(tag)) = handle.get(old_parser) {
            stack.extend(tag.children().top().iter().copied());
        }
    }
}

/// Pairs the two nodes; returns whether their whole subtrees match.
/// Iterative (an explicit task stack): a tag is clean when every child
/// pair matched, so its completion runs after its children (post-order).
/// Trees deeper than [`MAX_DOM_DEPTH`] never match (the fresh collect
/// caps them, so a cached spec could not describe them anyway).
fn pair_node(
    old_parser: &tl::Parser,
    old: tl::NodeHandle,
    fresh_parser: &tl::Parser,
    fresh: tl::NodeHandle,
    pairing: &mut HashMap<tl::NodeHandle, tl::NodeHandle>,
    reverse: &mut HashMap<tl::NodeHandle, tl::NodeHandle>,
    clean: &mut HashSet<tl::NodeHandle>,
    depth: usize,
    reads: SelectorReads,
) -> bool {
    /// A tag pair whose children are being paired; `all` folds their
    /// results; `siblings`: how the child lists differ (review_0005 I05).
    struct Parent {
        old: tl::NodeHandle,
        fresh: tl::NodeHandle,
        all: bool,
        siblings: SiblingChange,
    }
    enum Task {
        /// Pair two nodes, folding the result into the parent tag's
        /// accumulator (`None`: the root pair, folded into `all`).
        Pair {
            old: tl::NodeHandle,
            fresh: tl::NodeHandle,
            depth: usize,
            parent: Option<usize>,
        },
        /// A tag pair's children are all done: complete it.
        Complete {
            parent: usize,
            grandparent: Option<usize>,
        },
    }
    let mut all = true;
    let mut parents: Vec<Parent> = Vec::new();
    // `li`s an alignment moved: their marker numbers changed.
    let mut shifted: Vec<tl::NodeHandle> = Vec::new();
    let mut stack = vec![Task::Pair {
        old,
        fresh,
        depth,
        parent: None,
    }];
    while let Some(task) = stack.pop() {
        match task {
            Task::Pair {
                old,
                fresh,
                depth,
                parent,
            } => {
                let ok = 'pair: {
                    let (Some(old_node), Some(fresh_node)) =
                        (old.get(old_parser), fresh.get(fresh_parser))
                    else {
                        break 'pair false;
                    };
                    let (old_tag, fresh_tag) = match (old_node, fresh_node) {
                        (tl::Node::Tag(old_tag), tl::Node::Tag(fresh_tag)) => (old_tag, fresh_tag),
                        // Raw text has no item of its own (anonymous
                        // blocks are keyed by their element); its equality
                        // counts toward the parent's.
                        (tl::Node::Raw(old_text), tl::Node::Raw(fresh_text)) => {
                            // ADR 0031: a raw text node keys its anonymous
                            // block's height (and a cached spec's
                            // `window_key`): map it like an element, or a
                            // content change drops the height and the
                            // cursor stops culling there (bug_0160).
                            // An unchanged text is clean: its stored height
                            // still holds, so it may cull by it.
                            reverse.insert(old, fresh);
                            pairing.insert(fresh, old);
                            let same = old_text.as_utf8_str() == fresh_text.as_utf8_str();
                            if same {
                                clean.insert(old);
                            }
                            break 'pair same;
                        }
                        (tl::Node::Comment(_), tl::Node::Comment(_)) => break 'pair true,
                        _ => break 'pair false,
                    };
                    // A translated element's item comes from its per-build
                    // fragment, never from this DOM subtree: exclude it
                    // (and its descendants, whose items are fragment-based
                    // too) before any attribute work — an all-translated
                    // document exits the walk at the top.
                    let translated = |tag: &tl::HTMLTag| {
                        tag.attributes().get("data-l10n-id").flatten().is_some()
                    };
                    if translated(old_tag)
                        || translated(fresh_tag)
                        || !tag_equal(old_tag, fresh_tag)
                    {
                        break 'pair false;
                    }
                    let (old_children, fresh_children) = (old_tag.children(), fresh_tag.children());
                    let (old_children, fresh_children) = (old_children.top(), fresh_children.top());
                    if depth + 1 >= MAX_DOM_DEPTH {
                        break 'pair false;
                    }
                    // A changed child count still pairs the tag and its
                    // aligned children (bug_0168): unpaired, the whole
                    // subtree lost its reuse and its window heights.
                    let same_count = old_children.len() == fresh_children.len();
                    let pairs = align_siblings(
                        old_parser,
                        old_children.as_slice(),
                        fresh_parser,
                        fresh_children.as_slice(),
                        &mut shifted,
                    );
                    let index = parents.len();
                    parents.push(Parent {
                        old,
                        fresh,
                        all: same_count,
                        siblings: sibling_change(
                            old_parser,
                            old_children.as_slice(),
                            fresh_parser,
                            fresh_children.as_slice(),
                        ),
                    });
                    // The tag completes after its children: the completion
                    // task sits below theirs on the stack.
                    stack.push(Task::Complete {
                        parent: index,
                        grandparent: parent,
                    });
                    for (old_child, fresh_child) in pairs {
                        stack.push(Task::Pair {
                            old: old_child,
                            fresh: fresh_child,
                            depth: depth + 1,
                            parent: Some(index),
                        });
                    }
                    true
                };
                match parent {
                    Some(parent) => parents[parent].all &= ok,
                    None => all &= ok,
                }
            }
            Task::Complete {
                parent,
                grandparent,
            } => {
                let (old, fresh, children_all) = (
                    parents[parent].old,
                    parents[parent].fresh,
                    parents[parent].all,
                );
                let siblings = std::mem::take(&mut parents[parent].siblings);
                if let Some(tl::Node::Tag(tag)) = old.get(old_parser) {
                    unclean_siblings(
                        old_parser,
                        tag.children().top().as_slice(),
                        &siblings,
                        clean,
                        reads,
                    );
                }
                // Paired whether or not its subtree is clean: a changed
                // container's cached spec still carries the inherited style
                // its unchanged children were built under, and without the
                // pairing every child of a changed container re-collected
                // (ADR 0029's inherited-style gate found no cached spec)
                // — content reuse worked only at the document's top level
                // (bug_0150). `clean` alone gates reuse.
                pairing.insert(fresh, old);
                reverse.insert(old, fresh);
                if children_all {
                    clean.insert(old);
                }
                match grandparent {
                    Some(grandparent) => parents[grandparent].all &= children_all,
                    None => all &= children_all,
                }
            }
        }
    }
    for handle in shifted {
        clean.remove(&handle);
    }
    all
}

/// Which old child pairs with which fresh one. Equal counts pair by
/// position. A changed count (an insertion or removal: bug_0168) pairs the
/// common head and the common tail — children whose source is identical,
/// scanned from each end — and the rest of the middle by position, so one
/// inserted row no longer pairs every row after it with its neighbour
/// (each unclean, re-collected, and in a windowed UI built). Pairing never
/// claims cleanness: `pair_node` decides that per pair. An `li` moved to
/// another index changed its marker number, so it goes into `shifted`
/// (removed from the clean set once the pairing completes).
fn align_siblings(
    old_parser: &tl::Parser,
    old: &[tl::NodeHandle],
    fresh_parser: &tl::Parser,
    fresh: &[tl::NodeHandle],
    shifted: &mut Vec<tl::NodeHandle>,
) -> Vec<(tl::NodeHandle, tl::NodeHandle)> {
    if old.len() == fresh.len() {
        return old.iter().copied().zip(fresh.iter().copied()).collect();
    }
    let same = |old: tl::NodeHandle, fresh: tl::NodeHandle| {
        same_source(old_parser, old, fresh_parser, fresh)
    };
    let shorter = old.len().min(fresh.len());
    let head = (0..shorter).take_while(|&i| same(old[i], fresh[i])).count();
    let tail = (0..shorter - head)
        .take_while(|&i| same(old[old.len() - 1 - i], fresh[fresh.len() - 1 - i]))
        .count();
    let mut pairs = Vec::with_capacity(shorter);
    pairs.extend((0..head).map(|i| (old[i], fresh[i])));
    let (old_middle, fresh_middle) = (
        &old[head..old.len() - tail],
        &fresh[head..fresh.len() - tail],
    );
    pairs.extend(old_middle.iter().copied().zip(fresh_middle.iter().copied()));
    let tail_pairs = (0..tail).map(|i| (old[old.len() - tail + i], fresh[fresh.len() - tail + i]));
    pairs.extend(tail_pairs);
    // Index-dependent content: an `li`'s marker counts the `li`s before it.
    let ordinals =
        |parser: &tl::Parser, list: &[tl::NodeHandle]| -> HashMap<tl::NodeHandle, usize> {
            let mut count = 0;
            list.iter()
                .filter(|child| is_li(parser, **child))
                .map(|child| {
                    count += 1;
                    (*child, count)
                })
                .collect()
        };
    let (old_ordinals, fresh_ordinals) = (ordinals(old_parser, old), ordinals(fresh_parser, fresh));
    for &(old_child, fresh_child) in &pairs {
        if old_ordinals
            .get(&old_child)
            .is_some_and(|ordinal| fresh_ordinals.get(&fresh_child) != Some(ordinal))
        {
            shifted.push(old_child);
        }
    }
    pairs
}

fn is_li(parser: &tl::Parser, handle: tl::NodeHandle) -> bool {
    matches!(handle.get(parser), Some(tl::Node::Tag(tag)) if tag.name().as_utf8_str().eq_ignore_ascii_case("li"))
}

/// The two nodes' source text is identical (a tag's covers its whole
/// subtree): their content can't differ.
fn same_source(
    old_parser: &tl::Parser,
    old: tl::NodeHandle,
    fresh_parser: &tl::Parser,
    fresh: tl::NodeHandle,
) -> bool {
    match (old.get(old_parser), fresh.get(fresh_parser)) {
        (Some(tl::Node::Tag(old)), Some(tl::Node::Tag(fresh))) => {
            old.raw().as_bytes() == fresh.raw().as_bytes()
        }
        (Some(tl::Node::Raw(old)), Some(tl::Node::Raw(fresh))) => {
            old.as_bytes() == fresh.as_bytes()
        }
        (Some(tl::Node::Comment(old)), Some(tl::Node::Comment(fresh))) => {
            old.as_bytes() == fresh.as_bytes()
        }
        _ => false,
    }
}

/// Same tag name (case-insensitive) and the same attributes by name and
/// raw value (attribute order is not semantic in HTML).
fn tag_equal(old: &tl::HTMLTag, fresh: &tl::HTMLTag) -> bool {
    if !old
        .name()
        .as_utf8_str()
        .eq_ignore_ascii_case(&fresh.name().as_utf8_str())
    {
        return false;
    }
    let (old_attributes, fresh_attributes) = (old.attributes(), fresh.attributes());
    if old_attributes.iter().count() != fresh_attributes.iter().count() {
        return false;
    }
    old_attributes
        .iter()
        .all(|(name, value)| match fresh_attributes.get(name.as_ref()) {
            Some(Some(bytes)) => value.as_ref().is_some_and(|cow| *bytes == **cow),
            // A value-less attribute on both sides.
            Some(None) => value.is_none(),
            // The attribute is missing on the fresh side.
            None => false,
        })
}

/// How much collection work the tree represents: a fresh spec counts
/// itself plus its children; a reused (cached) subtree counts once — its
/// nodes were neither cascaded nor re-collected.
fn spec_count(specs: &[SpecNode]) -> usize {
    let mut count = 0;
    let mut stack = vec![specs];
    while let Some(level) = stack.pop() {
        for node in level {
            count += 1;
            if let SpecNode::Fresh(spec) = node {
                stack.push(&spec.children);
            }
        }
    }
    count
}

/// A node that produced a spec → its position in `specs`.
fn spec_table(specs: &[SpecNode]) -> HashMap<tl::NodeHandle, Vec<usize>> {
    let mut table = HashMap::default();
    // (the path of the level's parent, the level)
    let mut stack: Vec<(Vec<usize>, &[SpecNode])> = vec![(Vec::new(), specs)];
    while let Some((prefix, level)) = stack.pop() {
        for (index, node) in level.iter().enumerate() {
            let spec = node.spec();
            let mut path = Vec::with_capacity(prefix.len() + 1);
            path.extend_from_slice(&prefix);
            path.push(index);
            if !spec.children.is_empty() {
                stack.push((path.clone(), &spec.children));
            }
            if let Some(handle) = spec.handle {
                table.insert(handle, path);
            }
        }
    }
    table
}

impl SkeletonPass<'_> {
    /// The cached spec of the paired old node. The handles inside it point
    /// into the previous document; the build remaps them up front (see
    /// `remap_spec_handles`), so the `Arc` is handed on as is. Returns
    /// `false` when nothing was reused (the node produced no spec) and the
    /// caller must collect normally.
    fn reuse(&self, old: tl::NodeHandle, items: &mut Vec<Item>) -> bool {
        let Some(path) = self.table.get(&old) else {
            return false;
        };
        let Some(spec) = spec_rc_at(self.specs, path) else {
            return false;
        };
        self.reused.set(self.reused.get() + 1);
        items.push(Item::Cached(Arc::clone(spec)));
        true
    }

    /// The cached spec of a paired old node, without reusing it: ADR
    /// 0029's scoping reads its stored inherited style to decide whether
    /// a re-collected element's children must re-cascade.
    fn cached_spec(&self, old: tl::NodeHandle) -> Option<&Arc<NodeSpec>> {
        let path = self.table.get(&old)?;
        spec_rc_at(self.specs, path)
    }
}

/// The spec at `path`: the first index addresses a top-level spec, each
/// following index a container's child.
fn spec_rc_at<'a>(specs: &'a [SpecNode], path: &[usize]) -> Option<&'a Arc<NodeSpec>> {
    let (first, rest) = path.split_first()?;
    let mut current = specs.get(*first)?.spec();
    for &index in rest {
        current = current.children.get(index)?.spec();
    }
    Some(current)
}

/// Rewrites every DOM handle in the cached specs to the fresh document's
/// counterpart (a clean subtree's nodes are all paired, so every lookup
/// succeeds). Runs up front, when the skeleton's `Rc`s are exclusively
/// held: the reused specs are then handed on with current handles, and the
/// entities' `DomNode`s never go stale.
fn remap_spec_handles(specs: &mut [SpecNode], reverse: &HashMap<tl::NodeHandle, tl::NodeHandle>) {
    let remapped = |handle: &mut Option<tl::NodeHandle>| {
        if let Some(handle) = handle.as_mut()
            && let Some(new) = reverse.get(handle)
        {
            *handle = *new;
        }
    };
    let mut stack = vec![specs];
    while let Some(level) = stack.pop() {
        for node in level {
            let spec = match node {
                SpecNode::Fresh(spec) | SpecNode::Cached(spec) => spec,
            };
            let Some(spec) = Arc::get_mut(spec) else {
                // Shared with a previous frame's items (impossible: they
                // are dropped with the build); leave the handle stale.
                continue;
            };
            remapped(&mut spec.handle);
            remapped(&mut spec.window_key);
            for run in spec.text.iter_mut().flat_map(|text| &mut text.spans) {
                if let Some(link) = &mut run.link {
                    let mut handle = Some(link.handle);
                    remapped(&mut handle);
                    link.handle = handle.unwrap_or(link.handle);
                }
            }
            stack.push(&mut spec.children);
        }
    }
}

/// User content nested deeper than this is dropped with a warning. The
/// build's walks are iterative (an explicit work stack: the document's
/// depth costs heap, not stack), so this caps what's *downstream* of
/// them: Bevy's layout recurses per tree level (taffy), and pathological
/// fan-out (entities, spans, specs) stays bounded. Browsers parse far
/// deeper; this only bounds pathological documents.
const MAX_DOM_DEPTH: usize = 100;

/// Warns once per process when content is dropped for its depth.
/// Whether `element` is a form whose `data-on-submit` names a signal. A
/// blank name binds nothing (`signal_bindings` skips it), so its buttons
/// must not become submit buttons — they swallowed the enclosing
/// wrapper's click and emitted nothing (review_0003 G12).
fn form_submits(element: &HtmlElement) -> bool {
    element.tag == "form"
        && element
            .data("on-submit")
            .is_some_and(|name| !name.trim().is_empty())
}

/// Whether a `<button>` submits its form: HTML's `type` is an enumerated
/// attribute whose missing *and* invalid value default is `submit`, so
/// every value but `button` and `reset` submits (review_0003 G16: a
/// misspelt `type="sbumit"` did nothing).
fn button_submits(tag: &tl::HTMLTag) -> bool {
    tag.attributes()
        .get("type")
        .flatten()
        .map(|kind| kind.as_utf8_str())
        .is_none_or(|kind| {
            !(kind.eq_ignore_ascii_case("button") || kind.eq_ignore_ascii_case("reset"))
        })
}

/// Whether an `<input>` is a text field: HTML's missing *and* invalid
/// `type` default is `text`, so only the other known types build no field
/// (review_0003 G36: `type=""` or a misspelt type built nothing).
#[cfg(feature = "text-input")]
fn input_is_text(tag: &tl::HTMLTag) -> bool {
    const OTHER_TYPES: [&str; 21] = [
        "hidden",
        "search",
        "tel",
        "url",
        "email",
        "password",
        "date",
        "month",
        "week",
        "time",
        "datetime-local",
        "number",
        "range",
        "color",
        "checkbox",
        "radio",
        "file",
        "submit",
        "image",
        "reset",
        "button",
    ];
    tag.attributes().get("type").flatten().is_none_or(|kind| {
        let kind = kind.as_utf8_str();
        !OTHER_TYPES
            .iter()
            .any(|other| kind.eq_ignore_ascii_case(other))
    })
}

fn warn_overdepth() {
    static WARNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if !WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        warn!("document nesting deeper than {MAX_DOM_DEPTH} elements: deeper content is dropped");
    }
}

#[hotpath::measure]
fn collect_items(
    dom: &tl::VDom,
    localized: &HashMap<tl::NodeHandle, Result<String, String>>,
    states: Option<&HashMap<tl::NodeHandle, Pseudo>>,
    fragments: &HashMap<tl::NodeHandle, HashMap<tl::NodeHandle, Pseudo>>,
    focus_within: &HashSet<tl::NodeHandle>,
    styler: &Styler,
    root: Style,
    grid: Option<&GridNames>,
    skeleton: Option<&SkeletonPass>,
    window: Option<WindowCtx>,
) -> (Vec<Item>, Vec<String>) {
    let ctx = Ctx {
        parser: dom.parser(),
        localized,
        states,
        fragments: Some(fragments),
        styler,
        named: None,
        skeleton,
        inline_vars: std::cell::RefCell::default(),
        focus_within,
        window,
    };
    let mut items = Vec::new();
    // The document's own top level: walked-through roots (`html`,
    // `body`, unknown tags) are collected like any element, so their style
    // and their place in selectors hold (an unwrap that skipped them lost
    // `body { … }` and `body > p` for every document — bug_0172). For the
    // window they pass the root cursor through to their children.
    let roots: Vec<tl::NodeHandle> = dom.children().iter().copied().collect();
    // ADR 0028: the top-level elements' positions (1-based, of all and
    // same-tag).
    let positions = sibling_positions(&roots, &ctx);
    let siblings_list = sibling_list(&roots, &positions, &ctx);
    // ADR 0031: the document's cursor frame starts at the root's content
    // top (its own box is the root rule's, outside the walk).
    let root_frame = ctx.window.as_ref().map(|win| {
        win.frames.borrow_mut().push(CursorFrame {
            y: std::cell::Cell::new(0.0),
            gap: win.default_gap,
            valid: std::cell::Cell::new(true),
        });
        win.frames.borrow().len() - 1
    });
    // The roots advance the document cursor like any container's
    // children (an unknown height leaves the rest invalid).
    let mut cursor = 0.0;
    let root_estimate = ctx.window.as_ref().and_then(|win| win.estimate(&roots));
    for handle in &roots {
        // Raw roots (a whitespace-only page's text, say) collect too —
        // with no position (they aren't elements to match).
        let position = positions.get(handle).copied().unwrap_or_default();
        let advance = ctx.window.as_ref().and_then(|win| {
            // The ROOT frame's gap: materialized containers push their
            // own frames, so `last()` was the previous row's `gap`, not
            // the document's (bug_0151).
            let gap = root_frame
                .and_then(|frame| win.frames.borrow().get(frame).map(|frame| frame.gap))
                .unwrap_or(0.0);
            column_height(&ctx, win, *handle, gap, 0)
                .or_else(|| win.estimate_for(*handle, root_estimate, gap))
        });
        collect_node(
            &ctx,
            *handle,
            &root,
            &mut items,
            Rc::new(Vec::new()),
            grid,
            0,
            position,
            Some(siblings_list.clone()),
            root_frame.map(|frame| (frame, cursor)),
        );
        match advance {
            Some(step) => cursor += step,
            // No height and no estimate (no measured sibling, or an
            // element the cursor can't place) invalidates the cursor.
            None => {
                if let Some(frame) = root_frame {
                    ctx.window.as_ref().unwrap().frames.borrow()[frame]
                        .valid
                        .set(false);
                }
            }
        }
    }
    (items, ctx.inline_vars.borrow().iter().cloned().collect())
}

/// The empty focus-within set for translation fragments (fragments don't
/// carry the document's focus handles).
static EMPTY_FOCUS_WITHIN: HashSet<tl::NodeHandle> = HashSet::new();

/// ADR 0028: an element's `Ancestor` (the sibling lists' entry): its
/// tag/id/classes, the submit marker, and its interaction state.
fn ancestor_of(
    handle: tl::NodeHandle,
    ctx: &Ctx,
    positions: &HashMap<tl::NodeHandle, Position>,
) -> Option<Ancestor> {
    match handle.get(ctx.parser) {
        Some(tl::Node::Tag(tag)) => {
            let element = element_of(tag);
            let state = ctx
                .states
                .and_then(|states| states.get(&handle))
                .copied()
                .unwrap_or_default();
            Some(ancestor_from(
                element,
                state,
                positions.get(&handle).copied().unwrap_or_default(),
            ))
        }
        _ => None,
    }
}

/// An element as an [`Ancestor`]: everything a compound in an ancestor
/// or sibling position reads (review_0005 I07: dataset and position
/// too).
fn ancestor_from(element: HtmlElement, pseudo: Pseudo, position: Position) -> Ancestor {
    let submit = form_submits(&element);
    Ancestor {
        submit,
        tag: element.tag,
        id: element.id,
        classes: element.classes,
        pseudo,
        dataset: (!element.dataset.is_empty()).then(|| Rc::new(element.dataset)),
        position,
        link: SiblingLink::default(),
    }
}

/// The children's full structural states (positions and preceding
/// siblings): the inline run walk's input, where every child is matched
/// in place.
fn sibling_states(handles: &[tl::NodeHandle], ctx: &Ctx) -> HashMap<tl::NodeHandle, ElementState> {
    let positions = sibling_positions(handles, ctx);
    let list = sibling_list(handles, &positions, ctx);
    positions
        .iter()
        .map(|(handle, position)| {
            let state = ElementState {
                previous_siblings: Some(list.preceding(position.tag_index.saturating_sub(1))),
                ..ElementState::at(*position)
            };
            (*handle, state)
        })
        .collect()
}

/// One parent's element children as a shared sibling list (ADR 0028:
/// the sibling combinators' input; each child views its prefix).
fn sibling_list(
    handles: &[tl::NodeHandle],
    positions: &HashMap<tl::NodeHandle, Position>,
    ctx: &Ctx,
) -> Rc<SiblingList> {
    SiblingList::new(
        handles
            .iter()
            .filter_map(|handle| ancestor_of(*handle, ctx, positions))
            .collect(),
    )
}

/// ADR 0028: each element child's structural position — its 1-based
/// index among the parent's element children (all, and of the same tag)
/// with both counts (the `:nth-child`/`:nth-of-type` inputs), whether it
/// holds the focus (`:focus-within`) and whether it is `:empty`. Only
/// element handles appear. Linear: the names are read once and the
/// total counted once (review_0005 I03: it was recounted per child).
fn sibling_positions(handles: &[tl::NodeHandle], ctx: &Ctx) -> HashMap<tl::NodeHandle, Position> {
    let tags: Vec<(tl::NodeHandle, &tl::HTMLTag)> = handles
        .iter()
        .filter_map(|handle| match handle.get(ctx.parser) {
            Some(tl::Node::Tag(tag)) => Some((*handle, tag)),
            _ => None,
        })
        .collect();
    let names: Vec<String> = tags
        .iter()
        .map(|(_, tag)| tag.name().as_utf8_str().to_ascii_lowercase())
        .collect();
    let mut totals: HashMap<&str, usize> = HashMap::default();
    for name in &names {
        *totals.entry(name.as_str()).or_default() += 1;
    }
    let mut seen: HashMap<&str, usize> = HashMap::default();
    let mut out = HashMap::default();
    for (index, ((handle, tag), name)) in tags.iter().zip(&names).enumerate() {
        let type_index = {
            let seen = seen.entry(name.as_str()).or_default();
            *seen += 1;
            *seen
        };
        out.insert(
            *handle,
            Position {
                tag_index: index + 1,
                tag_count: tags.len(),
                type_index,
                type_count: totals[name.as_str()],
                focus_within: ctx.focus_within.contains(handle),
                // `:empty`: no children at all (comments don't count).
                empty: !tag
                    .children()
                    .top()
                    .iter()
                    .any(|child| !matches!(child.get(ctx.parser), Some(tl::Node::Comment(_)))),
            },
        );
    }
    out
}

fn collect_node(
    ctx: &Ctx,
    handle: tl::NodeHandle,
    inherited: &Style,
    items: &mut Vec<Item>,
    chain: Rc<Vec<Ancestor>>,
    grid: Option<&GridNames>,
    depth: usize,
    position: Position,
    siblings: Option<Rc<SiblingList>>,
    window: Option<(usize, f32)>,
) {
    /// A node to collect: everything its children need (the computed
    /// style, the ancestor chain, its grid template), plus, for a
    /// container, where its children's items start (post-order
    /// completion — the subtree's items are the tail of `items`).
    struct Open {
        element: HtmlElement,
        handle: tl::NodeHandle,
        signals: Vec<SignalBinding>,
        focus: Option<Focusable>,
        custom: Option<CustomElement>,
        boxed: BoxStyle,
        pointer_events: bool,
        font_size: f32,
        start: usize,
        style: Style,
        /// The container element's own reactivity; the children's folds in
        /// at completion (ADR 0029).
        own_reactive: bool,
    }
    enum Task {
        Node {
            handle: tl::NodeHandle,
            inherited: Style,
            chain: Rc<Vec<Ancestor>>,
            grid: Option<GridNames>,
            depth: usize,
            /// The node's inherited base changed against what its cached
            /// spec was collected with (ADR 0029): its own cascade output
            /// may differ, so it may not reuse (its children re-decide
            /// from its fresh inherited style).
            inherited_changed: bool,
            /// An ancestor's interaction state flipped (ADR 0029): the
            /// node re-decides reuse from its cached spec's
            /// `pseudo_reactive`.
            ancestor_flipped: bool,
            /// ADR 0027: the node's 1-based position among its parent's
            /// `li` children (`None` for anything that isn't an `li`):
            /// the marker's number comes from the parent list's counter.
            li_index: Option<usize>,
            /// ADR 0028: the node's 1-based index among the parent's
            /// element children (all, then same-tag), with both counts.
            position: Position,
            /// ADR 0028: the parent's element children as `Ancestor`s
            /// (the sibling combinators' input; the child views its
            /// prefix).
            siblings: Option<Rc<SiblingList>>,
            /// ADR 0027: the parent's tag-default list type (`ol` numbers,
            /// `ul` uses `disc`) — the `li`'s own computed type wins when
            /// its chain declared one.
            list_kind: ListStyleSpec,
            /// ADR 0031: the node's cursor frame and margin-box top
            /// (`None` inside translations and other windowing-free
            /// walks).
            window: Option<(usize, f32)>,
        },
        Complete(Box<Open>),
    }
    let mut stack = vec![Task::Node {
        handle,
        inherited: inherited.clone(),
        chain: Rc::new(chain.to_vec()),
        grid: grid.cloned(),
        depth,
        // The root style's only inputs are the sheet, the fonts and the
        // inline images — every one of them a `full_style` signal — so a
        // build that reuses at all has an unchanged root base.
        inherited_changed: false,
        ancestor_flipped: false,
        li_index: None,
        list_kind: ListStyleSpec::Disc,
        // The caller's position (the document root list's, computed by
        // `collect_items`; the fragment walk's for its own).
        position,
        // ADR 0028: the caller's sibling list (this element's parent's
        // children as `Ancestor`s).
        siblings,
        window,
    }];
    while let Some(task) = stack.pop() {
        let (
            handle,
            inherited,
            chain,
            grid,
            depth,
            inherited_changed,
            ancestor_flipped,
            li_index,
            list_kind,
            position,
            siblings,
            window,
        ) = match task {
            Task::Complete(open) => {
                let Open {
                    element,
                    handle,
                    signals,
                    focus,
                    custom,
                    boxed,
                    pointer_events,
                    font_size,
                    start,
                    style,
                    own_reactive,
                } = *open;
                // ADR 0031: consecutive culled children = one spacer.
                let children = coalesce_spacers(items.drain(start..).collect());
                // The container re-cascades on a flip when its own
                // rules or anything inside it could react (ADR 0029).
                let pseudo_reactive = own_reactive
                    || children.iter().any(|item| match item {
                        Item::Cached(spec) => spec.pseudo_reactive,
                        item => item_shell_reactive(item),
                    });
                items.push(Item::Container {
                    element,
                    handle: Some(handle),
                    signals,
                    focus,
                    custom,
                    boxed,
                    pointer_events,
                    font_size,
                    style,
                    pseudo_reactive,
                    keyed: ctx.window.is_some(),
                    children,
                });
                continue;
            }
            Task::Node {
                handle,
                inherited,
                chain,
                grid,
                depth,
                inherited_changed,
                ancestor_flipped,
                li_index,
                list_kind,
                position,
                siblings,
                window,
            } => (
                handle,
                inherited,
                chain,
                grid,
                depth,
                inherited_changed,
                ancestor_flipped,
                li_index,
                list_kind,
                position,
                siblings,
                window,
            ),
        };
        if depth >= MAX_DOM_DEPTH {
            warn_overdepth();
            continue;
        }
        let Some(node) = handle.get(ctx.parser) else {
            continue;
        };
        let tag = match node {
            tl::Node::Tag(tag) => tag,
            tl::Node::Raw(_) => {
                // Anonymous block: inherits the container's style — its
                // `white-space` decides collapsing (ADR 0028).
                let white_space = inherited.white_space.unwrap_or(WhiteSpaceSpec::Normal);

                // ADR 0031: an anonymous block outside the band = its
                // measured height (keyed by the raw text node's handle).
                if let (Some(win), Some((frame, y))) = (ctx.window.as_ref(), window)
                    && let Some(spacer) = ctx.cull(win, frame, y, handle)
                {
                    items.push(spacer);
                    continue;
                }
                let mut runs = Vec::new();
                let mut reactive = false;
                push_runs(
                    ctx,
                    handle,
                    &inherited,
                    RunExtras::default(),
                    &mut runs,
                    chain.clone(),
                    &mut reactive,
                    ElementState::default(),
                );
                let runs = finish_runs(runs, white_space);
                if !runs.is_empty() {
                    items.push(Item::Block(Block {
                        kind: BlockKind::Paragraph,
                        element: None,
                        handle: None,
                        key: ctx.window.is_some().then_some(handle),
                        signals: Vec::new(),
                        focus: None,
                        custom: None,
                        submit: false,
                        style: inherited.clone(),
                        pseudo_reactive: reactive,
                        marker: None,
                        white_space,
                        boxed: BoxStyle::default(),
                        runs,
                    }));
                }
                continue;
            }
            tl::Node::Comment(_) => continue,
        };
        // ADR 0021: an unchanged subtree reuses the previous build's item
        // verbatim (handles remapped), skipping the cascade and the run
        // construction. Only whole clean subtrees whose root produced a spec
        // reuse; a node whose cache entry is missing (walk-through tags like
        // `span`/`body` whose content makes anonymous items, raw text,
        // translations) falls through to the normal collection — returning
        // here would drop the subtree's content entirely.
        // ADR 0029: on a scoped restyle, a node on a flipped element's path
        // (itself or an ancestor flipped) re-cascades — its rules may now
        // match differently, or its inherited base moved — while every
        // other clean node's style comes out identical and reuses. So does
        // a node whose inherited base is unchanged (the parent's flag); a
        // node under a flipped ancestor reuses only when no
        // pseudo-referencing selector could match it (the cached spec's
        // `pseudo_reactive`).
        // ADR 0031: the subtree stands outside the band — a spacer with
        // its measured height, no cascade, no spec subtree. The reuse
        // runs FIRST (a culled clean subtree reuses its cached *spacer*
        // spec — no fresh spec, no respawn churn); the cull only catches
        // what the reuse can't.
        let culled_now = ctx
            .window
            .as_ref()
            .zip(window)
            .and_then(|(win, (frame, y))| ctx.cull(win, frame, y, handle));

        if let Some(skeleton) = ctx.skeleton
            && !skeleton.full_style
            && !inherited_changed
            && let Some(&old) = skeleton.pairing.get(&handle)
            && skeleton.clean.contains(&old)
            && !skeleton.flipped.contains(&old)
            && !(ancestor_flipped
                && skeleton
                    .cached_spec(old)
                    .is_some_and(|spec| spec.pseudo_reactive))
            // ADR 0031: a coalesced spacer never reuses — its run's
            // membership shifts with the band, so its height would be
            // stale (it respawns: one entity per run per build). A full
            // spec reuses only while its subtree stays materialized.
            && culled_now.is_none()
            && skeleton
                .cached_spec(old)
                .is_some_and(|spec| spec.spacer.is_none())
            // ADR 0031 (review_0006 J04): a cached subtree carries the
            // culling of the band it was built under. It reuses only when
            // no descendant would cull differently now: it holds no
            // spacer, and — where the cursor tracks it — it lies wholly
            // inside the band. A subtree straddling a band edge
            // re-collects; its children decide (and reuse) one by one.
            && ctx.window.as_ref().is_none_or(|win| {
                // A spec collected without a window (before the first
                // layout) has no `WindowKey`s: reused, its entities would
                // never be measured, and nothing in it would ever cull.
                skeleton
                    .cached_spec(old)
                    .is_some_and(|spec| spec.window_key.is_some() && !spec_has_spacer(spec))
                    && window.is_none_or(|(frame, y)| win.inside(frame, y, handle))
            })
            && skeleton.reuse(old, items)
        {
            if let (Some(win), Some((frame, y))) = (ctx.window.as_ref(), window) {
                win.note_reused(frame, y, handle);
            }
            continue;
        }

        if let Some(spacer) = culled_now {
            items.push(spacer);
            continue;
        }

        let signals = signals::signal_bindings(tag);
        let mut focus = focus::focusable(tag, &signals, false);
        let custom = custom_elements::custom_element(tag);
        let state = ctx
            .states
            .and_then(|states| states.get(&handle))
            .copied()
            .unwrap_or_default();
        let Some(element) = ctx.element(tag) else {
            // Unmatched `data-l10n-name` in a translation: content only.
            let children: Vec<tl::NodeHandle> = tag.children().top().iter().copied().collect();
            for child in children.into_iter().rev() {
                stack.push(Task::Node {
                    handle: child,
                    inherited: inherited.clone(),
                    chain: chain.clone(),
                    grid: None,
                    depth: depth + 1,
                    inherited_changed,
                    ancestor_flipped,
                    li_index: None,
                    list_kind: ListStyleSpec::Disc,
                    position: Position::default(),
                    siblings: None,
                    window: None,
                });
            }
            continue;
        };
        let inline = cascade::inline_style(tag);
        if let Some(inline) = &inline {
            // The parsed tokens' names (non-ASCII and escapes included).
            for name in inline.var_names() {
                ctx.inline_vars.borrow_mut().insert(name);
            }
        }
        // ADR 0028: the parent's siblings list, up to this element (the
        // sibling combinators' input; the parent's task state carries it)
        // — a view, not a copy (review_0005 I03).
        let previous_siblings = siblings
            .as_ref()
            .map(|list| list.preceding(position.tag_index.saturating_sub(1)));
        // This element becomes the nearest ancestor for everything inside
        // it, with its place in the sibling list (ADR 0034: the selector
        // engine counts an ancestor's siblings for its positions).
        let own = Ancestor {
            link: SiblingLink(previous_siblings.clone()),
            ..ancestor_from(element.clone(), state, position)
        };
        // The child chain is shared with every child task (an Rc clone is
        // O(1); a per-child Vec copy made attribute-heavy pages cost
        // O(children x depth) memory, review_0003 G05). `own`'s fields
        // are pulled out first, so the Vec is built only for containers.
        let child_chain = {
            let mut child_chain = Vec::with_capacity(chain.len() + 1);
            child_chain.push(own);
            child_chain.extend(chain.iter().cloned());
            Rc::new(child_chain)
        };
        // The `<html>` element is what the root style was computed from:
        // re-applying it would square its `opacity`.
        let element_state = ElementState {
            previous_siblings,
            ..ElementState::at(position)
        };
        let style = if element.tag == "html" {
            inherited.clone()
        } else {
            ctx.styler.style_of(
                &element,
                &inherited,
                state,
                inline.as_ref(),
                &chain,
                &element_state,
            )
        };
        // ADR 0029: the children's inherited base is this element's fresh
        // computed style; it changed when it differs from the cached
        // spec's (the one being replaced — a node without one has no
        // cached children either, so the flag's value is moot).
        let children_inherited_changed = match ctx.skeleton {
            Some(skeleton) => skeleton
                .pairing
                .get(&handle)
                .and_then(|&old| skeleton.cached_spec(old))
                .is_none_or(|cached| cached.style != style),
            None => true,
        };
        // A flipped element makes its whole subtree re-decide from their
        // cached `pseudo_reactive` (the flip's handle numbering is the
        // cached document's — identical sources parse identically; on a
        // full_style pass nothing reuses, so the flag's value is moot).
        let child_ancestor_flipped = ancestor_flipped
            || ctx
                .skeleton
                .is_some_and(|skeleton| !skeleton.full_style && skeleton.flipped.contains(&handle));
        // ADR 0029: whether a pseudo-referencing selector could match this
        // element — its own style (and, for containers, anything inside
        // it) may change on a flip.
        let own_reactive = ctx.styler.pseudo_reactive(&element, &chain, &element_state);
        let mut boxed = ctx
            .styler
            .box_of(&element, state, inline.as_ref(), &chain, &element_state);
        // ADR 0028: the tables' UA-sheet defaults — a `tr` lays its cells
        // out in a row (a declaration wins: `flex-direction` already
        // applies); a `th`'s text is bold.
        if element.tag == "tr" && boxed.layout.flex_direction.is_none() {
            boxed.layout.flex_direction = Some(FlexDirection::Row);
        }
        // ADR 0027: the tag's list-marker default for this element's
        // *children*, before `element` moves into the container's `Open`
        // (the marker reads the parent's kind, carried by the task — an
        // `li` recomputing this from its own tag would read `disc`).
        let child_list_kind = style.list_style.unwrap_or(match element.tag.as_str() {
            "ol" => ListStyleSpec::Decimal,
            _ => ListStyleSpec::Disc,
        });
        // This element's own placement resolves against its parent's grid
        // (named lines and areas); its own template then backs its children's.
        boxed.layout.resolve_names(grid.as_ref());
        let grid_of_own = boxed.layout.grid_names();
        boxed.drawn_outline = boxed.outline(style.color);
        boxed.fade(style.opacity);
        #[cfg(feature = "text-input")]
        if element.tag == "input" && input_is_text(tag) {
            // An editable text field (ADR 0016): a block with the widget's
            // components; its content is the field's state, not walked markup.
            // Other `type`s stay unsupported (the field is not built). An
            // `<input>` is focusable like a block with `data-on-click` (HTML
            // inputs are focusable), unless `tabindex="-1"`.
            focus = focus::focusable(tag, &signals, true);
            let value = tag
                .attributes()
                .get("value")
                .flatten()
                .map(|value| decode_attribute(value.as_utf8_str()).to_string());
            let name = tag
                .attributes()
                .get("name")
                .flatten()
                .map(|name| decode_attribute(name.as_utf8_str()).to_string());
            // The outer `boxed` already has the placement (`resolve_names`),
            // the outline and the fade applied.
            items.push(Item::TextInput {
                element,
                handle: Some(handle),
                signals,
                focus,
                custom,
                boxed,
                pointer_events: style.pointer_events,
                font_size: style.size,
                style: style.clone(),
                pseudo_reactive: own_reactive,
                keyed: ctx.window.is_some(),
                value: TextInputSpec {
                    initial: value.unwrap_or_default(),
                    font: ctx.styler.text_font(&style),
                    color: faded(style.color, style.opacity),
                    name,
                    max_characters: tag
                        .attributes()
                        .get("maxlength")
                        .flatten()
                        .and_then(|max| max.as_utf8_str().parse().ok()),
                },
            });
            continue;
        }
        // A button: a block carrying its bindings and element; clicking a
        // submit button submits the enclosing form (the marker is on the
        // spawn path, behind the `text-input` feature — without it the
        // Submit signal's payload is just empty).
        // An `<img>` directly in a container (raw HTML or Markdown): an
        // anonymous block carrying its image run — the same treatment a text
        // piece gets. In a block, `push_runs` handles it inline instead.
        if element.tag == "img" {
            let mut runs = Vec::new();
            let mut reactive = own_reactive;
            // ADR 0031: an `<img>`'s anonymous block outside the band.
            if let (Some(win), Some((frame, y))) = (ctx.window.as_ref(), window)
                && let Some(spacer) = ctx.cull(win, frame, y, handle)
            {
                items.push(spacer);
                continue;
            }
            push_runs(
                ctx,
                handle,
                &inherited,
                RunExtras::default(),
                &mut runs,
                child_chain.clone(),
                &mut reactive,
                element_state.clone(),
            );
            if !runs.is_empty() {
                items.push(Item::Block(Block {
                    kind: BlockKind::Paragraph,
                    element: None,
                    handle: None,
                    key: ctx.window.is_some().then_some(handle),
                    signals: Vec::new(),
                    focus: None,
                    custom: None,
                    submit: false,
                    style: inherited.clone(),
                    pseudo_reactive: reactive,
                    marker: None,
                    white_space: inherited.white_space.unwrap_or(WhiteSpaceSpec::Normal),
                    boxed: BoxStyle::default(),
                    runs,
                }));
            }
            continue;
        }

        let is_button = element.tag == "button";
        let kind = match element.tag.as_str() {
            "button" => {
                // HTML buttons are focusable without `data-on-click`.
                focus = focus::focusable(tag, &signals, true);
                BlockKind::Button
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => BlockKind::Heading,
            "p" => BlockKind::Paragraph,
            "li" => BlockKind::ListItem,
            "pre" => BlockKind::Preformatted,
            "head" | "script" | "style" => continue,
            // Containers hold their children's nodes; other elements outside a
            // block (`html`, `body`, unknown tags) are walked through, no node.
            // Either way a translation replaces the children.
            name => {
                let container = CONTAINERS.contains(&name);
                if !container && !signals.is_empty() {
                    debug!(
                        "html signals: <{name}> has data-on-* hooks, but only blocks and \
                         containers become nodes; skipped"
                    );
                }
                if !container && custom.is_some() {
                    debug!(
                        "html custom elements: <{name}> has an is attribute, but only blocks \
                         and containers become nodes; skipped"
                    );
                }
                // The container completes after its children (post-order):
                // it is pushed first, so its children's tasks (pushed
                // above it) pop — and append their items — before it.
                let start = items.len();
                // ADR 0031: the children's cursor — a new frame when this
                // container is one the vertical cursor can track (a
                // column, unwrapped, packed from the start, no
                // out-of-flow or auto-margin children); a walk-through
                // passes its own frame through (its children continue the
                // parent's cursor — usually already invalidated, since a
                // walk-through has no model entry).
                let child_window = if container {
                    ctx.window
                        .as_ref()
                        .zip(window)
                        .and_then(|(win, (parent_frame, own_y))| {
                            // The `Node` this container gets (a column
                            // under its declared layout), decided now: a
                            // restyle into a grid stops the cursor in its
                            // own frame (a mark measured from the laid-out
                            // node came a frame late, and once set never
                            // cleared — bug_0171).
                            let mut probe = Node {
                                flex_direction: FlexDirection::Column,
                                ..default()
                            };
                            boxed.layout.apply_to(&mut probe);
                            let opaque = children_are_opaque(&probe);
                            let Some(&(margin_top, _)) = win.model.heights.get(&handle) else {
                                return None;
                            };
                            let gap = boxed.row_gap.unwrap_or(win.default_gap);
                            let top = own_y
                                + margin_top
                                + boxed.border[0].unwrap_or(0.0)
                                + boxed.padding[0].unwrap_or(0.0);
                            // `top` already holds the margin (passing it
                            // again counted it twice — review_0006).
                            win.frame_for(parent_frame, top, gap, opaque)
                                .map(|frame| (frame, own_y))
                        })
                } else {
                    window
                };
                if container {
                    stack.push(Task::Complete(Box::new(Open {
                        element,
                        handle,
                        signals,
                        focus,
                        custom,
                        boxed,
                        pointer_events: style.pointer_events,
                        font_size: style.size,
                        start,
                        style: style.clone(),
                        own_reactive,
                    })));
                }
                let translated = walk_translation(ctx, handle, tag, |fragment, nodes| {
                    // ADR 0028: the fragment's top-level elements' positions
                    // (the fragment is the translation's own document).
                    let nodes: Vec<tl::NodeHandle> = nodes.iter().copied().collect();
                    let positions = sibling_positions(&nodes, fragment);
                    let fragment_siblings = sibling_list(&nodes, &positions, fragment);
                    for node in nodes {
                        let position = positions.get(&node).copied().unwrap_or_default();
                        collect_node(
                            fragment,
                            node,
                            &style,
                            items,
                            child_chain.clone(),
                            grid_of_own.as_ref(),
                            depth + 1,
                            position,
                            Some(fragment_siblings.clone()),
                            None,
                        );
                    }
                    // The fragment's inline-var names feed the parent's
                    // accumulator (review_0003 G30).
                    let fragment_names = fragment.inline_vars.borrow().clone();
                    for name in fragment_names {
                        ctx.inline_vars.borrow_mut().insert(name);
                    }
                });
                if !translated {
                    // Reversed: the leftmost child pops first.
                    let children: Vec<tl::NodeHandle> =
                        tag.children().top().iter().copied().collect();
                    // ADR 0028: each child's position among the parent's
                    // element children (the `:nth-child` inputs).
                    let positions = sibling_positions(&children, ctx);
                    // ADR 0027: the list's marker state (computed above:
                    // the computed `list-style-type`, with the UA-sheet
                    // tag default) and each `li` child's 1-based position.
                    // ADR 0028: the children's `Ancestor`s too (the
                    // sibling combinators' input, shared per container).
                    let child_siblings = sibling_list(&children, &positions, ctx);
                    let mut li_count = 0usize;
                    let indexed: Vec<_> = children
                        .iter()
                        .copied()
                        .map(|child| {
                            let child_li = matches!(
                                child.get(ctx.parser),
                                Some(tl::Node::Tag(tag)) if tag.name().as_utf8_str() == *"li"
                            );
                            let index = if child_li {
                                li_count += 1;
                                Some(li_count)
                            } else {
                                None
                            };
                            (child, index)
                        })
                        .collect();
                    // ADR 0031: the children's margin-box tops, in order;
                    // an unknown height invalidates the frame from there
                    // on (the remaining y's are garbage but unused — an
                    // invalidated frame culls nothing).
                    let ys: Vec<f32> = child_window
                        .as_ref()
                        .zip(ctx.window.as_ref())
                        .map(|((frame_idx, own_y), win)| {
                            let frames = win.frames.borrow();
                            let frame = frames.get(*frame_idx).expect("own frame");
                            let mut ys = Vec::with_capacity(indexed.len());
                            // A container's frame starts at its content
                            // top; a walk-through's children continue its
                            // parent's column from its own top.
                            let mut cursor = if container { frame.y.get() } else { *own_y };
                            let children: Vec<tl::NodeHandle> =
                                indexed.iter().map(|(child, _)| *child).collect();
                            let estimate = win.estimate(&children);
                            for child in &children {
                                ys.push(cursor);
                                match column_height(&ctx, win, *child, frame.gap, depth + 1)
                                    .or_else(|| win.estimate_for(*child, estimate, frame.gap))
                                {
                                    Some(step) => cursor += step,
                                    None => frame.valid.set(false),
                                }
                            }
                            ys
                        })
                        .unwrap_or_default();
                    for ((child, index), y) in indexed
                        .into_iter()
                        .zip(ys.into_iter().chain(std::iter::repeat(f32::NAN)))
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                    {
                        stack.push(Task::Node {
                            handle: child,
                            inherited: style.clone(),
                            chain: child_chain.clone(),
                            grid: grid_of_own.clone(),
                            depth: depth + 1,
                            inherited_changed: children_inherited_changed,
                            ancestor_flipped: child_ancestor_flipped,
                            li_index: index,
                            list_kind: child_list_kind,
                            position: positions.get(&child).copied().unwrap_or_default(),
                            siblings: Some(Rc::clone(&child_siblings)),
                            window: child_window.as_ref().map(|(frame, _)| (*frame, y)),
                        });
                    }
                }
                continue;
            }
        };
        // A missing translation falls back to the element's own content.
        // Only a submit button *of a form with a submit hook* carries the
        // marker: any button is a submit button in HTML, but outside such
        // a form it has no submit semantics — and `covered_by_deeper`
        // counts the button as bound, which would swallow the enclosing
        // wrapper's click.
        let submit =
            is_button && button_submits(tag) && chain.iter().any(|ancestor| ancestor.submit);
        let mut runs = Vec::new();
        let mut reactive = own_reactive;
        push_content_runs(
            ctx,
            handle,
            tag,
            &style,
            RunExtras::default(),
            &mut runs,
            child_chain.clone(),
            &mut reactive,
        );
        // ADR 0028: the block's `white-space` — a declaration beats the
        // `pre` tag's UA default (preserve, no wrap).
        let white_space = style.white_space.unwrap_or(match kind {
            BlockKind::Preformatted => WhiteSpaceSpec::Preserve,
            _ => WhiteSpaceSpec::Normal,
        });
        // ADR 0027: the `li`'s marker — its own computed
        // `list-style-type` (an own declaration beats the inherited
        // list's; `None` falls back to the parent's tag default)
        // numbered by the parent list's counter.
        let marker = (kind == BlockKind::ListItem).then(|| {
            let list_style = style.list_style.unwrap_or(list_kind);
            match list_style {
                ListStyleSpec::Disc => "• ".to_owned(),
                ListStyleSpec::Circle => "◦ ".to_owned(),
                ListStyleSpec::Square => "▪ ".to_owned(),
                ListStyleSpec::Decimal => li_index.map_or(String::new(), |n| format!("{n}. ")),
                ListStyleSpec::None => String::new(),
            }
        });
        items.push(Item::Block(Block {
            kind,
            element: Some(element),
            handle: Some(handle),
            key: ctx.window.is_some().then_some(handle),
            signals,
            focus,
            custom,
            style,
            pseudo_reactive: reactive,
            marker,
            white_space,
            submit,
            boxed,
            runs: finish_runs(runs, white_space),
        }));
    }
}
/// Whether a spec subtree holds a windowing spacer (it was built under a
/// band that culled part of it).
fn spec_has_spacer(spec: &NodeSpec) -> bool {
    let mut stack: Vec<&NodeSpec> = vec![spec];
    while let Some(spec) = stack.pop() {
        if spec.spacer.is_some() {
            return true;
        }
        stack.extend(spec.children.iter().map(|child| child.spec().as_ref()));
    }
    false
}

/// ADR 0031: consecutive culled siblings coalesce into one spacer, so a
/// flat document of culled blocks costs taffy one node per run, not one
/// per block. The run's key = its first member's (the height = the
/// derived sum; the members' own sticky heights re-derive it).
fn coalesce_spacers(items: Vec<Item>) -> Vec<Item> {
    let mut coalesced: Vec<Item> = Vec::with_capacity(items.len());
    for item in items {
        if let (Item::Spacer { height, gap }, Some(Item::Spacer { height: total, .. })) =
            (&item, coalesced.last_mut())
        {
            // The column spaced the two members by one gap; the merged
            // node must keep it (bug_0151).
            *total += *gap + *height;
            continue;
        }
        coalesced.push(item);
    }
    coalesced
}

/// ADR 0031: a node that produces no item, so no height and no space: text
/// that collapses away entirely (ASCII whitespace only, as the collapsing
/// is — an NBSP is content), a comment, or a skipped tag (`head`,
/// `script`, `style`). A comment or a `head` invalidated the cursor.
fn is_itemless(ctx: &Ctx, handle: tl::NodeHandle) -> bool {
    match handle.get(ctx.parser) {
        Some(tl::Node::Raw(text)) => text
            .as_utf8_str()
            .bytes()
            .all(|byte| matches!(byte, b' ' | b'\t' | b'\n' | b'\x0c' | b'\r')),
        Some(tl::Node::Comment(_)) => true,
        Some(tl::Node::Tag(tag)) => {
            matches!(
                tag.name().as_utf8_str().to_ascii_lowercase().as_str(),
                "head" | "script" | "style"
            )
        }
        None => false,
    }
}

/// ADR 0031: a tag the walk passes through without a node of its own
/// (`html`, `body`, unknown tags): not a container, block, widget or
/// skipped tag.
fn is_walked_through(ctx: &Ctx, handle: tl::NodeHandle) -> bool {
    let Some(tl::Node::Tag(tag)) = handle.get(ctx.parser) else {
        return false;
    };
    let name = tag.name().as_utf8_str().to_ascii_lowercase();
    !CONTAINERS.contains(&name.as_str())
        && !matches!(
            name.as_str(),
            "h1" | "h2"
                | "h3"
                | "h4"
                | "h5"
                | "h6"
                | "p"
                | "li"
                | "pre"
                | "button"
                | "input"
                | "img"
                | "head"
                | "script"
                | "style"
        )
}

/// ADR 0031: how far `handle` advances its column's cursor — its measured
/// outer height plus the column's `gap`; nothing for an itemless node; for
/// a walked-through element (no node of its own: its children are the
/// column's items) the sum over its children. `None`: a height the cursor
/// doesn't know (the frame stops culling from there).
fn column_height(
    ctx: &Ctx,
    win: &WindowCtx,
    handle: tl::NodeHandle,
    gap: f32,
    depth: usize,
) -> Option<f32> {
    if let Some(&(_, outer)) = win.model.heights.get(&handle) {
        return Some(outer + gap);
    }
    if is_itemless(ctx, handle) {
        return Some(0.0);
    }
    if depth >= MAX_DOM_DEPTH || !is_walked_through(ctx, handle) {
        return None;
    }
    let Some(tl::Node::Tag(tag)) = handle.get(ctx.parser) else {
        return None;
    };
    // A translation replaces the children: the DOM's say nothing.
    if tag.attributes().get("data-l10n-id").flatten().is_some() {
        return None;
    }
    tag.children().top().iter().try_fold(0.0, |sum, child| {
        Some(sum + column_height(ctx, win, *child, gap, depth + 1)?)
    })
}

/// Tag name (lowercase), `id` and `class` of an element.
fn element_of(tag: &tl::HTMLTag) -> HtmlElement {
    let attribute = |key: &str| {
        tag.attributes()
            .get(key)
            .flatten()
            .map(|value| decode_attribute(value.as_utf8_str()).into_owned())
    };
    HtmlElement {
        tag: tag.name().as_utf8_str().to_ascii_lowercase(),
        id: attribute("id"),
        classes: attribute("class")
            .map(|classes| classes.split_whitespace().map(str::to_owned).collect())
            .unwrap_or_default(),
        dataset: custom_elements::dataset(tag),
    }
}

/// An element known only by its tag (`html` root, `pre` for outlines).
pub(crate) fn element_tag(tag: &str) -> HtmlElement {
    HtmlElement {
        tag: tag.to_owned(),
        ..default()
    }
}

/// Runs for `tag`'s content: its translation if it has one, else its
/// children. Translations are markup (fluent-dom style overlays): inline
/// elements in them are styled by the stylesheet like elements in the
/// document; entities are decoded.
fn push_content_runs(
    ctx: &Ctx,
    handle: tl::NodeHandle,
    tag: &tl::HTMLTag,
    style: &Style,
    extras: RunExtras,
    runs: &mut Vec<Run>,
    chain: Rc<Vec<Ancestor>>,
    reactive: &mut bool,
) {
    let translated = walk_translation(ctx, handle, tag, |fragment, nodes| {
        let nodes: Vec<tl::NodeHandle> = nodes.iter().copied().collect();
        let positions = sibling_states(&nodes, fragment);
        run_walk(
            fragment,
            nodes.into_iter(),
            style,
            &extras,
            runs,
            Rc::new(chain[..].to_vec()),
            reactive,
            &positions,
        );
        // The fragment's inline-var names feed the parent's accumulator
        // (review_0003 G30): translations rebuild only when a tracked
        // change fires, so an untracked change never restyled them.
        let fragment_names = fragment.inline_vars.borrow().clone();
        for name in fragment_names {
            ctx.inline_vars.borrow_mut().insert(name);
        }
    });
    if !translated {
        let children: Vec<tl::NodeHandle> = tag.children().top().iter().copied().collect();
        let positions = sibling_states(&children, ctx);
        run_walk(
            ctx,
            children.into_iter(),
            style,
            &extras,
            runs,
            chain,
            reactive,
            &positions,
        );
    }
}

/// The runs walk over a set of roots, iterative (an explicit stack; a
/// node's children are pushed in reverse so the visit order — and with it
/// the run merge order and the named-overlay consumption — is the same
/// depth-first document order the recursion visited). The tree's depth
/// costs heap, not stack.
fn run_walk<'a>(
    ctx: &Ctx<'a, '_, '_>,
    roots: impl IntoIterator<Item = tl::NodeHandle>,
    style: &Style,
    extras: &RunExtras,
    runs: &mut Vec<Run>,
    chain: Rc<Vec<Ancestor>>,
    reactive: &mut bool,
    positions: &HashMap<tl::NodeHandle, ElementState>,
) {
    // The chain is shared (an Rc clone is O(1)); the depth caps how deep
    // the chain grows — inline nesting used to be uncapped, and every
    // level cloned the whole chain plus hashed it for the style cache
    // (20k nested `<b>` cost ~4×10⁸ ancestor copies, review_0003 G06).
    // Past MAX_DOM_DEPTH the text is kept, the ancestor contribution
    // stops — the same rule as the block walk.
    let mut stack: Vec<(
        tl::NodeHandle,
        Style,
        RunExtras,
        Rc<Vec<Ancestor>>,
        usize,
        ElementState,
    )> = roots
        .into_iter()
        .map(|handle| {
            let state = positions.get(&handle).cloned().unwrap_or_default();
            (
                handle,
                style.clone(),
                extras.clone(),
                chain.clone(),
                0,
                state,
            )
        })
        .collect();
    // Reversed: the last entry pops first, so the first root (and, in the
    // tag arm, the leftmost child) is visited first.
    stack.reverse();
    while let Some((handle, style, extras, chain, depth, element_state)) = stack.pop() {
        match handle.get(ctx.parser) {
            Some(tl::Node::Raw(text)) => {
                let text = decode_text(text.as_utf8_str());
                // Capitalization continues a word across inline elements
                // (`<b>foo</b>bar` is one word): the block's previous
                // character is the context, as in Blink.
                let previous = runs.last().and_then(|run| run.text.chars().last());
                let text = transform_text(&text, style.text_transform, previous);
                match runs.last_mut() {
                    Some(last)
                        if last.style == style && last.extras == extras && last.image.is_none() =>
                    {
                        last.text.push_str(&text);
                    }
                    _ => runs.push(Run {
                        text,
                        style,
                        extras,
                        image: None,
                    }),
                }
            }
            Some(tl::Node::Tag(tag)) => {
                // The tag's own interaction state (`:hover` on a link span,
                // say): entities with `DomNode` carry it (the link spans do).
                let state = ctx
                    .states
                    .and_then(|states| states.get(&handle))
                    .copied()
                    .unwrap_or_default();
                // The element, computed once: `element` consumes a named
                // overlay's entry (each usable once), so all uses share it.
                let element = ctx.element(tag);
                // Run-level inline styles' var() names count too (G30:
                // only block-level ones were tracked — an inline span
                // with `style="color: var(--brand)"` kept the old color).
                if let Some(inline) = cascade::inline_style(tag) {
                    for name in inline.var_names() {
                        ctx.inline_vars.borrow_mut().insert(name);
                    }
                }
                // This element becomes the nearest ancestor for its content.
                let child_chain = if depth < MAX_DOM_DEPTH {
                    let owned = element.clone();
                    let mut child_chain = Vec::with_capacity(chain.len() + 1);
                    child_chain.push(match owned {
                        Some(owned) => Ancestor {
                            link: SiblingLink(element_state.previous_siblings.clone()),
                            ..ancestor_from(owned, state, element_state.position())
                        },
                        None => Ancestor {
                            pseudo: state,
                            position: element_state.position(),
                            link: SiblingLink(element_state.previous_siblings.clone()),
                            ..Ancestor::default()
                        },
                    });
                    child_chain.extend(chain.iter().cloned());
                    Rc::new(child_chain)
                } else {
                    chain.clone()
                };
                if tag.name().as_utf8_str() == *"img" {
                    // The run's size styles from the image's CSS — reactive
                    // to a flip like any other run (ADR 0029).
                    if let Some(element) = &element {
                        *reactive |=
                            ctx.styler
                                .pseudo_reactive(element, &chain, &ElementState::default());
                    }
                    push_image_run(
                        ctx,
                        tag,
                        &element,
                        state,
                        &style,
                        extras,
                        runs,
                        &chain,
                        &element_state,
                    );
                    continue;
                }
                // An unmatched `data-l10n-name` element is plain text.
                let style = match &element {
                    Some(element) => {
                        let inline = cascade::inline_style(tag);
                        let styled = ctx.styler.style_of(
                            element,
                            &style,
                            state,
                            inline.as_ref(),
                            &chain,
                            &element_state,
                        );
                        // ADR 0029: the run carries this element's style —
                        // the owning block re-cascades on a flip when the
                        // element could react to one.
                        *reactive |= ctx.styler.pseudo_reactive(element, &chain, &element_state);
                        styled
                    }
                    None => style,
                };
                // This element's `background`/`text-decoration` over the
                // subtree's running values, faded with its opacity.
                let inherited_link = extras.link.clone();
                let mut extras = element
                    .as_ref()
                    .and_then(|element| {
                        let inline = cascade::inline_style(tag);
                        ctx.styler.run_extras(
                            element,
                            state,
                            inline.as_ref(),
                            &chain,
                            &element_state,
                        )
                    })
                    .map(|extras| extras.faded(style.opacity))
                    .unwrap_or(extras);
                // A link: its runs become picked span entities that emit the
                // `open` signal with the href when clicked (plus the element's
                // own `data-on-*` hooks). No href → plain styled text, as in
                // browsers. Nested elements inherit the enclosing link (their
                // own extras would otherwise replace it), so a non-link
                // element's extras keep the inherited one.
                if tag.name().as_utf8_str() == *"a"
                    && let Some(href) = tag.attributes().get("href").flatten()
                {
                    let href = decode_attribute(href.as_utf8_str());
                    if !href.is_empty() {
                        let mut bindings = signals::signal_bindings(tag);
                        bindings.push(SignalBinding {
                            trigger: SignalTrigger::Click,
                            name: "open".to_owned(),
                            payload: serde_json::json!({ "href": href }),
                        });
                        extras.link = Some(LinkRun {
                            bindings,
                            element: element_of(tag),
                            handle,
                        });
                    } else {
                        extras.link = inherited_link;
                    }
                } else {
                    extras.link = inherited_link;
                }
                // A translation's markup replaces the element's content; its
                // walk is a nested `run_walk` (a fragment context has no
                // translations of its own, so this never nests further).
                // Own children are pushed in reverse, so the next pops are
                // the leftmost subtree: the same depth-first document order
                // the recursion visited.
                let translated = walk_translation(ctx, handle, tag, |fragment, nodes| {
                    let nodes: Vec<tl::NodeHandle> = nodes.iter().copied().collect();
                    let nested = sibling_states(&nodes, fragment);
                    run_walk(
                        fragment,
                        nodes.into_iter(),
                        &style,
                        &extras,
                        runs,
                        child_chain.clone(),
                        reactive,
                        &nested,
                    );
                    // The fragment's inline-var names feed the parent's
                    // accumulator: translations rebuild only when a tracked
                    // change fires, so an untracked change never restyled
                    // them (review_0003 G30).
                    let fragment_names = fragment.inline_vars.borrow().clone();
                    for name in fragment_names {
                        ctx.inline_vars.borrow_mut().insert(name);
                    }
                });
                if !translated {
                    let children: Vec<tl::NodeHandle> =
                        tag.children().top().iter().copied().collect();
                    let child_positions = sibling_states(&children, ctx);
                    for child in children.into_iter().rev() {
                        let child_state = child_positions.get(&child).cloned().unwrap_or_default();
                        stack.push((
                            child,
                            style.clone(),
                            extras.clone(),
                            child_chain.clone(),
                            depth + 1,
                            child_state,
                        ));
                    }
                }
            }
            Some(tl::Node::Comment(_)) | None => {}
        }
    }
}

fn push_runs(
    ctx: &Ctx,
    handle: tl::NodeHandle,
    style: &Style,
    extras: RunExtras,
    runs: &mut Vec<Run>,
    chain: Rc<Vec<Ancestor>>,
    reactive: &mut bool,
    state: ElementState,
) {
    // The root's own structural state (the caller's: a text node has
    // none; its children's compute in the walk).
    let positions = HashMap::from_iter([(handle, state)]);
    run_walk(
        ctx,
        std::iter::once(handle),
        style,
        &extras,
        runs,
        chain,
        reactive,
        &positions,
    );
}

/// ADR 0028: `text-transform` applied to the run's text at collection
/// (post-`decode`, pre-collapse: the transform never produces or removes
/// whitespace).
fn transform_text(text: &str, transform: TextTransformSpec, previous: Option<char>) -> String {
    match transform {
        TextTransformSpec::None => text.to_owned(),
        // Full case mappings (`ß` → `SS`) and the final-sigma rule.
        TextTransformSpec::Uppercase => text.to_uppercase(),
        TextTransformSpec::Lowercase => text.to_lowercase(),
        TextTransformSpec::Capitalize => capitalize(text, previous),
    }
}

/// `text-transform: capitalize` as Chrome renders it (checked against it
/// by the browser oracle, `tests/vectors/text_transform_unicode`): the
/// first character of each word takes its simple titlecase mapping
/// (`ǆ` → `ǅ`; `ß`, `ŉ`, `ﬁ` stay, having no single-character titlecase).
/// Words are ICU's (UAX #29: apostrophes and `·` join, a word starting
/// with a digit has nothing to capitalize, CJK breaks from Latin) plus a
/// break around `.` and `:` between letters, where Chrome's ICU is
/// tailored to break (`A.B.C`, `O:Clock`). `previous` is the character
/// before `text` in the block, so a word continues across elements.
fn capitalize(text: &str, previous: Option<char>) -> String {
    use icu_casemap::CaseMapperBorrowed;
    use icu_segmenter::WordSegmenter;
    use icu_segmenter::options::WordBreakInvariantOptions;

    let mut probe = String::with_capacity(text.len() + 4);
    probe.extend(previous);
    let offset = probe.len();
    probe.push_str(text);
    let mut starts: Vec<usize> = WordSegmenter::new_auto(WordBreakInvariantOptions::default())
        .segment_str(&probe)
        .collect();
    let chars: Vec<(usize, char)> = probe.char_indices().collect();
    for (index, &(at, character)) in chars.iter().enumerate() {
        if matches!(
            character,
            '.' | ':' | '\u{FE52}' | '\u{FE55}' | '\u{FF0E}' | '\u{FF1A}'
        ) && index > 0
            && chars[index - 1].1.is_alphabetic()
            && chars
                .get(index + 1)
                .is_some_and(|(_, next)| next.is_alphabetic())
        {
            starts.push(at);
            starts.push(at + character.len_utf8());
        }
    }
    starts.sort_unstable();
    let case = CaseMapperBorrowed::new();
    let mut out = String::with_capacity(text.len());
    for &(at, character) in &chars {
        if at < offset {
            continue;
        }
        out.push(if starts.binary_search(&at).is_ok() {
            case.simple_titlecase(character)
        } else {
            character
        });
    }
    out
}

/// Whitespace handling for a block's runs (ADR 0028): collapsed as in
/// HTML's `normal`/`nowrap`, or — for the preserved kinds (`pre`,
/// `pre-wrap`) — kept, minus a newline right after the start tag and
/// trailing whitespace.
fn finish_runs(mut runs: Vec<Run>, white_space: WhiteSpaceSpec) -> Vec<Run> {
    if !matches!(
        white_space,
        WhiteSpaceSpec::Preserve | WhiteSpaceSpec::PreserveWrap
    ) {
        return collapse_runs(runs);
    }
    if let Some(first) = runs.first_mut() {
        let stripped = first
            .text
            .strip_prefix("\r\n")
            .or_else(|| first.text.strip_prefix('\n'));
        if let Some(stripped) = stripped {
            first.text = stripped.to_owned();
        }
    }
    if let Some(last) = runs.last_mut() {
        let trimmed = last.text.trim_end_matches(is_html_whitespace).len();
        last.text.truncate(trimmed);
    }
    runs.retain(|run| !run.text.is_empty() || run.image.is_some());
    runs
}

/// An `<img>` attribute in px (`width`/`height`); `%` and other units are
/// unsupported (Bevy's `InlineImage` takes logical pixels).
/// The `<img>` inline run: the image flows inline as an `InlineBox`/
/// `InlineImage` child of the block's `Text`. Unsupported sources (URLs,
/// data URIs) and a missing `src` attribute are skipped.
fn push_image_run(
    ctx: &Ctx,
    tag: &tl::HTMLTag,
    element: &Option<HtmlElement>,
    state: Pseudo,
    style: &Style,
    extras: RunExtras,
    runs: &mut Vec<Run>,
    ancestors: &[Ancestor],
    element_state: &ElementState,
) {
    let Some(image) = element.as_ref().and_then(|element| {
        ctx.styler
            .image_of(tag, element, state, ancestors, element_state)
    }) else {
        return;
    };
    runs.push(Run {
        text: String::new(),
        style: style.clone(),
        extras,
        image: Some(image),
    });
}

/// Sizes an `<img>` with a CSS-declared `width`/`height`
/// ([`InlineImageSized`]) against its containing block's content box (the
/// last frame's layout — the same slot
/// [`place_anchored`](crate::anchor::place_anchored) reads). Percentages
/// resolve against the content box; one dimension scales with the aspect
/// ratio preserved; both stretch. Runs every frame but writes only
/// changes, so a settled layout stops touching the entities (and
/// `width: 100%` of an image-sized container is the CSS cyclic case: it
/// converges when the container's width comes from elsewhere — a fixed
/// root or a sibling — and may thrash otherwise).
#[hotpath::measure]
pub(crate) fn size_images(
    mut images: Query<(
        &ChildOf,
        &mut InlineBox,
        &mut InlineImage,
        &InlineImageSized,
    )>,
    blocks: Query<&ComputedNode>,
    assets: Res<Assets<Image>>,
) {
    for (parent, mut box_, mut image, sized) in &mut images {
        // Percentages resolve against the containing block's *used* content
        // box (`size` minus borders and padding — `content_size` is the
        // content-driven size, which is 0 for a block stretched by flex and
        // would fix `width: 100%` at zero).
        let Some(block) = blocks.get(parent.0).ok() else {
            continue;
        };
        // `size` is the border box; the insets (`min_inset` carries the
        // resolved border and padding per edge) come out of it.
        let inset = block.border + block.padding;
        let content = Vec2::new(
            (block.size.x - inset.min_inset.x - inset.max_inset.x).max(0.0),
            (block.size.y - inset.min_inset.y - inset.max_inset.y).max(0.0),
        );
        let Some(intrinsic) = assets.get(&image.image).map(|image| image.size().as_vec2()) else {
            continue;
        };
        let resolve = |size: ImageSize, base: f32| match size {
            ImageSize::Px(px) => px,
            ImageSize::Percent(fraction) => fraction * base,
        };
        let target = match (sized.width, sized.height) {
            (Some(w), Some(h)) => Vec2::new(resolve(w, content.x), resolve(h, content.y)),
            (Some(w), None) => {
                let width = resolve(w, content.x);
                Vec2::new(width, width * intrinsic.y / intrinsic.x)
            }
            (None, Some(h)) => {
                let height = resolve(h, content.y);
                Vec2::new(height * intrinsic.x / intrinsic.y, height)
            }
            (None, None) => continue,
        };
        if box_.size != target {
            box_.size = target;
        }
        if image.width != Some(target.x) || image.height != Some(target.y) {
            image.width = Some(target.x);
            image.height = Some(target.y);
        }
    }
}

/// HTML whitespace (space, tab, LF, FF, CR): what collapsing and trimming
/// touch. NBSP, U+3000 and other Unicode spaces are content.
fn is_html_whitespace(c: char) -> bool {
    c.is_ascii_whitespace()
}

/// HTML whitespace collapsing across run boundaries: whitespace sequences
/// become one space, leading/trailing space of the block is dropped, and
/// emptied runs are removed.
fn collapse_runs(runs: Vec<Run>) -> Vec<Run> {
    let mut out: Vec<Run> = Vec::with_capacity(runs.len());
    // Start as if after a space so the block's leading whitespace is dropped.
    let mut after_space = true;
    for run in runs {
        if let Some(image) = run.image {
            // An inline image ends the whitespace sequence: spaces beside it
            // are significant, like around a word.
            out.push(Run {
                text: String::new(),
                style: run.style,
                extras: run.extras,
                image: Some(image),
            });
            after_space = false;
            continue;
        }
        let mut text = String::with_capacity(run.text.len());
        for c in run.text.chars() {
            if is_html_whitespace(c) {
                if !after_space {
                    text.push(' ');
                    after_space = true;
                }
            } else {
                text.push(c);
                after_space = false;
            }
        }
        if !text.is_empty() {
            out.push(Run {
                text,
                style: run.style,
                extras: run.extras,
                image: None,
            });
        }
    }
    if let Some(last) = out.last_mut().filter(|last| last.image.is_none()) {
        let trimmed = last.text.trim_end_matches(is_html_whitespace).len();
        last.text.truncate(trimmed);
        if last.text.is_empty() {
            out.pop();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;
    use lightningcss::values::easing::EasingFunction as CssEasing;
    use std::time::Duration;

    /// ADR 0031: the measured heights are logical pixels — `ComputedNode`
    /// sizes are physical, and on a HiDPI display (the inverse scale
    /// factor < 1) an unconverted model is the display's scale times too
    /// tall: the band maps every scroll offset to rows deeper than the
    /// viewport, and the visible list empties as the user scrolls.
    #[test]
    fn window_heights_are_measured_in_logical_pixels() {
        let mut world = World::new();
        // A UI root and a laid-out child at a 2x display: the child's
        // physical height is 40 px; logical is 20.
        let root = world
            .spawn((
                HtmlUi::new(Handle::default()),
                HtmlWindowed::default(),
                WindowModel::default(),
                ComputedNode {
                    size: Vec2::new(600.0, 400.0),
                    inverse_scale_factor: 0.5,
                    ..default()
                },
            ))
            .id();
        let child = world
            .spawn((
                WindowKey(tl::NodeHandle::new(0)),
                Node::default(),
                // The layout sets both; the measurement reads the
                // unrounded one (bug_0176).
                ComputedNode {
                    size: Vec2::new(110.0, 40.0),
                    unrounded_size: Vec2::new(110.0, 40.0),
                    inverse_scale_factor: 0.5,
                    ..default()
                },
                ChildOf(root),
            ))
            .id();
        world.run_system_once(measure_window_heights).unwrap();
        let model = world.get::<WindowModel>(root).expect("the root's model");
        assert_eq!(model.width, 300.0, "the root's content width, logical");
        assert_eq!(
            model.heights.get(&tl::NodeHandle::new(0)),
            Some(&(0.0, 20.0)),
            "the child's outer height, logical (40 physical at a 2x display)"
        );
        let _ = child;
    }

    /// The easing curves' endpoints are exact and their midpoints sane:
    /// linear is identity; the named cubic Béziers pass through 0 and 1
    /// and stay within (0, 1) inside; a step function jumps.
    #[test]
    fn easing_curves_have_exact_endpoints_and_sane_middles() {
        for easing in [
            EasingFunction::Linear,
            CssEasing::Ease,
            CssEasing::EaseIn,
            CssEasing::EaseOut,
            CssEasing::EaseInOut,
            CssEasing::CubicBezier {
                x1: 0.3,
                y1: 1.5,
                x2: 0.7,
                y2: -0.5,
            },
        ] {
            assert_eq!(eased(0.0, &easing), 0.0, "{easing:?}");
            assert_eq!(eased(1.0, &easing), 1.0, "{easing:?}");
            // Monotone in x-input by construction; a sane midpoint (except
            // the deliberately overshooting custom curve, which swings
            // past 1 and back).
            let middle = eased(0.5, &easing);
            if !matches!(easing, CssEasing::CubicBezier { .. }) {
                assert!((0.0..=1.0).contains(&middle), "{easing:?}: {middle}");
            }
        }
        // Linear is identity everywhere.
        for t in [0.1, 0.25, 0.5, 0.75, 0.9] {
            assert!((eased(t, &EasingFunction::Linear) - t).abs() < 1e-6);
        }
        // Steps: end stays on the floor of the interval; start rises at 0.
        let end = EasingFunction::Steps {
            count: 2,
            position: StepPosition::End,
        };
        assert_eq!(eased(0.4, &end), 0.0);
        assert_eq!(eased(0.6, &end), 0.5);
        assert_eq!(eased(1.0, &end), 1.0);
        let start = EasingFunction::Steps {
            count: 2,
            position: StepPosition::Start,
        };
        assert_eq!(eased(0.0, &start), 0.0);
        assert_eq!(eased(0.1, &start), 0.5);
    }

    /// A background color transition: the entry animates from the current
    /// value (written so the target never shows early) to the target, and
    /// finished entries are dropped with the target in place.
    #[test]
    fn advance_transitions_interpolates_and_finishes() {
        let from = Color::srgb(0.0, 0.0, 0.0);
        let to = Color::srgb(1.0, 1.0, 1.0);
        let transition = |start: f32, duration: f32, delay: f32| {
            Transitions(vec![ActiveTransition {
                property: TransitionProp::BackgroundColor,
                from: [from; 4],
                to: [to; 4],
                start,
                duration,
                delay,
                easing: CssEasing::Linear,
            }])
        };

        // Mid-animation: halfway between, at t = 0.5 of a 1s linear one.
        let mut world = World::new();
        world.insert_resource::<Time>(Time::default());
        let entity = world
            .spawn((transition(0.0, 1.0, 0.0), BackgroundColor(from)))
            .id();
        // Simulate 0.5s of elapsed time.
        let mut time = world.resource_mut::<Time>();
        let _ = time.advance_by(Duration::from_secs_f32(0.5));
        drop(time);
        world.run_system_once(super::advance_transitions).unwrap();
        let color = world.get::<BackgroundColor>(entity).unwrap().0;
        let (
            Srgba {
                red, green, blue, ..
            },
            _,
        ) = (color.to_srgba(), ());
        assert!((red - 0.5).abs() < 1e-4, "{red}");
        assert!((green - 0.5).abs() < 1e-4, "{green}");
        assert!((blue - 0.5).abs() < 1e-4, "{blue}");
        // Still running: the entry stays.
        assert_eq!(world.get::<Transitions>(entity).unwrap().0.len(), 1);

        // Past the end: the target is written and the entry is gone.
        let mut time = world.resource_mut::<Time>();
        let _ = time.advance_by(Duration::from_secs_f32(1.0));
        drop(time);
        world.run_system_once(super::advance_transitions).unwrap();
        assert_eq!(world.get::<Transitions>(entity).unwrap().0, Vec::new());
        assert_eq!(world.get::<BackgroundColor>(entity).unwrap().0, to);

        // Delay: at `now` = 2.0 (the clock accumulated above), an entry
        // that started 0.2s ago with a 1.5s delay hasn't begun animating.
        let entity = world
            .spawn((transition(1.8, 1.0, 1.5), BackgroundColor(from)))
            .id();
        let mut time = world.resource_mut::<Time>();
        let _ = time.advance_by(Duration::from_secs_f32(0.5));
        drop(time);
        world.run_system_once(super::advance_transitions).unwrap();
        assert_eq!(world.get::<BackgroundColor>(entity).unwrap().0, from);
        assert_eq!(world.get::<Transitions>(entity).unwrap().0.len(), 1);

        // Zero duration: finishes immediately.
        let entity = world
            .spawn((transition(0.0, 0.0, 0.0), BackgroundColor(from)))
            .id();
        world.run_system_once(super::advance_transitions).unwrap();
        assert_eq!(world.get::<BackgroundColor>(entity).unwrap().0, to);
        assert_eq!(world.get::<Transitions>(entity).unwrap().0, Vec::new());
    }
}

#[cfg(test)]
mod ua_default_tests {
    use super::*;
    use lightningcss::traits::IntoOwned;

    /// ADR 0028: a `th`'s text is bold unless a rule declares the weight
    /// (the UA sheet). The visual check needs a registered family (a
    /// handle font ignores the weight — bevy uses the file as-is); the
    /// computed style is the contract here.
    #[test]
    fn th_text_is_bold_by_default() {
        let sheet = lightningcss::stylesheet::StyleSheet::parse(
            "th {}",
            lightningcss::stylesheet::ParserOptions::default(),
        )
        .expect("css")
        .into_owned();
        let styles = HtmlStyles::from_sheet(&sheet);
        let fonts = FontFamilies::default();
        let images = Assets::<Image>::default();
        let styler = Styler {
            styles: &styles,
            fonts: &fonts,
            root_size: 16.0,
            sheet: None,
            images: &images,
            server: None,
            template: None,
        };
        let defaults = Style {
            color: DEFAULT_COLOR,
            family: None,
            size: DEFAULT_FONT_SIZE,
            bold: false,
            italic: false,
            pointer_events: true,
            opacity: 1.0,
            align: TextAlignSpec::Left,
            line_height: LineHeightSpec::Normal,
            letter_spacing: 0.0,
            list_style: None,
            text_transform: TextTransformSpec::None,
            white_space: None,
        };
        let state = ElementState::default();
        let style = styler.style_of(
            &element_tag("th"),
            &defaults,
            Pseudo::default(),
            None,
            &[],
            &state,
        );
        assert!(style.bold, "a th's text is bold by default");
        // An explicit weight wins.
        let sheet = lightningcss::stylesheet::StyleSheet::parse(
            "th { font-weight: normal }",
            lightningcss::stylesheet::ParserOptions::default(),
        )
        .expect("css")
        .into_owned();
        let styles = HtmlStyles::from_sheet(&sheet);
        let styler = Styler {
            styles: &styles,
            ..styler
        };
        let style = styler.style_of(
            &element_tag("th"),
            &defaults,
            Pseudo::default(),
            None,
            &[],
            &state,
        );
        assert!(!style.bold, "a declared weight wins");
    }
}

#[cfg(test)]
mod transform_tests {
    use super::*;

    #[test]
    fn text_transform_cases() {
        let t = |text, transform| transform_text(text, transform, None);
        assert_eq!(t("mixed Case", TextTransformSpec::None), "mixed Case");
        assert_eq!(t("mixed Case", TextTransformSpec::Uppercase), "MIXED CASE");
        assert_eq!(t("MIXED Case", TextTransformSpec::Lowercase), "mixed case");
        // Full mappings: ß uppercases to SS; Σ lowercases to a final ς.
        assert_eq!(t("ß", TextTransformSpec::Uppercase), "SS");
        assert_eq!(t("ΟΔΟΣ.", TextTransformSpec::Lowercase), "οδος.");
        // Capitalize, as Chrome renders it (the browser oracle checks the
        // full set in tests/vectors/text_transform_unicode).
        for (text, chrome) in [
            ("hello world", "Hello World"),
            ("don't stop-me_now (yes)", "Don't Stop-Me_now (Yes)"),
            ("\"quoted\"", "\"Quoted\""),
            ("v2next 2nd", "V2next 2nd"),
            ("a.b.c o:clock e·g 'tis", "A.B.C O:Clock E·g 'Tis"),
            ("ǆemal ßtraße ﬁne", "ǅemal ßtraße ﬁne"),
            ("e\u{301}cole 漢字abc", "E\u{301}cole 漢字Abc"),
            ("a  b", "A  B"),
        ] {
            assert_eq!(t(text, TextTransformSpec::Capitalize), chrome, "{text}");
        }
        // A word continues across elements: `<b>foo</b>bar`.
        assert_eq!(
            transform_text("bar baz", TextTransformSpec::Capitalize, Some('o')),
            "bar Baz"
        );
        assert_eq!(
            transform_text("bar", TextTransformSpec::Capitalize, Some(' ')),
            "Bar"
        );
    }
}
