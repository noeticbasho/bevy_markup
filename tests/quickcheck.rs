//! Property tests over structured input (`quickcheck`'s strength): derive
//! the input shape by hand, let quickcheck generate/shrink it.
//!
//! Unlike `properties.rs` (metamorphic, proptest), these check the pipeline
//! against an independent reference model of CSS precedence, and against a
//! round trip of arbitrary text through Tera's autoescaping.
//!
//! Observation is the usual dump; `TestUi` (see `common`) is the harness.

mod common;
use common::TestUi;

use bevy::prelude::*;
use bevy_markup::prelude::*;
use quickcheck::{Arbitrary, Gen};
use quickcheck_macros::quickcheck;

const BASE_CSS: &str = "html { color: #ffffff; font-size: 20px }";

// ---------------------------------------------------------------------------
// Cascade winner vs a reference precedence model
// ---------------------------------------------------------------------------

/// A `<p>` target element: optional `id="the-id"`, classes `c0`… (`n_classes`).
#[derive(Clone, Debug)]
struct Element {
    has_id: bool,
    n_classes: u8,
}

/// One CSS rule that may or may not match the element.
///
/// `ids`: 0 = no id part, 1 = `#the-id` (matches iff the element has the id),
/// 2 = `#ghost` (never matches). Class parts are `.c0`/`.c1`/`.c2`; the
/// element never has `c2`, so those rules never match either.
#[derive(Clone, Debug)]
struct Rule {
    tag: bool,
    ids: u8,
    classes: Vec<u8>,
    important: bool,
    color: [u8; 3],
}

#[derive(Clone, Debug)]
struct CascadeCase {
    element: Element,
    rules: Vec<Rule>,
}

impl Arbitrary for Rule {
    fn arbitrary(g: &mut Gen) -> Self {
        // `Gen`'s RNG is private; build everything from `arbitrary` + `choose`.
        let ids = *g.choose(&[0u8, 1, 2]).unwrap();
        let n_classes = *g.choose(&[0usize, 1, 2]).unwrap();
        Self {
            tag: bool::arbitrary(g),
            ids,
            classes: (0..n_classes)
                .map(|_| *g.choose(&[0u8, 1, 2]).unwrap())
                .collect(),
            important: bool::arbitrary(g),
            color: [u8::arbitrary(g), u8::arbitrary(g), u8::arbitrary(g)],
        }
    }
}

impl Arbitrary for CascadeCase {
    fn arbitrary(g: &mut Gen) -> Self {
        let n_rules = *g.choose(&[1usize, 2, 3, 4, 5, 6]).unwrap();
        Self {
            element: Element {
                has_id: bool::arbitrary(g),
                n_classes: *g.choose(&[0u8, 1, 2]).unwrap(),
            },
            rules: (0..n_rules).map(|_| Rule::arbitrary(g)).collect(),
        }
    }
}

fn rule_matches(rule: &Rule, element: &Element) -> bool {
    let id_ok = match rule.ids {
        0 => true,
        1 => element.has_id,
        _ => false,
    };
    id_ok && rule.classes.iter().all(|&class| class < element.n_classes)
}

/// (ids, classes, type), the CSS specificity tuple.
fn specificity(rule: &Rule) -> (u8, u8, u8) {
    (
        (rule.ids > 0) as u8,
        rule.classes.len() as u8,
        rule.tag as u8,
    )
}

/// The winning color per CSS precedence: importance, then specificity, then
/// source order; no match = inherited `html` color.
fn expected_color(case: &CascadeCase) -> [u8; 3] {
    case.rules
        .iter()
        .enumerate()
        .filter(|(_, rule)| rule_matches(rule, &case.element))
        .max_by_key(|(index, rule)| (rule.important, specificity(rule), *index))
        .map_or([0xff, 0xff, 0xff], |(_, rule)| rule.color)
}

fn rule_css(rule: &Rule) -> String {
    let mut selector = String::new();
    if rule.tag {
        selector.push_str("p");
    } else {
        selector.push('*');
    }
    if rule.ids > 0 {
        selector.push_str(if rule.ids == 1 { "#the-id" } else { "#ghost" });
    }
    for class in &rule.classes {
        selector.push_str(&format!(".c{class}"));
    }
    let bang = if rule.important { " !important" } else { "" };
    format!(
        "{} {{ color: #{:02x}{:02x}{:02x}{} }} ",
        selector, rule.color[0], rule.color[1], rule.color[2], bang
    )
}

