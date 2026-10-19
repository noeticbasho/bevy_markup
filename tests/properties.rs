//! Property-based tests over the same headless pipeline as the vectors:
//! relationships that must hold for *any* generated input (metamorphic
//! properties), not just fixed examples.
//!
//! Written with `test_strategy`'s `#[proptest]` (strategies live on the
//! parameters, so the bodies stay rustfmt-able).
//!
//! Like the vectors, these check components, not layout: the dump is the
//! observation, `TestUi` (see `common`) is the harness.

mod common;
use common::TestUi;

use bevy::prelude::*;
use bevy_markup::Skeleton;
use bevy_markup::prelude::*;
use proptest::strategy::Strategy;
use proptest::{prop_assert, prop_assert_eq};
use test_strategy::proptest;

/// Regex strategy for CSS color hex digits.
fn hex6() -> proptest::strategy::BoxedStrategy<String> {
    proptest::string::string_regex("[0-9a-f]{6}")
        .unwrap()
        .boxed()
}

/// Regex strategy for an unmatched class/id name.
fn name() -> proptest::strategy::BoxedStrategy<String> {
    proptest::string::string_regex("[a-z][a-z0-9]{0,7}")
        .unwrap()
        .boxed()
}

/// A generated stylesheet over a fixed set of selectors and declarations,
/// including ones that change structure (box properties need wrappers) and
/// layout.
fn generated_sheet() -> proptest::strategy::BoxedStrategy<String> {
    const SELECTORS: [&str; 6] = ["html", "p", ".box", "b", "li", "pre"];
    const DECLARATIONS: [&str; 10] = [
        "color: #ff0000",
        "color: #00ff00",
        "font-size: 30px",
        "font-family: Spectral",
        "font-weight: bold",
        "padding: 3px",
        "background-color: #102030",
        "border-width: 2px",
        "display: none",
        "flex-direction: row",
    ];
    proptest::collection::vec((0..SELECTORS.len(), 0..DECLARATIONS.len()), 0..6)
        .prop_map(|rules| {
            rules
                .into_iter()
                .map(|(selector, declaration)| {
                    format!(
                        "{} {{ {} }}\n",
                        SELECTORS[selector], DECLARATIONS[declaration]
                    )
                })
                .collect()
        })
        .boxed()
}

/// Restyling never disagrees with building: swapping from any stylesheet A
/// to B gives exactly the world a fresh build with B gives (whether the
/// swap restyled in place or had to rebuild for a new structure).
#[proptest(cases = 24)]
fn restyle_matches_a_fresh_build(
    #[strategy(generated_sheet())] a: String,
    #[strategy(generated_sheet())] b: String,
) {
    let page = r#"<p>Plain <b>bold</b></p><div class="box"><p>Inside</p></div><ul><li>Item</li></ul><pre>pre</pre>"#;
    let mut swapped = TestUi::new(
        "prop-restyle",
        &[("page.html", page), ("a.css", &a), ("b.css", &b)],
    )
    .stylesheet("a.css");
    let b_sheet = swapped.load::<Stylesheet>("b.css");
    let mut swapped = swapped.spawn("page.html", TemplateContext::new(), Node::default());
    swapped.settle();
    swapped.world_mut().resource_mut::<DefaultStylesheet>().0 = Some(b_sheet);
    let mut fresh = TestUi::new("prop-fresh", &[("page.html", page), ("b.css", &b)])
        .stylesheet("b.css")
        .spawn("page.html", TemplateContext::new(), Node::default());
    prop_assert_eq!(swapped.settle().dump(), fresh.settle().dump());
}

/// Fragments for [`content_update_matches_a_fresh_build`]: keyed and
/// unkeyed elements, an `id` reused with another tag, nested containers,
/// rich text, signals and inline styles.
const FRAGMENTS: &[&str] = &[
    r#"<p id="a">A</p>"#,
    r#"<p id="a">A changed</p>"#,
    r#"<section id="a"><p>A as a section</p></section>"#,
    "<p>loose</p>",
    "<p>Plain <b>bold</b> tail</p>",
    r#"<div id="d"><p>one</p><p>two</p></div>"#,
    r#"<div id="d"><p>two</p></div>"#,
    r#"<div class="box"><p>boxed</p></div>"#,
    "<ul><li>i</li><li>j</li></ul>",
    r#"<div id="btn" data-on-click="go" data-with='{"n": 1}'><p>Go</p></div>"#,
    r#"<p style="color: #ff0000; opacity: 0.5">styled</p>"#,
];

