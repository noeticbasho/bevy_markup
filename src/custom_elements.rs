//! Customized built-in elements: app behaviour declared in the template.
//!
//! An element names a definition with the `is` attribute, as in a browser's
//! customized built-ins, and passes it data in `data-*` attributes:
//!
//! ```html
//! <div class="tip-icon" is="input-icon" data-icon="{{ icon }}"></div>
//! ```
//!
//! The app defines the name with a system that takes
//! [`In<ElementConnected>`](ElementConnected), like a browser's
//! `connectedCallback`:
//!
//! ```no_run
//! # use bevy::prelude::*;
//! # use bevy_markup::prelude::*;
//! # #[derive(Resource)] struct Icons;
//! # impl Icons { fn image(&self, _: &str) -> Option<ImageNode> { None } }
//! fn input_icon(connected: In<ElementConnected>, icons: Res<Icons>, mut commands: Commands) {
//!     if let Some(image) = connected.data("icon").and_then(|icon| icons.image(icon)) {
//!         commands.entity(connected.entity).insert(image);
//!     }
//! }
//! # let mut app = App::new();
//! app.define_html_element("input-icon", input_icon);
//! ```
//!
//! The system runs whenever the element is spawned: on every (re)build of its
//! UI, in document order, before [`HtmlUiBuilt`](crate::html::HtmlUiBuilt)
//! fires. A restyle keeps the element's entity, and with it whatever the
//! system attached, so it doesn't run again. Only blocks and containers
//! become entities; `is` on other elements is ignored, and an undefined name
//! is logged once and otherwise ignored.

use bevy::ecs::system::SystemId;
use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;

use crate::template::decode_attribute;

/// The input of an element definition's system (see the [module
/// docs](self)): the spawned element and its `data-*` attributes.
#[derive(Clone, Debug, PartialEq)]
pub struct ElementConnected {
    /// The element's entity (it carries its [`HtmlElement`](crate::html::HtmlElement)).
    pub entity: Entity,
    /// The [`HtmlUi`](crate::html::HtmlUi) entity the element was built under.
    pub root: Entity,
    /// The `is` value.
    pub name: String,
    /// The element's `data-*` attributes without the `data-` prefix
    /// (`data-icon-size` → `icon-size`), entities decoded; a value-less
    /// attribute maps to `""`.
    pub dataset: HashMap<String, String>,
}

impl ElementConnected {
    /// The `data-<key>` attribute's value.
    pub fn data(&self, key: &str) -> Option<&str> {
        self.dataset.get(key).map(String::as_str)
    }
}

/// An element's `is` name and dataset, parsed while collecting the DOM.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CustomElement {
    pub(crate) name: String,
    pub(crate) dataset: HashMap<String, String>,
}

/// What a spawned custom element was connected as: an in-place update
/// keeps the entity (and what its definition attached) only while this is
/// unchanged; a different `is` or dataset spawns a new element.
#[derive(Component)]
pub(crate) struct ConnectedAs(pub(crate) CustomElement);

/// `tag`'s `is` attribute and dataset, if it names a definition.
pub(crate) fn custom_element(tag: &tl::HTMLTag) -> Option<CustomElement> {
    let attributes = tag.attributes();
    let name = decode_attribute(attributes.get("is").flatten()?.as_utf8_str());
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let dataset = dataset(tag).into_iter().collect();
    Some(CustomElement {
        name: name.to_owned(),
        dataset,
    })
}

/// `tag`'s `data-*` attributes, prefix stripped, keys lowercased, values
/// entity-decoded (`""` for a value-less attribute).
pub(crate) fn dataset(tag: &tl::HTMLTag) -> std::collections::BTreeMap<String, String> {
    tag.attributes()
        .iter()
        .filter_map(|(key, value)| {
            let key = key.strip_prefix("data-")?;
            let value =
                value.map_or_else(String::new, |value| decode_attribute(value).into_owned());
            Some((key.to_ascii_lowercase(), value))
        })
        .collect()
}

/// How many distinct undefined `is=` names are warned about (and
/// remembered) before further ones are ignored without a warning.
const MAX_WARNED_NAMES: usize = 256;

