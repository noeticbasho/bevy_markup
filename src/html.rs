//! The `HtmlUi` component and what it produces.
//!
//! Spawn [`HtmlUi`] with a template handle; its required components
//! ([`Node`], [`TemplateContext`], [`RenderedHtml`], …) are added for you, so
//! override any of them in the same bundle. The entity's children are owned by
//! the pipeline. When the template, context or locale change, the new
//! document is reconciled with them ([`HtmlUiBuilt`]): an element still there
//! — matched by `id`, else by position — is updated in place, keeping its
//! entity and what the app attached; the rest are despawned and spawned.
//! Stylesheet and font changes restyle them in place ([`HtmlUiRestyled`]).
//!
//! Tags decide structure:
//! - blocks: `h1`–`h6`, `p`, `li` (bulleted), `pre` (whitespace and line breaks
//!   kept, no wrapping), and loose text directly inside a container
//! - containers: `div`, `section`, `article`, `header`, `footer`, `main`,
//!   `nav`, `aside`, `ul`, `ol`, `blockquote`, `figure`, `form` — column nodes
//!   holding their children's nodes, spaced like the `HtmlUi` node's own
//!   `row_gap` unless CSS sets `gap`
//! - any other element is inline (styled text inside a block) or, outside a
//!   block, walked through without a node (`html`, `body`, unknown tags);
//!   `head`/`script`/`style` are skipped
//!
//! Each block is spawned as a `Text` with one `TextSpan` per styled run. Blocks
//! and containers carry an [`HtmlElement`] (anonymous loose text doesn't), so
//! CSS selectors and [`HtmlElements`] reach them.

use std::borrow::Cow;

use bevy::asset::LoadState;
use bevy::ecs::system::SystemParam;
use bevy::platform::collections::HashSet;
use bevy::prelude::*;
use serde::Serialize;

use crate::l10n::LocalizedText;
use crate::template::{HtmlDocument, HtmlTemplate, error_chain};

/// A Bevy UI subtree rendered from an HTML template.
///
/// ```no_run
/// # use bevy::prelude::*;
/// # use bevy_markup::prelude::*;
/// # fn system(mut commands: Commands, asset_server: Res<AssetServer>) {
/// commands.spawn((
///     HtmlUi::new(asset_server.load("ui/inventory.html")),
///     TemplateContext::new().with("gold", &120),
/// ));
/// # }
/// ```
#[derive(Component, Clone, Debug, Reflect)]
#[reflect(Component)]
#[require(
    Node,
    TemplateContext,
    RenderedHtml,
    LocalizedText,
    crate::rebuild::RebuildState
)]
pub struct HtmlUi(pub Handle<HtmlTemplate>);

impl HtmlUi {
    pub fn new(template: Handle<HtmlTemplate>) -> Self {
        Self(template)
    }
}

impl From<Handle<HtmlTemplate>> for HtmlUi {
    fn from(template: Handle<HtmlTemplate>) -> Self {
        Self(template)
    }
}

/// Tera variables for this entity's template. Mutate it (it derefs to
/// [`tera::Context`]) to re-render. A render whose HTML equals the previous
/// one changes nothing (no rebuild, attached components kept), so there's no
/// need to diff values before writing them.
#[derive(Component, Default, Clone, Deref, DerefMut)]
pub struct TemplateContext(pub tera::Context);

impl TemplateContext {
    pub fn new() -> Self {
        Self::default()
    }

    /// Builder form of [`tera::Context::insert`].
    pub fn with<T: Serialize + ?Sized>(
        mut self,
        key: impl Into<Cow<'static, str>>,
        value: &T,
    ) -> Self {
        self.0.insert(key, value);
        self
    }
}

impl From<tera::Context> for TemplateContext {
    fn from(context: tera::Context) -> Self {
        Self(context)
    }
}

/// The template's latest render. Read-only for users; written by
/// [`HtmlUiSystems::Render`](crate::HtmlUiSystems::Render).
#[derive(Component, Default)]
pub enum RenderedHtml {
    /// Template not loaded yet.
    #[default]
    Pending,
    Ready(HtmlDocument),
    /// Rendering or parsing failed; the message includes the error chain. The
    /// UI shows it as a paragraph.
    Failed(String),
}

