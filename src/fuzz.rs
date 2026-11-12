//! Fuzzing harnesses, behind the `fuzzing` cargo feature (used by the
//! `fuzz/`, `honggfuzz/`, `fuzzcheck/` and `test-fuzz/` drivers).
//! `#[doc(hidden)]` and not covered by semver: these call internal glue
//! directly, because the public API reaches it only through a full Bevy app,
//! which is far too slow per fuzz iteration.
//!
//! Contract for every function: arbitrary `&str` inputs may produce `Err` or
//! arbitrary output, but must never panic, hang, or abort.

use bevy::asset::RenderAssetUsages;
use bevy::image::Image;
use bevy::prelude::*;

use crate::build::{BoxStyle, Styler, element_tag, root_style};
use crate::cascade::{
    HtmlStyles, Pseudo, custom_image_urls, image_urls, nesting_over_limit, var_names,
};
use crate::fonts::FontFamilies;
use crate::html::HtmlElement;
use crate::l10n::LocalizedText;
use crate::style::Stylesheet;
use crate::template::HtmlTemplate;

/// A stand-in `frame.png`: 32×24 RGBA, so `%` slices resolve to the same
/// arithmetic as the test fixture.
fn fake_frame() -> Image {
    Image::new_fill(
        wgpu_types::Extent3d {
            width: 32,
            height: 24,
            depth_or_array_layers: 1,
        },
        wgpu_types::TextureDimension::D2,
        &[255; 4],
        wgpu_types::TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

/// Tera compile + render with a JSON context, then `tl` parse + outline +
/// entity decoding: the whole Render stage on arbitrary sources. The same
/// bytes also run through the Markdown path (ADR 0014): the conversion and
/// its parse must not panic either.
pub fn render_html(source: &str, context_json: &str) -> Result<String, String> {
    let name = "fuzz/template.html";
    // The loader's pre-scan, so the harness compiles what an app would
    // (bug_0182: without it, long operator chains overflowed the 2 MB
    // replay threads of debug builds inside Tera's compiler).
    crate::template::check_tera_operators(name, source)?;
    let mut tera = crate::template::new_tera();
    tera.add_raw_template(name, source)
        .map_err(|err| err.to_string())?;
    // The loader's safety check (ADR 0044): only bounded templates render.
    crate::tera_check::check(name, &[(name.to_owned(), source.to_owned())])
        .map_err(|err| err.to_string())?;
    let value: serde_json::Value =
        serde_json::from_str(context_json).map_err(|err| err.to_string())?;
    let context = tera::Context::from_serialize(&value).map_err(|err| err.to_string())?;
    let template = HtmlTemplate {
        tera,
        name: name.to_owned(),
    };
    let document = template.render(&context).map_err(|err| err.to_string())?;
    let outline = document.outline(&LocalizedText::default());

    let md_name = "fuzz/template.md";
    // The `.md` path: the load-time Markdown conversion (`to_html_template`,
    // ADR 0020) runs on the same bytes, then the converted template renders.
    let md_source = crate::md::to_html_template(source);
    crate::template::check_tera_operators(md_name, &md_source)?;
    let mut tera = crate::template::new_tera();
    tera.add_raw_template(md_name, &md_source)
        .map_err(|err| err.to_string())?;
    crate::tera_check::check(md_name, &[(md_name.to_owned(), md_source.clone())])
        .map_err(|err| err.to_string())?;
    let template = HtmlTemplate {
        tera,
        name: md_name.to_owned(),
    };
    let document = template.render(&context).map_err(|err| err.to_string())?;
    let _ = document.outline(&LocalizedText::default());

    Ok(outline)
}

/// lightningcss parse + cascade over synthetic elements + the `html` root
/// box, with every `url()` bound to the fake frame image (exercises slice
/// arithmetic and the style cache). Errors are values, not crashes.
pub fn cascade(css: &str) -> Result<String, String> {
    use lightningcss::stylesheet::{ParserOptions, StyleSheet};
    use lightningcss::traits::IntoOwned;

    // The loader's pre-scan, so the harness parses what an app would
    // (bug_0179: without it, corpus inputs nested ~500 deep overflowed
    // the 2 MB replay threads and nested functions timed the fuzzer out,
    // on inputs every app rejects).
    if nesting_over_limit(css) {
        return Err("stylesheet nests too deeply".into());
    }
    // Guarded as the loader's parse is (bug_0238): the known debug-only
    // panic unwinds into it instead of aborting the fuzz target.
    excuse_known_upstream_debug_panics();
    let parsed = crate::cascade::guarded_parse(|| {
        StyleSheet::parse(css, ParserOptions::default()).map_err(|err| err.to_string())
    })??;
    let sheet = parsed.into_owned();
    // The load-time scans: untrusted CSS, must not panic or hang. The
    // url → handle map mirrors the loader's (skipped URLs get no entry).
    let mut images = bevy::platform::collections::HashMap::new();
    let mut assets = Assets::<Image>::default();
    for url in custom_image_urls(&sheet)
        .into_iter()
        .chain(image_urls(&sheet))
    {
        images.insert(url, assets.add(fake_frame()));
    }
    let stylesheet = Stylesheet {
        var_names: var_names(&sheet),
        images,
        sheet,
    };

    let styles = HtmlStyles::from_sheet(&stylesheet.sheet);
    let fonts = FontFamilies::default();
    let root = root_style(&styles, &fonts, &assets, None, &element_tag("html"), None);
    let styler = Styler {
        styles: &styles,
        fonts: &fonts,
        root_size: root.size,
        sheet: Some(&stylesheet),
        images: &assets,
        server: None,
        template: None,
        #[cfg(feature = "svg")]
        inline_svgs: None,
    };

    let elements = [
        HtmlElement {
            tag: "html".into(),
            id: None,
            classes: vec![],
            ..Default::default()
        },
        HtmlElement {
            tag: "p".into(),
            id: None,
            classes: vec![],
            ..Default::default()
        },
        HtmlElement {
            tag: "p".into(),
            id: Some("lead".into()),
            classes: vec!["note".into()],
            ..Default::default()
        },
        HtmlElement {
            tag: "div".into(),
            id: None,
            classes: vec!["panel".into(), "wide".into()],
            ..Default::default()
        },
        HtmlElement {
            tag: "li".into(),
            id: None,
            classes: vec![],
            ..Default::default()
        },
        HtmlElement {
            tag: "pre".into(),
            id: None,
            classes: vec![],
            ..Default::default()
        },
    ];
    let mut out = String::new();
    // Ancestor chains for the combinator matching: descendants of the
    // elements (nearest first), plus pseudo states on them. Exercises the
    // `A B`/`A > B` paths, which the old harness passed no ancestors for.
    let chains: Vec<(Vec<crate::cascade::Ancestor>, Pseudo)> = vec![
        // The no-ancestor case (the element's own pseudo only).
        (vec![], Pseudo::default()),
        (
            vec![
                crate::cascade::Ancestor {
                    tag: "section".into(),
                    id: None,
                    classes: vec!["page".into()],
                    submit: false,
                    pseudo: Pseudo::default(),
                    ..Default::default()
                },
                crate::cascade::Ancestor {
                    tag: "div".into(),
                    id: None,
                    classes: vec!["panel".into()],
                    submit: false,
                    pseudo: Pseudo {
                        hover: true,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            ],
            Pseudo::default(),
        ),
        (
            vec![crate::cascade::Ancestor {
                tag: "ul".into(),
                id: None,
                classes: vec![],
                submit: false,
                pseudo: Pseudo::default(),
                ..Default::default()
            }],
            Pseudo {
                focus_visible: true,
                ..Default::default()
            },
        ),
    ];
    // Structural states (ADR 0028): none, and a middle child with
    // preceding siblings (the structural pseudo-classes, `+`/`~`).
    let sibling = |tag: &str, classes: &[&str]| crate::cascade::Ancestor {
        tag: tag.into(),
        id: None,
        classes: classes.iter().map(|class| (*class).into()).collect(),
        submit: false,
        pseudo: Pseudo {
            hover: true,
            ..Default::default()
        },
        dataset: Some(std::rc::Rc::new(std::collections::BTreeMap::from([(
            "mode".to_owned(),
            "dark".to_owned(),
        )]))),
        position: crate::cascade::Position {
            tag_index: 1,
            tag_count: 2,
            ..Default::default()
        },
        ..Default::default()
    };
    let states = [
        crate::cascade::ElementState::default(),
        crate::cascade::ElementState {
            tag_index: 3,
            tag_count: 5,
            type_index: 2,
            type_count: 3,
            focus_within: true,
            empty: false,
            previous_siblings: Some(crate::cascade::Siblings::of(vec![
                sibling("h1", &["title"]),
                sibling("p", &[]),
            ])),
        },
    ];
    // One hit per element (with and without ancestors), then a repeat:
    // exercises the cascade's match cache.
    for round in 0..2 {
        for element in &elements {
            for ((ancestors, pseudo), state) in chains
                .iter()
                .flat_map(|chain| states.iter().map(move |state| (chain, state)))
            {
                let style = styler.style_of(element, &root, *pseudo, None, ancestors, &state);
                let boxed: BoxStyle = styler.box_of(element, *pseudo, None, ancestors, &state);
                std::fmt::write(
                    &mut out,
                    format_args!(
                        "round {round} {}#{}{:?}: color={:?} family={:?} size={} bold={} italic={} \
                     border={:?} padding={:?} bg={:?} image={} gap={:?}\n",
                        element.tag,
                        element.id.as_deref().unwrap_or(""),
                        element.classes,
                        style.color,
                        style.family,
                        style.size,
                        style.bold,
                        style.italic,
                        boxed.border,
                        boxed.padding,
                        boxed.background,
                        boxed.image.as_ref().map_or(0, |_| 1),
                        boxed.row_gap,
                    ),
                )
                .ok();
            }
        }
    }
    Ok(out)
}

/// Fluent bundle compilation + `translate` with arbitrary message ids and
/// `data-l10n-args` JSON: parse failures and missing messages are values.
pub fn translate(ftl: &str, id: &str, args_json: &str) -> Result<String, String> {
    // Unparsable FTL is skipped: this fluent version drops the partial
    // resource, so there is no recovery path to fuzz. The loader's safety check (ADR 0045): what it refuses never reaches
    // fluent-syntax.
    crate::ftl_check::check(ftl).map_err(|err| err.to_string())?;
    let resource =
        fluent::FluentResource::try_new(ftl.to_owned()).map_err(|errors| format!("{errors:?}"))?;
    let locale: unic_langid::LanguageIdentifier = "en".parse().map_err(|err| format!("{err:?}"))?;
    let mut bundle = fluent::concurrent::FluentBundle::new_concurrent(vec![locale]);
    bundle
        .add_resource(resource)
        .map_err(|errors| format!("{errors:?}"))?;
    let text = crate::l10n::translate(&bundle, id, Some(args_json))?;
    // The `markdown: true` path (ADR 0020): the same source builds the
    // cache, and the formatted value substitutes into the converted HTML.
    let _ = crate::l10n::markdown_fuzz(ftl, id, args_json);
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The cascade harness itself: untrusted CSS through the load-time
    /// scans (`var_names`, the URL lists — never fuzzed before) and the
    /// element/ancestor sweep must terminate with no panic.
    #[test]
    fn cascade_harness_is_safe_on_adversarial_css() {
        for css in [
            "p { --a: var(--a, 1px) var(--a, 1px); width: var(--a) }",
            concat!(
                "p { --v0: 1px; ",
                "--v1: var(--v0); --v999: var(--v998); width: var(--v999) }"
            ),
            "p { --icon: url(\"data:image/svg+xml,<svg/>\"); border-image: var(--icon) 4 }",
            "p { grid-template-columns: repeat(40000, [a] 1fr); border-image: url(\"x.png\") 4 }",
        ] {
            let _ = cascade(&format!("{css} NONSENSE {css}"));
            let out = cascade(css).unwrap_or_default();
            assert!(!out.is_empty(), "the harness output nothing for: {css}");
        }
    }

    /// bug_0005, found by honggfuzz (`ftl` target): fluent-syntax 0.11.1
    /// quotes an invalid `\U` escape plus the character that ended it, sliced
    /// one byte past the escape — inside that character if it's multi-byte.
    /// The load-time check (`ftl_check`, ADR 0045) refuses it before
    /// fluent-syntax runs; this must never panic.
    #[test]
    fn broken_unicode_escape_after_multibyte_char_does_not_panic() {
        let ftl = "u={\"\\U\u{fffd}";
        assert!(translate(ftl, "", "").is_err());
    }

    /// bug_0015: expressions nested far beyond `MAX_FTL_NESTING` are refused
    /// before fluent-syntax recurses, even on a 2 MB thread, where unbounded
    /// debug builds overflowed between 200 and 400 levels. Both recursion
    /// paths: placeables and call arguments.
    #[test]
    fn deeply_nested_expressions_are_an_error_not_a_stack_overflow() {
        let placeables = format!("x = {}\"a\"{}\n", "{".repeat(100_000), "}".repeat(100_000));
        let calls = format!(
            "x = {{ {}1{} }}\n",
            "F(".repeat(100_000),
            ")".repeat(100_000)
        );
        for ftl in [placeables, calls] {
            let result = std::thread::Builder::new()
                .stack_size(2 * 1024 * 1024)
                .spawn(move || translate(&ftl, "x", "{}"))
                .unwrap()
                .join()
                .expect("no stack overflow");
            assert!(result.is_err(), "{result:?}");
        }
    }

    /// Nesting up to the limit still parses and formats; one more level is
    /// refused (the whole resource: fluent-syntax would recurse on it).
    #[test]
    fn nesting_up_to_the_limit_still_works() {
        let limit = crate::ftl_check::MAX_FTL_NESTING;
        let nested = |depth: usize| format!("{}\"a\"{}", "{".repeat(depth), "}".repeat(depth));
        assert_eq!(
            translate(&format!("x = {}\n", nested(limit)), "x", "{}"),
            Ok("a".to_owned())
        );
        let deeper = translate(
            &format!("deep = {}\nok = fine\n", nested(limit + 1)),
            "ok",
            "{}",
        );
        assert!(deeper.is_err_and(|err| err.contains("MAX_FTL_NESTING")));
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(64))]

        /// The cascade's per-element style cache is transparent: the second
        /// round over the same elements (all cache hits) reports exactly
        /// what the first computed, for stylesheets built from selectors
        /// matching the harness elements and every mapped property. Catches
        /// a cache key missing a part (id/classes) or a cached value that
        /// depends on the first caller's inherited style.
        #[test]
        fn style_cache_is_transparent(
            rules in proptest::collection::vec(
                (
                    proptest::sample::select(&["html", "p", "p#lead", ".note", "div.panel.wide", ".wide", "*", "li", "pre"][..]),
                    proptest::sample::select(&[
                        "color: #123456", "font-size: 1.5em", "font-size: 2rem", "font-size: 12px",
                        "font-weight: bold", "font-style: italic", "padding: 3px 4px", "border-width: 2px",
                        "background-color: red", "gap: 5px", "font-family: serif",
                        r#"border-image: url("f.png") 25% fill round"#,
                    ][..]),
                    proptest::bool::ANY,
                ),
                1..8,
            )
        ) {
            let css: String = rules
                .iter()
                .map(|(selector, declaration, important)| {
                    format!("{selector} {{ {declaration}{} }}\n", if *important { " !important" } else { "" })
                })
                .collect();
            let out = cascade(&css).unwrap();
            let (first, second): (Vec<&str>, Vec<&str>) = out.lines().partition(|line| line.starts_with("round 0 "));
            proptest::prop_assert_eq!(first.len(), second.len());
            for (a, b) in first.iter().zip(&second) {
                proptest::prop_assert_eq!(&a["round 0 ".len()..], &b["round 1 ".len()..], "css:\n{}", css);
            }
        }
    }

    /// bug_0178: the html target's crash (honggfuzz, 2026-10-26), minimized
    /// with `cargo fuzz tmin` to these 9 bytes (also a seed in
    /// fuzz/seeds/html). The `.md` path's conversion panicked inside
    /// pulldown-cmark 0.13.4 (UPSTREAM.md U13); this must never panic.
    #[test]
    fn a_refdef_list_item_crash_from_honggfuzz_does_not_panic() {
        let bytes = b"+\t[<]:\xda\n\x0b";
        let text = String::from_utf8_lossy(bytes);
        let _ = render_html(&text, "{}");
    }

    /// bug_0179: the harness applies the loader's nesting pre-scan. The
    /// nightly coverage replay aborted on a CI corpus input nested 505
    /// deep (the 2 MB test thread overflowed inside lightningcss), and the
    /// css target timed out on nested `light-dark(` (seeds in fuzz/seeds/css).
    /// Runs on the test harness's ordinary 2 MB thread, as the replay does.
    #[test]
    fn deep_and_exponential_css_is_rejected_like_the_loader_does() {
        let start = std::time::Instant::now();
        for css in [
            format!("a{{width:{}1px{}}}", "calc(".repeat(3000), ")".repeat(3000)),
            format!("a{{x:{}", "[".repeat(505)),
            format!("a{{color:{}", "light-dark(".repeat(30)),
            // bug_0183: after a name that only ends in `url`.
            format!("a{{color:\0url({}", "light-dark(".repeat(30)),
        ] {
            assert!(cascade(&css).is_err(), "rejected: {}", &css[..20]);
        }
        assert!(start.elapsed().as_secs() < 5);
        assert!(cascade("a{width:calc(min(100%, max(10px, 2em)) - 4px)}").is_ok());
    }

    /// bug_0180: the css target's crash (2026-10-28, a 3-minute local run
    /// once the light-dark timeouts stopped masking it), minimized with
    /// `cargo fuzz tmin` to these 8 bytes (also a seed in fuzz/seeds/css).
    /// `Compound::parse` sliced a U+FFFD apart; this must never panic.
    #[test]
    fn a_structural_pseudo_crash_from_cargo_fuzz_does_not_panic() {
        let bytes = b":not\0(){";
        let _ = cascade(&String::from_utf8_lossy(bytes));
    }
}

/// The conversion budget of the `svg` target, scaled down from
/// [`crate::svg::MAX_SVG_WORK`] like its render cost: the target times each
/// input, so what the budget lets through must convert in milliseconds
/// even under ASan (a slow input means the work bound missed a cost).
pub const SVG_FUZZ_WORK: u64 = crate::svg::MAX_SVG_WORK / 16;

/// The `svg` loader's whole path on arbitrary bytes: usvg parse (with the
/// refusing image resolver), the size cap, the render-cost estimate, and
/// the resvg draw when the estimate allows it. Drawn within a 256 px side
/// and a budget scaled to that area, so one input stays fast; the fuzz
/// target times the call, and a slow one means the estimate missed a cost.
/// `Ok` carries the size and the estimate.
/// The fonts the `svg` harness draws `<text>` with, built once.
fn svg_fonts() -> &'static crate::svg::SvgFonts {
    static FONTS: std::sync::OnceLock<crate::svg::SvgFonts> = std::sync::OnceLock::new();
    FONTS.get_or_init(crate::svg::SvgFonts::default_font)
}

pub fn svg(source: &[u8]) -> Result<String, String> {
    let side = 256;
    let full = u64::from(crate::svg::MAX_SVG_SIDE).pow(2);
    let limits = crate::svg::Limits {
        side,
        cost: crate::svg::MAX_SVG_COST / (full / u64::from(side).pow(2)),
        work: SVG_FUZZ_WORK,
    };
    // The inline-`<svg>` path too (ADR 0038 §4): the same bytes as an
    // element's source text, prepared (namespaces, the cascade color).
    // `<text>` draws with Bevy's default font (`fuzzing` implies
    // `svg-text`; bug_0221: without fonts text drew nothing).
    let fonts = svg_fonts();
    if let Some(prepared) = std::str::from_utf8(source)
        .ok()
        .and_then(|text| crate::svg::prepare_inline(text, Some([255, 0, 0, 255])))
    {
        let _ = crate::svg::rasterize_with(prepared.as_bytes(), 1.0, limits, fonts);
    }
    let raster =
        crate::svg::rasterize_with(source, 1.0, limits, fonts).map_err(|err| err.to_string())?;
    Ok(format!(
        "{}x{} cost {}",
        raster.width, raster.height, raster.cost
    ))
}

/// Whether a panic is one of the known debug-build-only upstream panics,
/// which the fuzz targets let unwind into the library's guard
/// (`svg::guarded`, `cascade::guarded_parse`) instead of aborting.
/// Release builds don't panic there, and in debug builds the guard
/// already turns the panic into an error. `file` is the panic
/// location's path; the crate directory name pins the version, so a bump
/// makes the fuzzer report it again until rechecked.
///
/// - UPSTREAM U20 (resvg #1185): resvg's turbulence overflows `i32`
///   lattice indices after many octaves or on a huge `baseFrequency`
///   (release wraps, then masks into range).
/// - UPSTREAM U45 (bug_0238): cssparser-color's `hsl_to_rgb`
///   `debug_assert!`s the hue lightningcss hands it, NaN for an infinite
///   `hsl()`/`hwb()` hue.
///
/// tiny-skia's far-off-canvas panics (U24, U27, U28) are not excused: they
/// panic in release builds too, and `svg::check_extent` refuses their
/// inputs before drawing, so one reaching tiny-skia is a miss to report.
pub fn is_known_upstream_debug_panic(file: &str, message: &str) -> bool {
    let file = file.replace('\\', "/");
    let turbulence = file.contains("/resvg-0.49.0/")
        && file.ends_with("/src/filter/turbulence.rs")
        && message.starts_with("attempt to ")
        && message.ends_with(" with overflow");
    let hue = file.contains("/cssparser-color-0.5.0/")
        && file.ends_with("/lib.rs")
        && message == "assertion failed: (0.0..=1.0).contains(&hue)";
    turbulence || hue
}

/// Makes the driver's panic hook skip the known upstream debug-only panics
/// ([`is_known_upstream_debug_panic`]), so they unwind into the library's
/// guard instead of aborting the fuzz target. Call it from inside the target's
/// closure: the drivers install their aborting hook before the first
/// input, and this wraps whichever hook is current, once.
pub fn excuse_known_upstream_debug_panics() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let aborting = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let excused = info.location().is_some_and(|location| {
                is_known_upstream_debug_panic(location.file(), info.payload_as_str().unwrap_or(""))
            });
            if !excused {
                aborting(info);
            }
        }));
    });
}

