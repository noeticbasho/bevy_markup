//! Font families for CSS `font-family`.
//!
//! Register each family's faces in the [`FontFamilies`] resource; a CSS
//! `font-family` list resolves to every registered family in order (not
//! just the first hit), and each family's face is picked by
//! `font-weight`/`font-style`. The chain composes into Bevy 0.20's
//! fallback list: fontique picks the first face that covers a glyph, so
//! mixed scripts (Latin + CJK) render from different fonts without
//! bundling. Changing the resource rebuilds every `HtmlUi`.
//!
//! A face is any [`FontSource`]: a loaded font file, or a family resolved
//! from the fonts installed on the system (a name like `"DejaVu Serif"` or a
//! generic like [`FontSource::serif`]; needs Bevy's `system_font_discovery`,
//! which the `system_fonts` feature enables). For system families, bold and
//! italic are requested from the system, which picks the family's real faces.
//!
//! ```no_run
//! # use bevy::prelude::*;
//! # use bevy_markup::prelude::*;
//! fn register(asset_server: Res<AssetServer>, mut fonts: ResMut<FontFamilies>) {
//!     fonts
//!         // Font files: one per face.
//!         .insert(
//!             "Iosevka Slab Mono",
//!             FontFaces::new(asset_server.load("fonts/IosevkaSlabMono-Regular.ttf"))
//!                 .with_bold(asset_server.load("fonts/IosevkaSlabMono-Bold.ttf")),
//!         )
//!         // The system's serif family, all faces.
//!         .insert("Body", FontFaces::new(FontSource::serif()))
//!         .set_generic(GenericFamily::Monospace, "Iosevka Slab Mono");
//! }
//! ```

use bevy::platform::collections::HashMap;
use bevy::prelude::*;

/// CSS generic family keywords that can be mapped to a registered family.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Reflect)]
pub enum GenericFamily {
    Serif,
    SansSerif,
    Monospace,
    Cursive,
    Fantasy,
    SystemUi,
}

/// The faces of one family. Missing faces fall back as CSS font matching
/// does (style narrows before weight): bold-italic → italic → bold → regular.
/// A system family (any [`FontSource`] other than a handle) usually needs
/// only `regular`: bold and italic are then requested from the system.
#[derive(Clone, Debug)]
pub struct FontFaces {
    pub regular: FontSource,
    pub bold: Option<FontSource>,
    pub italic: Option<FontSource>,
    pub bold_italic: Option<FontSource>,
}

impl FontFaces {
    pub fn new(regular: impl Into<FontSource>) -> Self {
        Self {
            regular: regular.into(),
            bold: None,
            italic: None,
            bold_italic: None,
        }
    }

    pub fn with_bold(mut self, bold: impl Into<FontSource>) -> Self {
        self.bold = Some(bold.into());
        self
    }

    pub fn with_italic(mut self, italic: impl Into<FontSource>) -> Self {
        self.italic = Some(italic.into());
        self
    }

    pub fn with_bold_italic(mut self, bold_italic: impl Into<FontSource>) -> Self {
        self.bold_italic = Some(bold_italic.into());
        self
    }

    /// The face for this weight/style, with fallbacks.
    pub fn face(&self, bold: bool, italic: bool) -> FontSource {
        let pick = match (bold, italic) {
            // CSS font matching: an italic face (bold synthesized) beats an
            // upright bold one.
            (true, true) => self
                .bold_italic
                .as_ref()
                .or(self.italic.as_ref())
                .or(self.bold.as_ref()),
            (true, false) => self.bold.as_ref(),
            (false, true) => self.italic.as_ref(),
            (false, false) => None,
        };
        pick.unwrap_or(&self.regular).clone()
    }
}

/// Registered font families, by CSS name (matched case-insensitively).
#[derive(Resource, Default, Debug)]
pub struct FontFamilies {
    families: Vec<(String, FontFaces)>,
    generics: HashMap<GenericFamily, String>,
}

impl FontFamilies {
    /// Registers (or replaces) the family `name`.
    pub fn insert(&mut self, name: impl Into<String>, faces: FontFaces) -> &mut Self {
        let name = name.into();
        match self.index_of(&name) {
            Some(index) => self.families[index].1 = faces,
            None => self.families.push((name, faces)),
        }
        self
    }

    /// Makes the CSS generic keyword (`serif`, `monospace`, …) mean the
    /// registered family `name`.
    pub fn set_generic(&mut self, generic: GenericFamily, name: impl Into<String>) -> &mut Self {
        self.generics.insert(generic, name.into());
        self
    }

    pub fn get(&self, name: &str) -> Option<&FontFaces> {
        self.index_of(name).map(|index| &self.families[index].1)
    }

    /// Every registered family, in registration order.
    #[cfg(feature = "svg-text")]
    pub(crate) fn iter(&self) -> impl Iterator<Item = (&str, &FontFaces)> {
        self.families
            .iter()
            .map(|(name, faces)| (name.as_str(), faces))
    }

    /// The family a generic keyword maps to, if any.
    #[cfg(feature = "svg-text")]
    pub(crate) fn generic(&self, generic: GenericFamily) -> Option<&str> {
        self.generics.get(&generic).map(String::as_str)
    }

    fn index_of(&self, name: &str) -> Option<usize> {
        self.families
            .iter()
            .position(|(family, _)| family.eq_ignore_ascii_case(name))
    }

    pub(crate) fn faces(&self, index: usize) -> Option<&FontFaces> {
        self.families.get(index).map(|(_, faces)| faces)
    }

