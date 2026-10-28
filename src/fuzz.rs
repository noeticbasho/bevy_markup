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
    let mut tera = tera::Tera::new();
    tera.add_raw_template(name, source)
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
    let mut tera = tera::Tera::new();
    tera.add_raw_template(md_name, &md_source)
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
    let parsed = StyleSheet::parse(css, ParserOptions::default()).map_err(|err| err.to_string())?;
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
    // resource, so there is no recovery path to fuzz.
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
    /// Fixed in the fluent-syntax fork (root Cargo.toml `[patch]`); this must
    /// never panic.
    #[test]
    fn broken_unicode_escape_after_multibyte_char_does_not_panic() {
        let ftl = "u={\"\\U\u{fffd}";
        let _ = translate(ftl, "", "");
    }

    /// bug_0015: expressions nested far beyond the fork's limit
    /// (`MAX_NESTING_DEPTH`, 100) are a parse error, not a stack overflow,
    /// even on a 2 MB thread, where unbounded debug builds overflowed
    /// between 200 and 400 levels. Both recursion paths: placeables and
    /// call arguments (the first fix covered placeables only).
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

    /// Nesting up to the limit still parses and formats, and the limit
    /// leaves later entries alone (the deep entry becomes Junk).
    #[test]
    fn nesting_up_to_the_limit_still_works() {
        let nested = |depth: usize| format!("{}\"a\"{}", "{".repeat(depth), "}".repeat(depth));
        assert_eq!(
            translate(&format!("x = {}\n", nested(100)), "x", "{}"),
            Ok("a".to_owned())
        );
        let resource =
            fluent::FluentResource::try_new(format!("deep = {}\nok = fine\n", nested(101)));
        let (resource, errors) = resource.expect_err("depth 101 is an error");
        assert_eq!(errors.len(), 1, "{errors:?}");
        let ids: Vec<&str> = resource
            .entries()
            .filter_map(|entry| match entry {
                fluent_syntax::ast::Entry::Message(message) => Some(message.id.name),
                _ => None,
            })
            .collect();
        assert_eq!(ids, ["ok"], "the entry after the deep one survives");
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