/// Updating the content in place never disagrees with building: rendering
/// document A and then B gives exactly the world a fresh build of B gives,
/// whatever was kept, replaced, inserted or removed on the way.
#[proptest(cases = 48)]
fn content_update_matches_a_fresh_build(
    #[strategy(proptest::collection::vec(0..FRAGMENTS.len(), 0..6))] a: Vec<usize>,
    #[strategy(proptest::collection::vec(0..FRAGMENTS.len(), 0..6))] b: Vec<usize>,
) {
    let doc = |picks: &[usize]| picks.iter().map(|&i| FRAGMENTS[i]).collect::<String>();
    let page = "{{ doc | safe }}";
    let css = "html { color: #ffffff } .box { padding: 4px; background-color: #102030 } \
               b { color: #00ff00 } #a { color: #0000ff }";
    let mut updated = TestUi::new("prop-update", &[("page.html", page), ("style.css", css)])
        .stylesheet("style.css")
        .spawn(
            "page.html",
            TemplateContext::new().with("doc", &doc(&a)),
            Node::default(),
        );
    updated.settle();
    let root = updated.root();
    updated
        .world_mut()
        .get_mut::<TemplateContext>(root)
        .unwrap()
        .insert("doc", &doc(&b));
    let mut fresh = TestUi::new(
        "prop-update-fresh",
        &[("page.html", page), ("style.css", css)],
    )
    .stylesheet("style.css")
    .spawn(
        "page.html",
        TemplateContext::new().with("doc", &doc(&b)),
        Node::default(),
    );
    let expected = fresh.settle().dump();
    // An identical render builds nothing, so only settle on a change.
    let actual = if doc(&a) == doc(&b) {
        updated.settle_quiet().dump()
    } else {
        updated.settle().dump()
    };
    prop_assert_eq!(actual, expected);
}

/// Fragments for [`one_insertion_or_removal_matches_a_fresh_build`]:
/// repeated rows (the head/tail alignment must not pair a moved row with
/// its twin wrongly), list items (an `ol`'s markers count positions),
/// rows with ids and a nested list.
const ROWS: &[&str] = &[
    "<p>row</p>",
    "<p>row</p>",
    r#"<p class="hot">hot</p>"#,
    r#"<p id="k">keyed</p>"#,
    "<li>item</li>",
    "<li>item two</li>",
    r#"<div class="box"><p>boxed</p></div>"#,
];

/// bug_0168's alignment: document B is document A with one fragment
/// inserted or removed — at the top level, inside a `section` or inside
/// an `ol` — and the updated world equals a fresh build of B, under a
/// sheet whose matches depend on siblings and positions (`+`, `~`,
/// `:first-child`, `:nth-child`) or a plain one, and with `ol` numbering.
#[proptest(cases = 64)]
fn one_insertion_or_removal_matches_a_fresh_build(
    #[strategy(proptest::collection::vec(0..ROWS.len(), 1..8))] rows: Vec<usize>,
    #[strategy(0usize..8)] at: usize,
    #[strategy(0..ROWS.len())] inserted: usize,
    #[strategy(0usize..3)] wrapper: usize,
    remove: bool,
    positional: bool,
) {
    let mut changed = rows.clone();
    let at = at % (rows.len() + usize::from(!remove));
    if remove {
        changed.remove(at.min(rows.len() - 1));
    } else {
        changed.insert(at, inserted);
    }
    let doc = |picks: &[usize]| {
        let body: String = picks.iter().map(|&i| ROWS[i]).collect();
        match wrapper {
            0 => format!("<p>first</p>{body}<p>last</p>"),
            1 => format!("<p>first</p><section>{body}</section><p>last</p>"),
            _ => format!("<p>first</p><ol>{body}</ol><p>last</p>"),
        }
    };
    let page = "{{ doc | safe }}";
    // A positional sheet un-cleans every child of a changed list anyway;
    // a plain one leaves only the `ol` numbering to catch a moved `li`.
    let css = if positional {
        "html { color: #ffffff } .hot + p { color: #ff0000 } .hot ~ li { color: #00ff00 } \
         p:first-child { font-weight: bold } li:nth-child(2) { color: #0000ff } \
         .box { padding: 4px; background-color: #102030 }"
    } else {
        "html { color: #ffffff } .hot { color: #ff0000 } .box { padding: 4px; background-color: #102030 }"
    };
    let mut updated = TestUi::new("prop-align", &[("page.html", page), ("style.css", css)])
        .stylesheet("style.css")
        .spawn(
            "page.html",
            TemplateContext::new().with("doc", &doc(&rows)),
            Node::default(),
        );
    updated.settle();
    let root = updated.root();
    updated
        .world_mut()
        .get_mut::<TemplateContext>(root)
        .unwrap()
        .insert("doc", &doc(&changed));
    let mut fresh = TestUi::new(
        "prop-align-fresh",
        &[("page.html", page), ("style.css", css)],
    )
    .stylesheet("style.css")
    .spawn(
        "page.html",
        TemplateContext::new().with("doc", &doc(&changed)),
        Node::default(),
    );
    let expected = fresh.settle().dump();
    let actual = if doc(&rows) == doc(&changed) {
        updated.settle_quiet().dump()
    } else {
        updated.settle().dump()
    };
    prop_assert_eq!(actual, expected);
}