/// For any stylesheet of compound rules, the color bevy_markup puts on the target
/// `<p>` is the one CSS precedence picks.
#[quickcheck]
fn cascade_winner_matches_precedence_model(case: CascadeCase) {
    let element = &case.element;
    let id_attr = if element.has_id { " id=\"the-id\"" } else { "" };
    let classes: Vec<String> = (0..element.n_classes).map(|c| format!("c{c}")).collect();
    let page = format!("<p{id_attr} class=\"{}\">Text</p>", classes.join(" "));
    let css = format!(
        "{BASE_CSS} {}",
        case.rules.iter().map(rule_css).collect::<String>()
    );

    let mut ui = TestUi::new("qc-cascade", &[("page.html", &page), ("style.css", &css)])
        .stylesheet("style.css")
        .spawn("page.html", TemplateContext::new(), Node::default());
    let color = expected_color(&case);
    let expected = format!(
        "\"Text\" default 20px #{:02x}{:02x}{:02x}",
        color[0], color[1], color[2]
    );
    let dump = ui.settle().dump();
    assert!(
        dump.contains(&expected),
        "expected run {expected:?} (winner of {case:#?})\nCSS:\n{css}\n--- dump ---\n{dump}"
    );
}

// ---------------------------------------------------------------------------
// Selector lists, per-declaration importance, several elements
// ---------------------------------------------------------------------------

/// One compound selector: `p`/`*`, id part (0 none, 1 `#i0`, 2 `#i1`, 3
/// `#ghost`), class parts from `c0`…`c2`.
#[derive(Clone, Debug)]
struct Selector {
    tag: bool,
    id: u8,
    classes: Vec<u8>,
}

/// A rule with a selector list and two independently cascaded
/// declarations, each optionally `!important`.
#[derive(Clone, Debug)]
struct ListRule {
    selectors: Vec<Selector>,
    color: Option<([u8; 3], bool)>,
    size: Option<(u8, bool)>,
}

/// A `<p>` element: id (0 none, 1 `i0`, 2 `i1`) and a class bit set over
/// `c0`/`c1` (`c2` never occurs).
#[derive(Clone, Debug)]
struct ListElement {
    id: u8,
    classes: u8,
}

#[derive(Clone, Debug)]
struct ListCase {
    elements: Vec<ListElement>,
    rules: Vec<ListRule>,
}

impl Arbitrary for Selector {
    fn arbitrary(g: &mut Gen) -> Self {
        let n_classes = *g.choose(&[0usize, 0, 1, 2]).unwrap();
        Self {
            tag: bool::arbitrary(g),
            id: *g.choose(&[0u8, 0, 1, 2, 3]).unwrap(),
            classes: (0..n_classes)
                .map(|_| *g.choose(&[0u8, 1, 2]).unwrap())
                .collect(),
        }
    }
}

impl Arbitrary for ListRule {
    fn arbitrary(g: &mut Gen) -> Self {
        let n_selectors = *g.choose(&[1usize, 1, 2, 3]).unwrap();
        let color = [u8::arbitrary(g), u8::arbitrary(g), u8::arbitrary(g)];
        let size = *g.choose(&[8u8, 10, 12, 14, 24, 30, 36]).unwrap();
        // At least one declaration, so every rule can matter.
        let (has_color, has_size) = *g
            .choose(&[(true, false), (false, true), (true, true)])
            .unwrap();
        Self {
            selectors: (0..n_selectors).map(|_| Selector::arbitrary(g)).collect(),
            color: has_color.then(|| (color, *g.choose(&[false, false, true]).unwrap())),
            size: has_size.then(|| (size, *g.choose(&[false, false, true]).unwrap())),
        }
    }
}

impl Arbitrary for ListCase {
    fn arbitrary(g: &mut Gen) -> Self {
        let n_rules = *g.choose(&[1usize, 2, 3, 4, 5, 6, 7, 8]).unwrap();
        Self {
            elements: (0..3)
                .map(|_| ListElement {
                    id: *g.choose(&[0u8, 1, 2]).unwrap(),
                    classes: *g.choose(&[0u8, 1, 2, 3]).unwrap(),
                })
                .collect(),
            rules: (0..n_rules).map(|_| ListRule::arbitrary(g)).collect(),
        }
    }
}

fn selector_matches(selector: &Selector, element: &ListElement) -> bool {
    let id_ok = selector.id == 0 || selector.id == element.id;
    id_ok
        && selector
            .classes
            .iter()
            .all(|&class| class < 2 && element.classes & (1 << class) != 0)
}

/// The rule's specificity *for this element*: the most specific of its
/// selectors that match (CSS Selectors 4 §17), `None` if none match.
fn list_specificity(rule: &ListRule, element: &ListElement) -> Option<(u8, u8, u8)> {
    rule.selectors
        .iter()
        .filter(|selector| selector_matches(selector, element))
        .map(|selector| {
            (
                (selector.id > 0) as u8,
                selector.classes.len() as u8,
                selector.tag as u8,
            )
        })
        .max()
}