/// Show the DOM outline (elements, attributes, text, translations) instead of
/// the rendered UI — for debugging templates. Styled like the stylesheet's
/// `pre`. Also logged at `debug` level on every rebuild.
#[derive(Component, Default, Clone, Copy, Debug, Reflect)]
#[reflect(Component, Default)]
pub struct HtmlDebugOutline;

/// On an `HtmlUi` root (ADR 0031): build only the part of the document in
/// the scroll window — materialized blocks between two spacers, the
/// culled subtrees replaced by their last measured height.
///
/// The window is a band `scroll_offset − margin … scroll_offset + viewport
/// + margin` in the root's content coordinates (logical pixels; the
/// viewport is the root's padding box). It is rebuilt when the scroll
/// position moves far enough to change which blocks it holds, when the
/// viewport's size changes, or when a measured height does; the blocks
/// that stay in it keep their entities and reuse their specs. The margin
/// defaults to one viewport in each direction.
///
/// With this component, apps must not assume every DOM element has an
/// entity: only materialized elements exist. [`HtmlElements`] and the
/// signal/focus machinery see materialized elements only (focus
/// navigation stays inside the window; scrolling an element out of it
/// despawns it and the focus repair refocuses). Translated subtrees and
/// containers laid out as rows, grids, reversed or wrapping are never
/// culled inside; a root that isn't a column (the `HtmlUi` default is a
/// row: give `html` `flex-direction: column`) windows nothing.
///
/// Content changes cost more than scrolling. An element whose content
/// changed, or a new one, builds for one frame to be measured; the rows
/// after a new element are placed by its siblings' average height until
/// then. A change that grows or shrinks the rows above the viewport by
/// more than the margin in one step (or an added element much taller or
/// shorter than its siblings) leaves part of the viewport unbuilt for
/// that frame, as does the first frame after the root's width changes;
/// the next frame corrects it.
#[derive(Component, Clone, Copy, Debug, Reflect)]
#[reflect(Component, Default)]
pub struct HtmlWindowed {
    /// Extra materialized height on each side of the viewport, in
    /// viewports (the band extends this far past both edges). 0 means
    /// exactly the viewport.
    pub margin_viewports: f32,
}

impl Default for HtmlWindowed {
    fn default() -> Self {
        Self {
            margin_viewports: 1.0,
        }
    }
}

/// The HTML element a spawned block node came from.
#[derive(Component, Clone, Debug, Default, PartialEq, Reflect)]
#[reflect(Component, Default)]
pub struct HtmlElement {
    /// Lowercase tag name, e.g. `"p"`.
    pub tag: String,
    /// The `id` attribute.
    pub id: Option<String>,
    /// The `class` attribute, split on whitespace.
    pub classes: Vec<String>,
    /// The `data-*` attributes without the `data-` prefix (`data-tooltip-key`
    /// → `tooltip-key`), entities decoded, like a browser's
    /// `element.dataset` (keys stay kebab-case); a value-less attribute maps
    /// to `""`. Hooks' own attributes (`data-on-click`, `data-with`,
    /// `data-l10n-id`, …) are included.
    pub dataset: std::collections::BTreeMap<String, String>,
}

impl HtmlElement {
    pub fn has_class(&self, class: &str) -> bool {
        self.classes.iter().any(|c| c == class)
    }

    /// The `data-<key>` attribute's value.
    pub fn data(&self, key: &str) -> Option<&str> {
        self.dataset.get(key).map(String::as_str)
    }
}