/// `padding: T R` builds the same world as the four longhands.
#[proptest(cases = 24)]
fn padding_shorthand_equals_longhands(#[strategy(0u8..40)] t: u8, #[strategy(0u8..40)] r: u8) {
    let page = "<p>Text</p>";
    let base = "html { color: #ffffff; font-size: 20px } p { ";
    let shorthand = format!("{base}padding: {t}px {r}px }}");
    let longhands = format!(
        "{base}padding-top: {t}px; padding-right: {r}px; \
         padding-bottom: {t}px; padding-left: {r}px }}"
    );
    let mut a = TestUi::new(
        "prop-pad-shorthand",
        &[("page.html", page), ("style.css", &shorthand)],
    )
    .stylesheet("style.css")
    .spawn("page.html", TemplateContext::new(), Node::default());
    let mut b = TestUi::new(
        "prop-pad-longhands",
        &[("page.html", page), ("style.css", &longhands)],
    )
    .stylesheet("style.css")
    .spawn("page.html", TemplateContext::new(), Node::default());
    prop_assert_eq!(a.settle().dump(), b.settle().dump());
}

/// `border-image: …` (shorthand) draws the same frame as its
/// `-source`/`-slice`/`-repeat` longhands.
#[proptest(cases = 24)]
fn border_image_shorthand_equals_longhands(#[strategy(1u8..12)] slice: u8, #[any] tile: bool) {
    let page = "<p>Text</p>";
    let repeat = if tile { "repeat" } else { "stretch" };
    let shorthand = format!(
        r#"html {{ color: #ffffff; font-size: 20px }} p {{ border-image: url("frame.png") {slice} fill {repeat} }}"#
    );
    let longhands = format!(
        r#"html {{ color: #ffffff; font-size: 20px }} p {{
            border-image-source: url("frame.png");
            border-image-slice: {slice} fill;
            border-image-repeat: {repeat};
        }}"#
    );
    let mut a = TestUi::new(
        "prop-bi-shorthand",
        &[("page.html", page), ("style.css", &shorthand)],
    )
    .stylesheet("style.css")
    .spawn("page.html", TemplateContext::new(), Node::default());
    let mut b = TestUi::new(
        "prop-bi-longhands",
        &[("page.html", page), ("style.css", &longhands)],
    )
    .stylesheet("style.css")
    .spawn("page.html", TemplateContext::new(), Node::default());
    prop_assert_eq!(a.settle().dump(), b.settle().dump());
}

/// Rules that match nothing change nothing, no matter what they declare —
/// even with `!important`.
#[proptest(cases = 24)]
fn unmatched_rules_change_nothing(
    #[strategy(proptest::collection::vec(name(), 1..4))] ids: Vec<String>,
    #[strategy(proptest::collection::vec(name(), 1..4))] classes: Vec<String>,
    #[strategy(proptest::collection::vec(8u8..40, 4))] sizes: Vec<u8>,
) {
    let page = "<p class=\"note\">Text</p><div><p>Inner</p></div>";
    let base = "html { color: #ffffff; font-size: 20px } .note { color: #00ff00 }";
    // Every generated selector names an id or class no element carries.
    let mut junk = String::new();
    for ((id, class), size) in ids.iter().zip(&classes).zip(&sizes) {
        junk.push_str(&format!(
            "\n#{id} {{ font-size: {size}px }}\n.{class} {{ color: #123456 !important }}\n"
        ));
    }
    let (with_junk, without) = (format!("{base}{junk}"), base.to_owned());
    let mut a = TestUi::new(
        "prop-unmatched-junk",
        &[("page.html", page), ("style.css", &with_junk)],
    )
    .stylesheet("style.css")
    .spawn("page.html", TemplateContext::new(), Node::default());
    let mut b = TestUi::new(
        "prop-unmatched-base",
        &[("page.html", page), ("style.css", &without)],
    )
    .stylesheet("style.css")
    .spawn("page.html", TemplateContext::new(), Node::default());
    prop_assert_eq!(a.settle().dump(), b.settle().dump());
}