/// The defined element names and their systems.
#[derive(Resource, Default)]
pub(crate) struct CustomElements {
    systems: HashMap<String, SystemId<In<ElementConnected>>>,
    /// Undefined names already logged, up to `MAX_WARNED_NAMES`.
    warned: HashSet<String>,
}

/// Runs `connected.name`'s definition on `connected.entity` (queued by the
/// build, after the spawn commands, before `HtmlUiBuilt`).
pub(crate) fn connect(world: &mut World, connected: ElementConnected) {
    let Some(id) = world
        .get_resource::<CustomElements>()
        .and_then(|elements| elements.systems.get(&connected.name).copied())
    else {
        let mut elements = world.get_resource_or_init::<CustomElements>();
        // Bounded: content can name any number of undefined elements, and
        // the set grew by one entry per name for the app's lifetime
        // (review_0003 G22).
        if elements.warned.len() < MAX_WARNED_NAMES {
            if elements.warned.insert(connected.name.clone()) {
                warn!(
                    "html custom elements: is=\"{}\" isn't defined (define_html_element); ignored",
                    connected.name
                );
                if elements.warned.len() == MAX_WARNED_NAMES {
                    warn!(
                        "html custom elements: {MAX_WARNED_NAMES} undefined names reported; \
                         further ones are ignored silently"
                    );
                }
            }
        }
        return;
    };
    if world.get_entity(connected.entity).is_err() {
        return;
    }
    let name = connected.name.clone();
    if let Err(error) = world.run_system_with(id, connected) {
        warn!("html custom elements: is=\"{name}\": {error}");
    }
}

/// Defines custom element names on an [`App`] (see the [module docs](self)).
pub trait HtmlCustomElementsExt {
    /// Runs `system` on every spawned element with `is="<name>"`. Defining a
    /// name again replaces its system.
    fn define_html_element<M>(
        &mut self,
        name: impl Into<String>,
        system: impl IntoSystem<In<ElementConnected>, (), M> + 'static,
    ) -> &mut Self;
}

impl HtmlCustomElementsExt for App {
    fn define_html_element<M>(
        &mut self,
        name: impl Into<String>,
        system: impl IntoSystem<In<ElementConnected>, (), M> + 'static,
    ) -> &mut Self {
        let world = self.world_mut();
        let id = world.register_system(system);
        let previous = world
            .get_resource_or_init::<CustomElements>()
            .systems
            .insert(name.into(), id);
        if let Some(previous) = previous {
            let _ = world.unregister_system(previous);
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(html: &str) -> Option<CustomElement> {
        let dom = tl::parse(html, tl::ParserOptions::default()).unwrap();
        let node = dom.children()[0].get(dom.parser()).unwrap();
        custom_element(node.as_tag().unwrap())
    }

    #[test]
    fn is_and_dataset_are_parsed() {
        let custom = parse(
            r#"<div is=" icon " data-icon="key_e" data-Size="2" data-flag class="x" data-note="a &amp; b"></div>"#,
        )
        .unwrap();
        assert_eq!(custom.name, "icon");
        let mut dataset: Vec<_> = custom.dataset.into_iter().collect();
        dataset.sort();
        assert_eq!(
            dataset,
            [
                ("flag".into(), String::new()),
                ("icon".into(), "key_e".into()),
                ("note".into(), "a & b".into()),
                ("size".into(), "2".into()),
            ]
        );
    }

    #[test]
    fn no_or_blank_is_is_not_custom() {
        assert!(parse(r#"<div data-icon="x"></div>"#).is_none());
        assert!(parse(r#"<div is="  "></div>"#).is_none());
        assert!(parse(r#"<div is></div>"#).is_none());
    }

    /// Undefined names are remembered (to warn once each) only up to a
    /// bound: content naming thousands of distinct undefined elements no
    /// longer grows the set for the app's lifetime (review_0003 G22).
    #[test]
    fn undefined_name_warnings_are_bounded() {
        let mut world = World::new();
        let entity = world.spawn_empty().id();
        for index in 0..1000 {
            connect(
                &mut world,
                ElementConnected {
                    entity,
                    root: entity,
                    name: format!("undefined-{index}"),
                    dataset: HashMap::default(),
                },
            );
        }
        assert_eq!(
            world.resource::<CustomElements>().warned.len(),
            MAX_WARNED_NAMES
        );
    }
}