/// How long one `svg` input may take in a fuzz target.
pub const SVG_FUZZ_TIME: std::time::Duration = std::time::Duration::from_secs(5);

/// The `svg` fuzz target, shared by every driver: [`svg`] on inputs up to
/// 64 KiB, timed. The budget allows a few milliseconds of drawing, so a
/// slow input means the estimate missed a cost (or a parse step is
/// superlinear) and panics here instead of passing as a mere refusal.
pub fn svg_target(data: &[u8]) {
    excuse_known_upstream_debug_panics();
    if data.len() > 64 * 1024 {
        return;
    }
    let started = std::time::Instant::now();
    let outcome = svg(data);
    let elapsed = started.elapsed();
    assert!(
        elapsed < SVG_FUZZ_TIME,
        "svg took {elapsed:?} ({outcome:?}): the depth check or the cost estimate missed a cost"
    );
}

/// The `svg` loader's size and cost estimate at its default limits, without
/// drawing: `(width, height, estimate)`. For calibrating the estimate
/// against resvg's real render time (probe report §7).
pub fn svg_estimate(source: &[u8]) -> Result<(u32, u32, u64), String> {
    let tree = crate::svg::parse(source).map_err(|err| err.to_string())?;
    let (width, height, _) = crate::svg::raster_size(&tree, 1.0, crate::svg::MAX_SVG_SIDE)
        .map_err(|err| err.to_string())?;
    Ok((
        width,
        height,
        crate::svg::estimate_cost(&tree, width, height, u64::MAX),
    ))
}