/// Specificity monotonicity: a class rule always beats a type rule, in
/// either source order.
#[proptest(cases = 24)]
fn class_beats_type_selector_regardless_of_order(
    #[strategy(hex6())] type_color: String,
    #[strategy(hex6())]
    #[filter(#type_color != #class_color)]
    class_color: String,
) {
    let page = "<p class=\"note\">Text</p>";
    let head = "html { color: #ffffff; font-size: 20px } ";
    let type_rule = format!("p {{ color: #{type_color} }} ");
    let class_rule = format!(".note {{ color: #{class_color} }} ");
    let (type_first, class_first) = (
        format!("{head}{type_rule}{class_rule}"),
        format!("{head}{class_rule}{type_rule}"),
    );
    let mut a = TestUi::new(
        "prop-spec-type-first",
        &[("page.html", page), ("style.css", &type_first)],
    )
    .stylesheet("style.css")
    .spawn("page.html", TemplateContext::new(), Node::default());
    let mut b = TestUi::new(
        "prop-spec-class-first",
        &[("page.html", page), ("style.css", &class_first)],
    )
    .stylesheet("style.css")
    .spawn("page.html", TemplateContext::new(), Node::default());
    let dump = a.settle().dump();
    prop_assert!(
        dump.contains(&format!("#{class_color}")),
        "class rule must win:\n{dump}"
    );
    prop_assert_eq!(dump, b.settle().dump());
}

/// Idempotence: re-setting the same context value leaves the subtree as it
/// was — no rebuild at all (the render is identical), same dump.
#[proptest(cases = 24)]
fn rebuild_with_same_context_is_idempotent(#[strategy(0i64..1000)] n: i64) {
    let mut ui = TestUi::new(
        "prop-idempotent",
        &[
            ("page.html", "<p>{{ n }}</p>"),
            ("style.css", "html { color: #ffffff; font-size: 20px }"),
        ],
    )
    .stylesheet("style.css")
    .spawn(
        "page.html",
        TemplateContext::new().with("n", &n),
        Node::default(),
    );
    let before = ui.settle().dump();

    let root = ui.root();
    ui.world_mut()
        .get_mut::<TemplateContext>(root)
        .unwrap()
        .insert("n", &n); // same value; only the mutation is detected
    let builds = ui.builds();
    let after = ui.settle_quiet().dump();
    prop_assert_eq!(ui.builds(), builds, "an identical render must not rebuild");
    prop_assert_eq!(before, after);
}

/// Round trip: theme X → Y → X ends where it started, including the
/// `html` rule's box properties (dropped and re-applied).
#[proptest(cases = 24)]
fn theme_round_trip(#[strategy(0u8..20)] padding: u8) {
    let swap_css = format!("html {{ color: #ffffff; font-size: 20px; padding: {padding}px }}");
    let files = [
        ("page.html", "<p>Text</p>"),
        (
            "framed.css",
            r#"html { color: #ffffff; font-size: 20px; border-image: url("frame.png") 4 fill stretch; border-width: 16px; padding: 10px }"#,
        ),
        (
            "plain.css",
            "html { color: #00ff00; font-size: 12px; padding: 0px }",
        ),
        ("swap.css", swap_css.as_str()),
    ];
    let mut ui = TestUi::new("prop-theme-round-trip", &files)
        .stylesheet("framed.css")
        .spawn("page.html", TemplateContext::new(), Node::default());
    let start = ui.settle().dump();

    let server = ui.world_mut().resource::<AssetServer>().clone();
    let plain: Handle<Stylesheet> = server.load("plain.css");
    ui.world_mut()
        .insert_resource(DefaultStylesheet::new(plain));
    ui.settle();
    let framed: Handle<Stylesheet> = server.load("framed.css");
    ui.world_mut()
        .insert_resource(DefaultStylesheet::new(framed));
    prop_assert_eq!(start, ui.settle().dump());
}

