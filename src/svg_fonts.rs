//! Fonts for SVG `<text>` (the `svg-text` feature, ADR 0038 §6).
//!
//! usvg turns `<text>` into outlines while it parses, so the fonts must be
//! known then. [`SvgFontSet`] is a snapshot of the fonts the app registered
//! in [`FontFamilies`]: every face given as a font file (`FontSource::Handle`)
//! is loaded into a `fontdb` database, plus Bevy's default font. A custom
//! resolver maps an SVG's `font-family` list through the same names,
//! generic keywords and face fallback (bold-italic → italic → bold →
//! regular) as the page's CSS, never through the names inside the font
//! files. A family it doesn't know falls back to Bevy's default font.
//!
//! [`sync_svg_fonts`] rebuilds the snapshot when `FontFamilies` changes or
//! a font it names finishes loading; then `.svg` files that contain
//! `<text>` are reloaded and UIs with inline `<svg>` text restyle, so text
//! drawn before its font arrived is drawn again.
//!
//! Limit: system font families (`FontSource::Family`, `Generic`, …) have no
//! bytes in Bevy's assets; SVG text naming them uses the default font.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, RwLock};

use bevy::asset::AssetPath;
use bevy::prelude::*;
use bevy::text::FontSource;
use resvg::usvg::{self, fontdb};

use crate::fonts::{FontFamilies, GenericFamily};

/// The fonts SVG `<text>` resolves against at one moment.
pub(crate) struct SvgFontSet {
    /// Bumped whenever the set's content changes.
    pub generation: u64,
    db: Arc<fontdb::Database>,
    /// By lowercase family name: the face for (bold, italic), indexed
    /// `bold as usize * 2 + italic as usize`.
    families: HashMap<String, [Option<fontdb::ID>; 4]>,
    generics: HashMap<GenericFamily, String>,
    default: Option<fontdb::ID>,
    /// What the set was built from, to tell a real change from a repeat.
    signature: Vec<(String, [Option<AssetId<Font>>; 4], [bool; 4])>,
}

impl SvgFontSet {
    fn empty() -> Self {
        Self {
            generation: 0,
            db: Arc::new(fontdb::Database::new()),
            families: HashMap::new(),
            generics: HashMap::new(),
            default: None,
            signature: Vec::new(),
        }
    }

    /// The face for a usvg font request: the first family in its list the
    /// set knows, with CSS's weight/style fallback, else the default font.
    pub(crate) fn select(&self, font: &usvg::Font) -> Option<fontdb::ID> {
        let bold = font.weight() > 500;
        let italic = !matches!(font.style(), usvg::FontStyle::Normal);
        let index = usize::from(bold) * 2 + usize::from(italic);
        for family in font.families() {
            let name = match family {
                usvg::FontFamily::Named(name) => Some(name.as_str()),
                usvg::FontFamily::Serif => self.generic(GenericFamily::Serif),
                usvg::FontFamily::SansSerif => self.generic(GenericFamily::SansSerif),
                usvg::FontFamily::Monospace => self.generic(GenericFamily::Monospace),
                usvg::FontFamily::Cursive => self.generic(GenericFamily::Cursive),
                usvg::FontFamily::Fantasy => self.generic(GenericFamily::Fantasy),
            };
            if let Some(id) = name
                .and_then(|name| self.families.get(&name.to_ascii_lowercase()))
                .and_then(|faces| faces[index])
            {
                return Some(id);
            }
        }
        self.default
    }

    fn generic(&self, generic: GenericFamily) -> Option<&str> {
        self.generics.get(&generic).map(String::as_str)
    }

    /// Points usvg's options at this set.
    pub(crate) fn apply(self: &Arc<Self>, options: &mut usvg::Options<'static>) {
        options.fontdb = self.db.clone();
        let set = self.clone();
        options.font_resolver = usvg::FontResolver {
            select_font: Box::new(move |font, _db| set.select(font)),
            select_fallback: usvg::FontResolver::default_fallback_selector(),
        };
    }
}

/// The current [`SvgFontSet`], shared with the loaders (they run on IO
/// threads), plus the `.svg` paths whose text depends on it.
#[derive(Default)]
pub(crate) struct Shared {
    current: RwLock<Option<Arc<SvgFontSet>>>,
    text_paths: Mutex<HashSet<AssetPath<'static>>>,
}

impl Shared {
    pub(crate) fn current(&self) -> Arc<SvgFontSet> {
        self.current
            .read()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone()
            .unwrap_or_else(|| Arc::new(SvgFontSet::empty()))
    }