/// A parsed SVG, for the benchmarks' stage measurements
/// (`benches/svg.rs`, `benches/svg-divan.rs`).
pub struct SvgTree(resvg::usvg::Tree);

impl SvgTree {
    /// The intrinsic size the loader draws at (scale 1).
    pub fn intrinsic(&self) -> (u32, u32) {
        let (width, height, _) = crate::svg::raster_size(&self.0, 1.0, crate::svg::MAX_SVG_SIDE)
            .unwrap_or((1, 1, false));
        (width, height)
    }
}

/// The loader's parse: roxmltree, the recursion-depth bound, usvg.
pub fn svg_parse(source: &[u8]) -> Result<SvgTree, String> {
    crate::svg::parse(source)
        .map(SvgTree)
        .map_err(|err| err.to_string())
}

/// Only the XML parse and the depth and work bounds (what the check adds
/// before usvg runs): the depth.
pub fn svg_depth(source: &str) -> Result<u32, String> {
    let options = roxmltree::ParsingOptions {
        allow_dtd: true,
        ..roxmltree::ParsingOptions::default()
    };
    let document =
        roxmltree::Document::parse_with_options(source, options).map_err(|err| err.to_string())?;
    crate::svg_bounds::bounds(&document, crate::svg_bounds::MAX_REFERENCES, u64::MAX)
        .map(|bounds| bounds.depth)
        .map_err(|err| err.to_string())
}