/// Fired on an [`HtmlUi`] entity after its children were built or updated
/// for new content (template, context, locale or outline), or after a
/// restyle that had to spawn elements.
///
/// Elements still in the document keep their entities — and whatever was
/// attached to them — so this fires again for elements already wired:
/// anything done here must be idempotent. Inserting a component is;
/// adding an observer is not (each update would add one more, and one click
/// would run them all). Declare behaviour in the template instead:
/// `data-on-click` and friends for interactions ([`signals`](crate::signals)),
/// `is="…"` for components or observers attached once per element
/// ([custom elements](crate::custom_elements)).
///
/// ```no_run
/// # use bevy::prelude::*;
/// # use bevy_markup::prelude::*;
/// # #[derive(Component)] struct Highlighted;
/// // Idempotent: re-inserting the marker on a kept element changes nothing.
/// fn mark_current(built: On<HtmlUiBuilt>, elements: HtmlElements, mut commands: Commands) {
///     for current in elements.by_class(built.entity, "current") {
///         commands.entity(current).insert(Highlighted);
///     }
/// }
/// # App::new().add_observer(mark_current);
/// ```
#[derive(EntityEvent, Clone, Copy, Debug)]
pub struct HtmlUiBuilt {
    pub entity: Entity,
}

/// Fired on an [`HtmlUi`] entity after a style-only change (stylesheet swap
/// or reload, a frame image loading, `FontFamilies`) restyled its existing
/// children in place. Every entity — and whatever the app attached to it —
/// was kept, so there's nothing to re-wire. When the new styles need new
/// nodes (e.g. box properties appearing on a block, which then needs a
/// wrapper node), [`HtmlUiBuilt`] fires instead.
#[derive(EntityEvent, Clone, Copy, Debug)]
pub struct HtmlUiRestyled {
    pub entity: Entity,
}

/// Finds spawned [`HtmlElement`]s below an [`HtmlUi`] entity.
#[derive(SystemParam)]
pub struct HtmlElements<'w, 's> {
    children: Query<'w, 's, &'static Children>,
    elements: Query<'w, 's, &'static HtmlElement>,
}

impl HtmlElements<'_, '_> {
    /// All element nodes below `root`, in document order.
    pub fn iter(&self, root: Entity) -> impl Iterator<Item = (Entity, &HtmlElement)> + '_ {
        self.children
            .iter_descendants_depth_first(root)
            .filter_map(|entity| Some((entity, self.elements.get(entity).ok()?)))
    }

    /// The first element below `root` with this `id`.
    pub fn by_id(&self, root: Entity, id: &str) -> Option<Entity> {
        self.iter(root)
            .find(|(_, element)| element.id.as_deref() == Some(id))
            .map(|(entity, _)| entity)
    }

    /// Elements below `root` with this class.
    pub fn by_class<'a>(
        &'a self,
        root: Entity,
        class: &'a str,
    ) -> impl Iterator<Item = Entity> + 'a {
        self.iter(root)
            .filter(move |(_, element)| element.has_class(class))
            .map(|(entity, _)| entity)
    }

    /// Elements below `root` with this tag.
    pub fn by_tag<'a>(&'a self, root: Entity, tag: &'a str) -> impl Iterator<Item = Entity> + 'a {
        self.iter(root)
            .filter(move |(_, element)| element.tag == tag)
            .map(|(entity, _)| entity)
    }
}