/// Round trip: locale A → B → A and context v → w → v end where they
/// started.
#[proptest(cases = 24)]
fn locale_and_context_round_trip(
    #[strategy(0i64..100)] v: i64,
    #[strategy(0i64..100)]
    #[filter(#v != #w)]
    w: i64,
) {
    let mut ui = TestUi::new(
        "prop-locale-round-trip",
        &[
            (
                "page.html",
                r#"<p data-l10n-id="count" data-l10n-args='{"n": {{ n }}}'>{{ n }}</p>"#,
            ),
            ("style.css", "html { color: #ffffff; font-size: 20px }"),
            (
                "locales/en-US/main.ftl.ron",
                r#"(locale: "en-US", resources: ["ui.ftl"])"#,
            ),
            ("locales/en-US/ui.ftl", "count = { $n } items"),
            (
                "locales/de/main.ftl.ron",
                r#"(locale: "de", resources: ["ui.ftl"])"#,
            ),
            ("locales/de/ui.ftl", "count = { $n } Dinge"),
        ],
    )
    .stylesheet("style.css")
    .locale("locales/en-US/main.ftl.ron")
    .spawn(
        "page.html",
        TemplateContext::new().with("n", &v),
        Node::default(),
    );
    let start = ui.settle().dump();

    let server = ui.world_mut().resource::<AssetServer>().clone();
    let de: Handle<LocaleBundle> = server.load("locales/de/main.ftl.ron");
    ui.world_mut().insert_resource(ActiveLocale::new(de));
    let root = ui.root();
    ui.world_mut()
        .get_mut::<TemplateContext>(root)
        .unwrap()
        .insert("n", &w);
    ui.settle();

    let en: Handle<LocaleBundle> = server.load("locales/en-US/main.ftl.ron");
    ui.world_mut().insert_resource(ActiveLocale::new(en));
    ui.world_mut()
        .get_mut::<TemplateContext>(root)
        .unwrap()
        .insert("n", &v);
    prop_assert_eq!(start, ui.settle().dump());
}

/// Selectors for the generated stylesheets below: each matches some
/// element of [`LIST_PAGE`] or nothing (`#ghost`), and `div p` is a valid
/// but unsupported (combinator) selector.
const SELECTORS: &[&str] = &[
    "p", ".a", "#x", "p.a", "*", "div", ".b", "p#x.a", "#ghost", "div p",
];
const LIST_PAGE: &str = r#"<p id="x" class="a">One</p><p class="b">Two <b class="a">bold</b></p><div class="a"><p>Three</p></div>"#;