/// The depth and work bounds of `source` (ADR 0040): `(depth, work)`.
pub fn svg_work(source: &str) -> Result<(u32, u64), String> {
    let options = roxmltree::ParsingOptions {
        allow_dtd: true,
        ..roxmltree::ParsingOptions::default()
    };
    let document =
        roxmltree::Document::parse_with_options(source, options).map_err(|err| err.to_string())?;
    crate::svg_bounds::bounds(&document, crate::svg_bounds::MAX_REFERENCES, u64::MAX)
        .map(|bounds| (bounds.depth, bounds.work))
        .map_err(|err| err.to_string())
}

/// usvg's conversion with the work bound off (the depth bound stays): for
/// calibrating the bound against real conversion time.
pub fn svg_convert_unbounded(source: &[u8]) -> Result<SvgTree, String> {
    crate::svg::parse_within(source, &crate::svg::SvgFonts::default(), u64::MAX)
        .map(SvgTree)
        .map_err(|err| err.to_string())
}

/// The render-cost estimate at `width`×`height`.
pub fn svg_cost(tree: &SvgTree, width: u32, height: u32) -> u64 {
    crate::svg::estimate_cost(&tree.0, width, height, u64::MAX)
}

/// The budgeted draw at `width`×`height` (the loader's and the redraw's
/// work), returning the image's byte count.
pub fn svg_draw(tree: &SvgTree, width: u32, height: u32) -> Result<usize, String> {
    crate::svg::draw(&tree.0, width, height, crate::svg::Limits::DEFAULT)
        .map(|(image, _)| image.data.map_or(0, |data| data.len()))
        .map_err(|err| err.to_string())
}