/// Per property: among matching rules declaring it, the max of
/// (importance, specificity, source order); else the `html` value.
fn cascaded<T: Copy>(
    case: &ListCase,
    element: &ListElement,
    declared: impl Fn(&ListRule) -> Option<(T, bool)>,
    inherited: T,
) -> T {
    case.rules
        .iter()
        .enumerate()
        .filter_map(|(index, rule)| {
            let (value, important) = declared(rule)?;
            Some(((important, list_specificity(rule, element)?, index), value))
        })
        .max_by_key(|(key, _)| *key)
        .map_or(inherited, |(_, value)| value)
}

fn list_rule_css(rule: &ListRule) -> String {
    let selectors: Vec<String> = rule
        .selectors
        .iter()
        .map(|selector| {
            let mut css = if selector.tag {
                "p".to_owned()
            } else {
                "*".to_owned()
            };
            css.push_str(["", "#i0", "#i1", "#ghost"][selector.id as usize]);
            for class in &selector.classes {
                css.push_str(&format!(".c{class}"));
            }
            css
        })
        .collect();
    let bang = |important: bool| if important { " !important" } else { "" };
    let mut declarations = Vec::new();
    if let Some(([r, g, b], important)) = rule.color {
        declarations.push(format!("color: #{r:02x}{g:02x}{b:02x}{}", bang(important)));
    }
    if let Some((size, important)) = rule.size {
        declarations.push(format!("font-size: {size}px{}", bang(important)));
    }
    format!(
        "{} {{ {} }}\n",
        selectors.join(", "),
        declarations.join("; ")
    )
}

/// Several elements under one stylesheet of selector-list rules whose
/// declarations carry their own `!important`: each element's color and
/// size are what CSS precedence picks for that element, per property.
/// Catches a list rule using one specificity for all elements (its first
/// or most specific selector, matching or not), importance applied per
/// rule instead of per declaration, and a style cache keyed too coarsely
/// (elements sharing a tag getting each other's styles).
#[quickcheck]
fn selector_lists_cascade_per_element_and_property(case: ListCase) {
    let mut page = String::new();
    for (k, element) in case.elements.iter().enumerate() {
        let id = ["", " id=\"i0\"", " id=\"i1\""][element.id as usize];
        let classes: Vec<&str> = ["c0", "c1"]
            .into_iter()
            .enumerate()
            .filter(|(bit, _)| element.classes & (1 << bit) != 0)
            .map(|(_, class)| class)
            .collect();
        page.push_str(&format!("<p{id} class=\"{}\">E{k}</p>", classes.join(" ")));
    }
    let css = format!(
        "{BASE_CSS}\n{}",
        case.rules.iter().map(list_rule_css).collect::<String>()
    );

    let mut ui = TestUi::new("qc-lists", &[("page.html", &page), ("style.css", &css)])
        .stylesheet("style.css")
        .spawn("page.html", TemplateContext::new(), Node::default());
    let dump = ui.settle().dump();
    for (k, element) in case.elements.iter().enumerate() {
        let [r, g, b] = cascaded(&case, element, |rule| rule.color, [0xff; 3]);
        let size = cascaded(&case, element, |rule| rule.size, 20);
        let expected = format!("\"E{k}\" default {size}px #{r:02x}{g:02x}{b:02x}");
        assert!(
            dump.contains(&expected),
            "expected run {expected:?} for {element:?}\nCSS:\n{css}\n--- dump ---\n{dump}"
        );
    }
}

// ---------------------------------------------------------------------------
// Arbitrary text round trips through the template
// ---------------------------------------------------------------------------

/// Printable, non-whitespace ASCII — enough to hit every escape Tera emits
/// (`<`, `>`, `&`, `"`, `'`) without whitespace collapsing changing the text.
#[derive(Clone, Debug)]
struct SafeText(String);

const SAFE: &[u8] = b"abcXYZ0123456789.,:;!?()[]{}<>+-*/=#%&\"'\\_|~^$@`";

impl Arbitrary for SafeText {
    fn arbitrary(g: &mut Gen) -> Self {
        let len = *g.choose(&[1usize, 5, 10, 20, 40]).unwrap();
        let chars: String = (0..len).map(|_| *g.choose(SAFE).unwrap() as char).collect();
        Self(chars)
    }
}

/// Whatever a context variable holds, `{{ s }}` renders it back verbatim:
/// Tera's autoescaping and the crate's decoding must cancel exactly.
#[quickcheck]
fn context_text_round_trips_through_template(text: SafeText) {
    let SafeText(s) = &text;
    let page = "<p>{{ s }}</p>";
    let mut ui = TestUi::new("qc-text", &[("page.html", page), ("style.css", BASE_CSS)])
        .stylesheet("style.css")
        .spawn(
            "page.html",
            TemplateContext::new().with("s", s),
            Node::default(),
        );
    let expected = format!("{s:?} default 20px #ffffff");
    let dump = ui.settle().dump();
    assert!(
        dump.contains(&expected),
        "text {s:?} did not round trip\n--- dump ---\n{dump}"
    );
}