#[hotpath::measure]
pub(crate) fn render_templates(
    mut events: MessageReader<AssetEvent<HtmlTemplate>>,
    templates: Res<Assets<HtmlTemplate>>,
    server: Res<AssetServer>,
    mut views: Query<(Ref<HtmlUi>, Ref<TemplateContext>, &mut RenderedHtml)>,
) {
    let reloaded: HashSet<AssetId<HtmlTemplate>> = events
        .read()
        .filter_map(|event| match event {
            AssetEvent::LoadedWithDependencies { id } | AssetEvent::Modified { id } => Some(*id),
            _ => None,
        })
        .collect();

    for (html, context, mut rendered) in &mut views {
        if !html.is_changed() && !context.is_changed() && !reloaded.contains(&html.0.id()) {
            continue;
        }
        let Some(template) = templates.get(&html.0) else {
            // A template whose load failed (a Tera syntax error): record
            // it as a failed render — the error paragraph builds and the
            // UI converges, like a failed render's — instead of staying
            // `Pending` forever (a blank UI that never settles).
            if !matches!(server.load_state(html.0.id()), LoadState::Failed(_)) {
                continue;
            }
            let message = "template failed to load (syntax error?)".to_owned();
            if !matches!(&*rendered, RenderedHtml::Failed(previous) if *previous == message) {
                *rendered = RenderedHtml::Failed(message);
            }
            continue;
        };
        // An identical render changes nothing: leave `RenderedHtml`
        // untouched (no change tick, so no rebuild). Apps can then write
        // their `TemplateContext` freely, e.g. every frame.
        let source = match template.render_source(&context) {
            Ok(source) => source,
            Err(err) => {
                let message = error_chain(&err);
                if !matches!(&*rendered, RenderedHtml::Failed(previous) if *previous == message) {
                    error!("rendering {}: {message}", template.name());
                    *rendered = RenderedHtml::Failed(message);
                }
                continue;
            }
        };
        // A `.md` template converted at load (ADR 0020): the Tera output is
        // the HTML the pipeline parses, so the identical-render check still
        // rebuilds only when the rendered HTML actually changes.
        if matches!(&*rendered, RenderedHtml::Ready(document) if document.source() == source) {
            continue;
        }
        *rendered = match HtmlDocument::parse(source) {
            Ok(document) => RenderedHtml::Ready(document),
            Err(err) => {
                let message = error_chain(&err);
                error!("parsing {}: {message}", template.name());
                RenderedHtml::Failed(message)
            }
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    /// `TemplateContext::from(tera::Context)` keeps the context's variables
    /// (an app building a `tera::Context` itself must not lose them).
    #[test]
    fn template_context_from_tera_keeps_variables() {
        let mut context = tera::Context::new();
        context.insert("player", "Ada");
        let converted = TemplateContext::from(context);
        assert_eq!(
            converted.get("player").and_then(|value| value.as_str()),
            Some("Ada")
        );
    }

    fn element(tag: &str, id: Option<&str>, classes: &[&str]) -> HtmlElement {
        HtmlElement {
            tag: tag.to_owned(),
            id: id.map(str::to_owned),
            classes: classes.iter().map(|c| (*c).to_owned()).collect(),
            ..Default::default()
        }
    }

    /// `iter`/`by_id`/`by_class`/`by_tag` walk the built tree in document
    /// (depth-first pre-) order, skipping anonymous nodes. Bevy's plain
    /// `iter_descendants` is breadth-first: with it, `by_id` returned a
    /// later, shallower duplicate id instead of the first one in the
    /// document, and `by_class` listed siblings before nested elements.
    #[test]
    fn lookups_follow_document_order() {
        let mut world = World::new();
        // <div.a><p#x.a>…</p></div> <anonymous text> <p#x.b> <section><p.a></section>
        let root = world.spawn_empty().id();
        let div = world
            .spawn((element("div", None, &["a"]), ChildOf(root)))
            .id();
        let nested = world
            .spawn((element("p", Some("x"), &["a"]), ChildOf(div)))
            .id();
        let anonymous = world.spawn(ChildOf(root)).id();
        let shallow = world
            .spawn((element("p", Some("x"), &["b"]), ChildOf(root)))
            .id();
        let section = world
            .spawn((element("section", None, &[]), ChildOf(root)))
            .id();
        let deep = world
            .spawn((element("p", None, &["a"]), ChildOf(section)))
            .id();
        let elsewhere = world.spawn(element("p", Some("x"), &["a"])).id();

        let found = world
            .run_system_once(move |elements: HtmlElements| {
                (
                    elements
                        .iter(root)
                        .map(|(entity, _)| entity)
                        .collect::<Vec<_>>(),
                    elements.by_id(root, "x"),
                    elements.by_class(root, "a").collect::<Vec<_>>(),
                    elements.by_tag(root, "p").collect::<Vec<_>>(),
                    elements.by_id(root, "missing"),
                )
            })
            .unwrap();
        assert_eq!(found.0, [div, nested, shallow, section, deep]);
        assert_eq!(found.1, Some(nested));
        assert_eq!(found.2, [div, nested, deep]);
        assert_eq!(found.3, [nested, shallow, deep]);
        assert_eq!(found.4, None);
        assert!(!found.0.contains(&anonymous) && !found.0.contains(&elsewhere));
    }
}