    /// A loaded `.svg` file whose text needs a redraw when fonts change.
    pub(crate) fn note_text(&self, path: AssetPath<'static>) {
        self.text_paths
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .insert(path);
    }
}

/// Whether SVG source contains text (the only content fonts affect).
pub(crate) fn has_text(source: &[u8]) -> bool {
    source.windows(5).any(|window| window == b"<text")
}

/// Rebuilds the [`SvgFontSet`] when [`FontFamilies`] changes or a font it
/// uses (or Bevy's default font) loads; on a real change, reloads the
/// `.svg` files with text and restyles the UIs (inline `<svg>` keys its
/// drawing by the set's generation, so a restyle draws it again).
#[hotpath::measure]
pub(crate) fn sync_svg_fonts(
    fonts: Res<crate::svg::SvgFonts>,
    mut families: ResMut<FontFamilies>,
    assets: Res<Assets<Font>>,
    mut events: MessageReader<AssetEvent<Font>>,
    server: Res<AssetServer>,
) {
    let loaded = events.read().any(|event| {
        matches!(
            event,
            AssetEvent::Added { .. }
                | AssetEvent::Modified { .. }
                | AssetEvent::LoadedWithDependencies { .. }
        )
    });
    if !loaded && !families.is_changed() {
        return;
    }
    let shared = fonts.shared();
    let previous = shared.current();
    let default_handle: Handle<Font> = Handle::default();
    let handle_of = |source: &FontSource| match source {
        FontSource::Handle(handle) => Some(handle.clone()),
        _ => None,
    };
    // What a set would be built from now.
    let combos = [(false, false), (false, true), (true, false), (true, true)];
    let mut signature: Vec<(String, [Option<AssetId<Font>>; 4], [bool; 4])> = families
        .iter()
        .map(|(name, faces)| {
            let handles = combos.map(|(bold, italic)| handle_of(&faces.face(bold, italic)));
            (
                name.to_ascii_lowercase(),
                handles
                    .clone()
                    .map(|handle| handle.map(|handle| handle.id())),
                handles.map(|handle| handle.is_some_and(|handle| assets.contains(&handle))),
            )
        })
        .collect();
    signature.push((
        String::new(),
        [Some(default_handle.id()), None, None, None],
        [assets.contains(&default_handle), false, false, false],
    ));
    let generics_now: HashMap<GenericFamily, String> = [
        GenericFamily::Serif,
        GenericFamily::SansSerif,
        GenericFamily::Monospace,
        GenericFamily::Cursive,
        GenericFamily::Fantasy,
    ]
    .into_iter()
    .filter_map(|generic| Some((generic, families.generic(generic)?.to_owned())))
    .collect();
    if signature == previous.signature && generics_now == previous.generics {
        return;
    }

    // Build: each loaded font file once, then the families' faces by id.
    let mut db = fontdb::Database::new();
    let mut ids: HashMap<AssetId<Font>, fontdb::ID> = HashMap::new();
    let mut load = |handle: &Handle<Font>, db: &mut fontdb::Database| -> Option<fontdb::ID> {
        if let Some(id) = ids.get(&handle.id()) {
            return Some(*id);
        }
        let font = assets.get(handle)?;
        let blob = font.data.clone();
        let id = *db
            .load_font_source(fontdb::Source::Binary(Arc::new(blob)))
            .first()?;
        ids.insert(handle.id(), id);
        Some(id)
    };
    let mut by_name = HashMap::new();
    for (name, faces) in families.iter() {
        let faces = combos.map(|(bold, italic)| {
            handle_of(&faces.face(bold, italic)).and_then(|handle| load(&handle, &mut db))
        });
        by_name.insert(name.to_ascii_lowercase(), faces);
    }
    let default = load(&default_handle, &mut db);
    let set = SvgFontSet {
        generation: previous.generation + 1,
        db: Arc::new(db),
        families: by_name,
        generics: generics_now,
        default,
        signature,
    };
    *shared
        .current
        .write()
        .unwrap_or_else(|poison| poison.into_inner()) = Some(Arc::new(set));

    // Text drawn with the old set: draw it again.
    let paths: Vec<AssetPath<'static>> = shared
        .text_paths
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .iter()
        .cloned()
        .collect();
    for path in paths {
        server.reload(path);
    }
    // Inline `<svg>` with text keys its drawing by the generation: a
    // restyle (the `FontFamilies` change signal) collects the new key. The
    // next run sees the same signature and stops, so this doesn't loop.
    if loaded {
        families.set_changed();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::fonts::FontFaces;

    /// The app's fonts: three copies of Bevy's default font as separate
    /// assets (distinct fontdb faces), plus the default font itself.
    fn app_with_families() -> (App, [Handle<Font>; 3]) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Font>()
            .init_resource::<FontFamilies>()
            .init_resource::<crate::svg::SvgFonts>()
            .add_systems(Update, sync_svg_fonts);
        let font = || Font::from_bytes(bevy::text::DEFAULT_FONT_DATA.to_vec());
        let mut assets = app.world_mut().resource_mut::<Assets<Font>>();
        let _ = assets.insert(AssetId::default(), font());
        let handles = [assets.add(font()), assets.add(font()), assets.add(font())];
        let mut families = app.world_mut().resource_mut::<FontFamilies>();
        families
            .insert(
                "Alpha",
                FontFaces::new(handles[0].clone()).with_bold(handles[1].clone()),
            )
            .insert("Beta", FontFaces::new(handles[2].clone()))
            .set_generic(GenericFamily::Serif, "Beta");
        app.update();
        app.update();
        (app, handles)
    }

    /// The resolved face of each `<text>`'s glyphs, in document order.
    fn glyph_faces(tree: &usvg::Tree) -> Vec<fontdb::ID> {
        fn walk(group: &usvg::Group, out: &mut Vec<fontdb::ID>) {
            for node in group.children() {
                match node {
                    usvg::Node::Group(group) => walk(group, out),
                    usvg::Node::Text(text) => out.extend(
                        text.layouted()
                            .iter()
                            .flat_map(|span| span.positioned_glyphs.iter().map(|glyph| glyph.font))
                            .take(1),
                    ),
                    _ => {}
                }
            }
        }
        let mut out = Vec::new();
        walk(tree.root(), &mut out);
        out
    }

    /// An SVG's `font-family` resolves as the page's CSS does: registered
    /// names (any case), generic keywords through `set_generic`, CSS's face
    /// fallback (no italic face → regular), unknown names → Bevy's default
    /// font, never the names inside the font files (all four are FiraMono).
    #[test]
    fn font_families_resolve_like_css() {
        let (app, _) = app_with_families();
        let fonts = app.world().resource::<crate::svg::SvgFonts>().clone();
        let set = fonts.shared().current();
        assert!(set.generation >= 1);
        let source = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100">
            <text y="10" font-family="Alpha">a</text>
            <text y="20" font-family="Alpha" font-weight="bold">b</text>
            <text y="30" font-family="nope, serif">c</text>
            <text y="40" font-family="FiraMono">d</text>
            <text y="50" font-family="ALPHA" font-style="italic">e</text>
        </svg>"#;
        let tree = crate::svg::parse_with(source.as_bytes(), &fonts).unwrap();
        let alpha = set.families["alpha"];
        let beta = set.families["beta"];
        assert_eq!(
            glyph_faces(&tree),
            vec![
                alpha[0].unwrap(),
                alpha[2].unwrap(),
                beta[0].unwrap(),
                set.default.unwrap(),
                alpha[1].unwrap(),
            ]
        );
        // Every face is distinct, so the comparison means something.
        let mut ids = vec![alpha[0], alpha[2], beta[0], set.default];
        ids.dedup();
        assert_eq!(ids.len(), 4);
        assert_eq!(alpha[1], alpha[0], "no italic face: the regular one");
    }

    /// The set is rebuilt only when what it's built from changes: another
    /// frame (or a `FontFamilies` change signal the system itself sent)
    /// keeps the generation, a new family bumps it.
    #[test]
    fn the_font_set_rebuilds_only_on_a_change() {
        let (mut app, handles) = app_with_families();
        let current = |app: &App| {
            app.world()
                .resource::<crate::svg::SvgFonts>()
                .shared()
                .current()
                .generation
        };
        let before = current(&app);
        app.update();
        app.update();
        app.world_mut().resource_mut::<FontFamilies>().set_changed();
        app.update();
        assert_eq!(current(&app), before);
        app.world_mut()
            .resource_mut::<FontFamilies>()
            .insert("Gamma", FontFaces::new(handles[2].clone()));
        app.update();
        assert_eq!(current(&app), before + 1);
    }

    #[test]
    fn text_is_found_in_source() {
        assert!(has_text(b"<svg><text>A</text></svg>"));
        assert!(has_text(b"<svg><textPath/></svg>"));
        assert!(!has_text(b"<svg><rect/></svg>"));
    }
}