/// An inline `<svg>`'s preparation (namespaces, the cascade color).
pub fn svg_prepare_inline(raw: &str) -> Option<String> {
    crate::svg::prepare_inline(raw, Some([255, 0, 0, 255]))
}

#[cfg(test)]
mod svg_tests {
    /// The harness draws `<text>` with a font, so the fuzzer reaches text's
    /// paths (bug_0221): bug_0204's pattern on text is refused, which
    /// without a font drew nothing and passed.
    #[test]
    fn the_harness_draws_text() {
        let svg = |body: &str| {
            format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64">{body}</svg>"#
            )
        };
        let label = super::svg(svg("<text y='40' font-size='32'>A</text>").as_bytes());
        assert!(label.is_ok(), "{label:?}");
        let pattern = super::svg(
            svg(
                "<pattern id='p' width='1e4' height='1e4' patternUnits='userSpaceOnUse'>\
                 <rect width='1' height='1'/></pattern>\
                 <text y='8' font-size='8' fill='url(#p)' transform='scale(100)'>A</text>",
            )
            .as_bytes(),
        );
        assert!(
            pattern.as_ref().is_err_and(|err| err.contains("expensive")),
            "{pattern:?}"
        );
    }

    /// The input behind U20 (by `baseFrequency`) still panics in debug
    /// builds with the message `is_known_upstream_debug_panic` excuses. When
    /// upstream fixes it, this fails: drop the excuse. U24's input is
    /// refused before tiny-skia draws it, so its panic is never excused.
    #[test]
    fn known_upstream_debug_panics_still_panic_as_excused() {
        let svg = |body: &str| {
            format!("<svg xmlns='http://www.w3.org/2000/svg' width='64' height='64'>{body}</svg>")
        };
        // One pair: enough to pass the extent, under the cost estimate (which
        // counts the rows each edge walks, bug_0230).
        let arcs = "A1e9 1e9 0 1 1 1e9 0 A1e9 1e9 0 1 1 0 0 ";
        let u24 = svg(&format!("<path d='M0 0 {arcs}'/>"));
        let err = super::svg(u24.as_bytes()).expect_err("U24");
        assert!(err.contains("too far outside the image"), "U24: {err}");
        let u20 = svg(
            "<filter id='f'><feTurbulence baseFrequency='1e20'/></filter><rect width='64' height='64' filter='url(#f)'/>",
        );
        let outcome = super::svg(u20.as_bytes());
        if cfg!(debug_assertions) {
            let err = outcome.expect_err("U20");
            assert!(err.contains("with overflow"), "U20: {err}");
            let panic = err.trim_start_matches("the SVG renderer panicked: ");
            assert!(
                super::is_known_upstream_debug_panic(
                    "/r/resvg-0.49.0/src/filter/turbulence.rs",
                    panic
                ),
                "U20: {panic}"
            );
        }
        // U45 (bug_0238): an infinite hue. The harness's guard turns the
        // panic into an error; the hook excuses exactly that assertion.
        let hue = super::cascade("p { color: hsl(1e39 50% 50%) }");
        if cfg!(debug_assertions) {
            let err = hue.expect_err("U45");
            let panic = err.trim_start_matches("the CSS parser panicked: ");
            assert!(
                super::is_known_upstream_debug_panic("/r/cssparser-color-0.5.0/lib.rs", panic),
                "U45: {err}"
            );
        }
        assert!(!super::is_known_upstream_debug_panic(
            "/r/cssparser-color-0.6.0/lib.rs",
            "assertion failed: (0.0..=1.0).contains(&hue)"
        ));
        // Anything else still aborts the fuzz target.
        assert!(!super::is_known_upstream_debug_panic(
            "/r/tiny-skia-0.12.0/src/scan/path.rs",
            "index out of bounds"
        ));
        assert!(!super::is_known_upstream_debug_panic(
            "/r/usvg-0.49.0/src/parser/filter.rs",
            "called `Option::unwrap()` on a `None` value"
        ));
        assert!(!super::is_known_upstream_debug_panic(
            "/r/tiny-skia-0.12.0/src/scan/path.rs",
            "assertion failed: edges[curr_idx].last_y >= curr_y as i32"
        ));
        assert!(!super::is_known_upstream_debug_panic(
            "/r/resvg-0.50.0/src/filter/turbulence.rs",
            "attempt to add with overflow"
        ));
    }
}