/// A rule: selector list, color, `!important`.
type ListRule = (Vec<&'static str>, String, bool);

fn list_rules() -> impl Strategy<Value = Vec<ListRule>> {
    let selectors = proptest::collection::vec(proptest::sample::select(SELECTORS), 1..4);
    proptest::collection::vec((selectors, hex6(), proptest::bool::weighted(0.2)), 1..6)
}

fn declaration(color: &str, important: bool) -> String {
    format!(
        "color: #{color}{}",
        if important { " !important" } else { "" }
    )
}

/// Metamorphic CSS identities over generated stylesheets, all of which
/// must build the same world:
/// - a selector list `a, b { X }` ≡ `a { X } b { X }` in its place (each
///   selector matches with its own specificity — catches a list taking its
///   first or highest selector's specificity, or dropping the whole list
///   when one selector is unsupported);
/// - comments, line breaks and spacing between tokens are insignificant,
///   and repeating the whole sheet changes nothing (catches source-order
///   or cache bookkeeping that depends on rule count or formatting).
#[proptest(cases = 16)]
fn selector_lists_formatting_and_duplicates_change_nothing(
    #[strategy(list_rules())] rules: Vec<ListRule>,
) {
    let base = "html { color: #ffffff; font-size: 20px }\n";
    let mut lists = base.to_owned();
    let mut expanded = base.to_owned();
    let mut noisy = String::new();
    for (selectors, color, important) in &rules {
        let declaration = declaration(color, *important);
        lists.push_str(&format!("{} {{ {declaration} }}\n", selectors.join(",")));
        for selector in selectors {
            expanded.push_str(&format!("{selector} {{ {declaration} }}\n"));
        }
        noisy.push_str(&format!(
            "/* rule */\n{}\n/* before block */{{\n\t{} /* end */ ;\n}}\n\n",
            selectors.join(" ,\n  "),
            declaration.replace(": ", " :\n   ")
        ));
    }
    let noisy = format!("/* top */ {base}{noisy}{noisy}");
    let mut dumps = Vec::new();
    for (name, css) in [
        ("prop-lists", &lists),
        ("prop-expanded", &expanded),
        ("prop-noisy", &noisy),
    ] {
        let mut ui = TestUi::new(name, &[("page.html", LIST_PAGE), ("style.css", css)])
            .stylesheet("style.css")
            .spawn("page.html", TemplateContext::new(), Node::default());
        dumps.push(ui.settle().dump());
    }
    prop_assert_eq!(
        &dumps[0],
        &dumps[1],
        "selector list vs expanded:\n{}\n---\n{}",
        lists,
        expanded
    );
    prop_assert_eq!(
        &dumps[0],
        &dumps[2],
        "compact vs noisy+duplicated:\n{}\n---\n{}",
        lists,
        noisy
    );
}

/// Every sRGB notation of one color lands on the same color: `#rrggbb`,
/// upper-case hex, `#rrggbbaa` with opaque alpha, `rgb()` in comma and
/// space syntax, `rgba(…, 1)`, `#rgb` when the channels allow it. The
/// expected hex is computed from the channels, so a channel swap or a
/// dropped notation shows up as a wrong or inherited (white) color.
#[proptest(cases = 16)]
fn color_notations_agree(
    #[strategy(proptest::array::uniform3(0u8..=255))] rgb: [u8; 3],
    #[strategy(proptest::array::uniform3(0u8..16))] short: [u8; 3],
) {
    let [r, g, b] = rgb;
    let notations = [
        format!("#{r:02x}{g:02x}{b:02x}"),
        format!("#{r:02X}{g:02X}{b:02X}"),
        format!("#{r:02x}{g:02x}{b:02x}ff"),
        format!("rgb({r}, {g}, {b})"),
        format!("rgb({r} {g} {b})"),
        format!("rgba({r}, {g}, {b}, 1)"),
    ];
    // `#rgb` doubles each digit: channel = digit × 17.
    let [sr, sg, sb] = short;
    let short_hex = format!("#{sr:x}{sg:x}{sb:x}");
    let short_expected = format!("#{:02x}{:02x}{:02x}", sr * 17, sg * 17, sb * 17);

    let mut page = String::new();
    // Not white, so an ignored declaration (inherited white) can't pass.
    let mut css = "html { color: #010203; font-size: 20px }\n".to_owned();
    for (i, notation) in notations.iter().chain([&short_hex]).enumerate() {
        page.push_str(&format!("<p class=\"c{i}\">c{i}</p>"));
        css.push_str(&format!(".c{i} {{ color: {notation} }}\n"));
    }
    let mut ui = TestUi::new("prop-colors", &[("page.html", &page), ("style.css", &css)])
        .stylesheet("style.css")
        .spawn("page.html", TemplateContext::new(), Node::default());
    let dump = ui.settle().dump();
    let expected = format!("#{r:02x}{g:02x}{b:02x}");
    for (i, notation) in notations.iter().enumerate() {
        let run = format!("\"c{i}\" default 20px {expected}");
        prop_assert!(
            dump.contains(&run),
            "{notation} should be {expected}:\n{dump}"
        );
    }
    let run = format!("\"c{}\" default 20px {short_expected}", notations.len());
    prop_assert!(
        dump.contains(&run),
        "{short_hex} should be {short_expected}:\n{dump}"
    );
}

/// CSS named colors are the sRGB values the spec lists (checked against a
/// hand-copied table), including mixed case.
#[proptest(cases = 4)]
fn named_colors_match_the_spec(#[any] upper: bool) {
    const NAMED: &[(&str, &str)] = &[
        ("red", "#ff0000"),
        ("Lime", "#00ff00"),
        ("navy", "#000080"),
        ("teal", "#008080"),
        ("rebeccapurple", "#663399"),
        ("GoldenRod", "#daa520"),
        ("gray", "#808080"),
        ("grey", "#808080"),
    ];
    let mut page = String::new();
    let mut css = "html { color: #010203; font-size: 20px }\n".to_owned();
    for (i, (name, _)) in NAMED.iter().enumerate() {
        let name = if upper {
            name.to_ascii_uppercase()
        } else {
            (*name).to_owned()
        };
        page.push_str(&format!("<p class=\"c{i}\">c{i}</p>"));
        css.push_str(&format!(".c{i} {{ color: {name} }}\n"));
    }
    let mut ui = TestUi::new("prop-named", &[("page.html", &page), ("style.css", &css)])
        .stylesheet("style.css")
        .spawn("page.html", TemplateContext::new(), Node::default());
    let dump = ui.settle().dump();
    for (i, (name, hex)) in NAMED.iter().enumerate() {
        prop_assert!(
            dump.contains(&format!("\"c{i}\" default 20px {hex}")),
            "{name} → {hex}:\n{dump}"
        );
    }
}

/// Relative font sizes resolve against the right base: `Nem` and `N×100%`
/// against the parent's size, `Nrem` against the root's, nested `em` on
/// an inline element against the enclosing block's computed size. Bases
/// and factors are multiples of 1/4, so every expected size is exact.
/// Catches `rem` resolved against the parent (or `em` against the root),
/// `%` not divided by 100, and inline `em` compounding twice.
#[proptest(cases = 16)]
fn relative_font_sizes_resolve_against_their_base(
    #[strategy(8u16..40)] root: u16,
    #[strategy(8u16..40)] parent: u16,
    #[strategy(1u16..16)] quarters: u16,
) {
    let k = f32::from(quarters) / 4.0;
    let percent = u32::from(quarters) * 25;
    let css = format!(
        "html {{ color: #ffffff; font-size: {root}px }}\n\
         div {{ font-size: {parent}px }}\n\
         .em {{ font-size: {k}em }}\n\
         .pct {{ font-size: {percent}% }}\n\
         .rem {{ font-size: {k}rem }}\n\
         .half {{ font-size: 0.5em }}\n"
    );
    let page = r#"<div><p class="em">em <b class="half">half</b></p><p class="pct">pct</p><p class="rem">rem</p><p>inherit</p></div>"#;
    let mut ui = TestUi::new(
        "prop-font-size",
        &[("page.html", page), ("style.css", &css)],
    )
    .stylesheet("style.css")
    .spawn("page.html", TemplateContext::new(), Node::default());
    let dump = ui.settle().dump();
    let (root, parent) = (f32::from(root), f32::from(parent));
    for (text, size) in [
        ("em ", parent * k),
        ("half", parent * k * 0.5),
        ("pct", parent * k),
        ("rem", root * k),
        ("inherit", parent),
    ] {
        let run = format!("{text:?} default {size}px #ffffff");
        prop_assert!(dump.contains(&run), "missing {run:?}\n{css}\n{dump}");
    }
}

/// A `*.slice.ron` manifest reaches the frame node side by side: `left`
/// and `top` are the min insets, `right` and `bottom` the max insets,
/// `sides`/`center` the scale modes, drawn over the border box. Catches
/// swapped or mirrored sides (invisible for symmetric frames).
#[proptest(cases = 8)]
fn nine_slice_manifest_maps_each_side(
    #[strategy(proptest::array::uniform4(0u8..12))] sides: [u8; 4],
    #[any] tile_sides: bool,
    #[any] tile_center: bool,
) {
    let [left, right, top, bottom] = sides;
    let mode = |tile: bool| if tile { "Tile(2.0)" } else { "Stretch" };
    let manifest = format!(
        r#"(image: "frame.png", border: (left: {left}, right: {right}, top: {top}, bottom: {bottom}),
            sides: {}, center: {})"#,
        mode(tile_sides),
        mode(tile_center)
    );
    let mut ui = TestUi::new(
        "prop-nine-slice",
        &[("page.html", "<p>x</p>"), ("frame.slice.ron", &manifest)],
    )
    .spawn("page.html", TemplateContext::new(), Node::default());
    let slice: Handle<NineSlice> = ui.load("frame.slice.ron");
    let frame = ui.world_mut().spawn(NineSliceFrame(slice)).id();
    ui.settle();

    let image = ui
        .world_mut()
        .get::<ImageNode>(frame)
        .expect("frame got an ImageNode");
    prop_assert_eq!(image.visual_box, VisualBox::BorderBox);
    prop_assert_eq!(
        image
            .image
            .path()
            .map(|path| path.path().display().to_string()),
        Some("frame.png".to_owned())
    );
    let NodeImageMode::Sliced(slicer) = &image.image_mode else {
        panic!("not sliced: {:?}", image.image_mode);
    };
    let border = slicer.border;
    prop_assert_eq!(
        [
            border.min_inset.x,
            border.max_inset.x,
            border.min_inset.y,
            border.max_inset.y
        ],
        sides.map(f32::from)
    );
    let tiles = |mode: &SliceScaleMode| matches!(mode, SliceScaleMode::Tile { stretch_value } if *stretch_value == 2.0);
    prop_assert_eq!(tiles(&slicer.sides_scale_mode), tile_sides);
    prop_assert_eq!(tiles(&slicer.center_scale_mode), tile_center);
}