    /// The CSS `font-family` list resolved to registered family indexes, in
    /// CSS order (generics through their mapped names, unregistered names
    /// skipped): the text font composes the chain into Bevy 0.20's
    /// per-glyph fallback list (ADR 0015).
    pub(crate) fn resolve_chain(&self, list: &[FamilyRef]) -> Vec<usize> {
        list.iter()
            .filter_map(|family| match family {
                FamilyRef::Named(name) => self.index_of(name),
                FamilyRef::Generic(generic) => self.index_of(self.generics.get(generic)?),
            })
            .collect()
    }
}

/// One entry of a CSS `font-family` list.
#[derive(Clone, Debug)]
pub(crate) enum FamilyRef {
    Named(String),
    Generic(GenericFamily),
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::asset::uuid::Uuid;

    fn font(id: u128) -> Handle<Font> {
        Handle::Uuid(Uuid::from_u128(id), std::marker::PhantomData)
    }

    fn named(name: &str) -> FamilyRef {
        FamilyRef::Named(name.to_owned())
    }

    /// Every combination of present faces × requested weight/style follows
    /// the documented fallback (bold-italic → italic → bold → regular; bold
    /// or italic alone → regular), and a present exact face always wins.
    /// Catches a wrong arm (e.g. italic requests getting the bold face).
    #[test]
    fn face_fallback_chain() {
        let (regular, bold, italic, bold_italic) = (font(1), font(2), font(3), font(4));
        for mask in 0..8u8 {
            let mut faces = FontFaces::new(regular.clone());
            let has = |bit: u8| mask & bit != 0;
            if has(1) {
                faces = faces.with_bold(bold.clone());
            }
            if has(2) {
                faces = faces.with_italic(italic.clone());
            }
            if has(4) {
                faces = faces.with_bold_italic(bold_italic.clone());
            }
            let pick = |chain: &[(u8, &Handle<Font>)]| {
                chain
                    .iter()
                    .find(|(bit, _)| has(*bit))
                    .map_or(regular.clone(), |(_, face)| (*face).clone())
            };
            let expected = [
                ((false, false), regular.clone()),
                ((true, false), pick(&[(1, &bold)])),
                ((false, true), pick(&[(2, &italic)])),
                (
                    (true, true),
                    pick(&[(4, &bold_italic), (2, &italic), (1, &bold)]),
                ),
            ];
            for ((b, i), face) in expected {
                assert_eq!(
                    faces.face(b, i),
                    face.into(),
                    "faces mask {mask:03b}, bold {b}, italic {i}"
                );
            }
        }
    }

    fn families() -> FontFamilies {
        let mut families = FontFamilies::default();
        families
            .insert("Spectral", FontFaces::new(font(1)))
            .insert("Iosevka Slab", FontFaces::new(font(2)))
            .set_generic(GenericFamily::Monospace, "iosevka slab")
            // Mapped to a name that was never registered.
            .set_generic(GenericFamily::Serif, "Missing");
        families
    }

    /// The *list* order decides, not registration order; names match ASCII
    /// case-insensitively (CSS family names are case-insensitive).
    #[test]
    fn registered_names_resolve_in_list_order() {
        let families = families();
        let spectral = families.resolve_chain(&[named("spectral")]);
        let iosevka = families.resolve_chain(&[named("IOSEVKA SLAB")]);
        assert!(!spectral.is_empty() && !iosevka.is_empty() && spectral != iosevka);
        // Case-insensitive; the first hit no longer shadows later names —
        // the whole chain resolves (ADR 0015's per-glyph fallback).
        assert_eq!(
            families.resolve_chain(&[named("Iosevka Slab"), named("Spectral")]),
            [iosevka[0], spectral[0]]
        );
        assert_eq!(
            families.resolve_chain(&[named("Nope"), named("Spectral")]),
            spectral
        );
        assert_eq!(families.faces(spectral[0]).unwrap().regular, font(1).into());
    }

    /// Unregistered names, unmapped generics and generics mapped to
    /// unregistered names are skipped, and a list with nothing registered
    /// resolves to `None` (Bevy's default font), not the first entry.
    #[test]
    fn unresolvable_entries_are_skipped() {
        let families = families();
        let iosevka = families.resolve_chain(&[named("Iosevka Slab")]);
        let serif = FamilyRef::Generic(GenericFamily::Serif);
        let cursive = FamilyRef::Generic(GenericFamily::Cursive);
        let mono = FamilyRef::Generic(GenericFamily::Monospace);
        assert_eq!(
            families.resolve_chain(&[serif.clone(), cursive.clone(), mono]),
            iosevka
        );
        assert!(
            families
                .resolve_chain(&[named("Nope"), serif, cursive])
                .is_empty()
        );
        assert!(families.resolve_chain(&[]).is_empty());
    }

    /// The chain keeps every registered family in CSS order — the fallback
    /// list fontique resolves per glyph (ADR 0015) — with duplicates
    /// deduplicated by the registry's in-place replace.
    #[test]
    fn chains_keep_all_registered_families() {
        let families = families();
        let chain = families.resolve_chain(&[
            named("Spectral"),
            named("Nope"),
            FamilyRef::Generic(GenericFamily::Monospace),
        ]);
        assert_eq!(chain.len(), 2, "{chain:?}");
    }

    /// Re-registering a name (in any case) replaces its faces in place:
    /// no duplicate entry shadows it, and other families keep resolving.
    #[test]
    fn insert_replaces_case_insensitively() {
        let mut families = families();
        let before = families.resolve_chain(&[named("Spectral")]);
        families.insert("SPECTRAL", FontFaces::new(font(9)));
        assert_eq!(families.resolve_chain(&[named("Spectral")]), before);
        assert_eq!(families.get("spectral").unwrap().regular, font(9).into());
        assert_eq!(
            families.get("Iosevka Slab").unwrap().regular,
            font(2).into()
        );
    }
}