/// ADR 0021: a build that reused its clean skeleton equals a build without
/// one. Two long-lived UIs get the same op sequence; the second drops its
/// skeleton before every change, so every build there is fresh. The ops
/// cover the shapes reuse got wrong: walk-through-tag siblings of dynamic
/// content, stylesheet swaps in the same frame as a content change, the
/// debug outline freezing the skeleton, all-dirty changes that trip
/// the adaptive gate and then retry it, and hover (a restyle that must
/// hand the reused subtree's `DomNode` handles on).
///
/// Inputs are only touched when the op changes them: every insert
/// (even of the same value) is a change signal, and a style-side change
/// turns reuse off — re-inserting the stylesheet every op would make the
/// property compare two fresh builds, exercising no reuse at all.
#[proptest(cases = 16)]
fn skeleton_reuse_matches_a_fresh_build(
    #[strategy(proptest::collection::vec(0..7u8, 1..24))] ops: Vec<u8>,
) {
    let page = r#"
<h1>Report {{ a }}</h1>
<div class="panel">
  <p class="static">Unchanged prose.</p>
  <span class="tag">Static tag</span>
  <b>Static bold</b>
  <p class="dynamic">Value: {{ b }}</p>
  <p>See <a href="/p/{{ a }}">the page</a> now.</p>
</div>
{% for i in range(end=2) %}
<p class="row">Row {{ i }}: {{ b }}</p>
{% endfor %}
"#;
    // The `:hover` rule makes a hover op a real restyle (pseudo-only
    // compounds match any hovered element): without it a hover op changes
    // nothing buildable, and `settle` (which demands a new build) hangs.
    let sheet_a = r#"
html { color: #ffffff; font-size: 20px; flex-direction: column }
.panel { padding: 8px }
:hover { color: #ff8888 }
"#;
    let sheet_b = r#"
html { color: #ffff88; font-size: 24px; flex-direction: column }
.panel { padding: 3px }
:hover { color: #88ff88 }
"#;
    let files = [("page.html", page), ("a.css", sheet_a), ("b.css", sheet_b)];
    let mut reused = TestUi::with_pointer("prop-skeleton-reused", &files, UVec2::new(640, 480))
        .stylesheet("a.css")
        .spawn(
            "page.html",
            TemplateContext::new().with("a", &1).with("b", &"x"),
            Node::default(),
        );
    let mut fresh = TestUi::with_pointer("prop-skeleton-fresh", &files, UVec2::new(640, 480))
        .stylesheet("a.css")
        .spawn(
            "page.html",
            TemplateContext::new().with("a", &1).with("b", &"x"),
            Node::default(),
        );
    reused.settle();
    fresh.settle();
    prop_assert_eq!(reused.dump(), fresh.dump());

    // Every op changes something (a no-op render rebuilds nothing —
    // covered elsewhere). Ops 2 and 5 swap the stylesheet: 2 alone (a pure
    // restyle), 5 in the same frame as a content change — the theme-button
    // case reuse got wrong. Op 6 toggles the pointer between a hovered
    // element and off-content (the hover restyle, then the restyle back).
    let (mut a, mut b): (u32, &str) = (1, "x");
    let (mut sheet_b, mut outlined, mut hovered) = (false, false, false);
    for op in ops {
        let (prev_a, prev_b) = (a, b);
        let prev_sheet_b = sheet_b;
        let prev_outlined = outlined;
        match op {
            0 => a = if a == 1 { 2 } else { 1 },
            1 => b = if b == "x" { "y" } else { "x" },
            2 => sheet_b = !sheet_b,
            3 => outlined = !outlined,
            4 => {
                a = if a == 1 { 2 } else { 1 };
                b = if b == "x" { "y" } else { "x" };
            }
            5 => {
                sheet_b = !sheet_b;
                a = if a == 1 { 2 } else { 1 };
            }
            _ => {
                hovered = !hovered;
                // Over the heading when hovering; the bottom-right corner
                // is off-content (a leave).
                for ui in [&mut reused, &mut fresh] {
                    ui.move_pointer(if hovered {
                        Vec2::new(20.0, 20.0)
                    } else {
                        Vec2::new(630.0, 470.0)
                    });
                }
            }
        }
        for (ui, keep_skeleton) in [(&mut reused, true), (&mut fresh, false)] {
            let root = ui.root();
            if !keep_skeleton {
                // The comparator rebuilds without a skeleton: the next
                // build there collects everything from scratch.
                ui.world_mut().entity_mut(root).remove::<Skeleton>();
            }
            if sheet_b != prev_sheet_b {
                let handle: Handle<Stylesheet> = if sheet_b {
                    ui.load("b.css")
                } else {
                    ui.load("a.css")
                };
                ui.world_mut()
                    .entity_mut(root)
                    .insert(HtmlStylesheet(handle));
            }
            if outlined != prev_outlined {
                let mut entity = ui.world_mut().entity_mut(root);
                if outlined {
                    entity.insert(HtmlDebugOutline);
                } else {
                    entity.remove::<HtmlDebugOutline>();
                }
            }
            if a != prev_a || b != prev_b {
                let mut entity = ui.world_mut().entity_mut(root);
                if a != prev_a {
                    entity.get_mut::<TemplateContext>().unwrap().insert("a", &a);
                }
                if b != prev_b {
                    entity.get_mut::<TemplateContext>().unwrap().insert("b", &b);
                }
            }
        }
        // A pointer-only op can change nothing buildable (hover over the
        // outline's anonymous text block matches no rule): the rebuild is
        // allowed, not required.
        if op == 6 {
            reused.settle_quiet();
            fresh.settle_quiet();
        } else {
            reused.settle();
            fresh.settle();
        }
        prop_assert_eq!(reused.dump(), fresh.dump());
    }
}
