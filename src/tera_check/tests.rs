//! The check against real templates: hostile ones refused for their own
//! reason, ordinary ones accepted, and bounds that hold when Tera renders.

use super::*;
use crate::template::new_tera;

fn check_one(source: &str) -> Result<analysis::Report, CheckError> {
    let name = "t.html".to_owned();
    // Only what Tera compiles reaches the check.
    let mut tera = new_tera();
    tera.add_raw_template(&name, source)
        .unwrap_or_else(|err| panic!("Tera refuses {source:?}: {err}"));
    check(&name, &[(name.clone(), source.to_owned())])
}

fn refused(source: &str) -> CheckError {
    match check_one(source) {
        Ok(report) => panic!("accepted {source:?}: {report:?}"),
        Err(err) => err,
    }
}

/// The probes behind bug_0220 and their variations: each refused, by the
/// limit it would break.
#[test]
fn hostile_templates_are_refused() {
    use CheckError::*;
    let doubling: String = (0..20).map(|_| "{% set s = s ~ s ~ s ~ s %}").collect();
    for (source, expected) in [
        // Hours of loops printing nothing.
        (
            "{% for i in range(end=100000) %}{% for j in range(end=100000) %}{% endfor %}{% endfor %}"
                .to_owned(),
            TooManySteps,
        ),
        // A terabyte by doubling across iterations, and unrolled.
        (
            "{% set_global s = 'x' %}{% for i in range(end=40) %}{% set_global s = s ~ s %}{% endfor %}"
                .to_owned(),
            Superlinear("s".into()),
        ),
        (format!("{{% set s = 'x' %}}{doubling}{{{{ s }}}}"), ValueTooLarge),
        // A list nested in itself: shared in memory, exponential written.
        (
            "{% set_global a = [1] %}{% for i in range(end=60) %}{% set_global a = [a, a] %}{% endfor %}{{ a }}"
                .to_owned(),
            Superlinear("a".into()),
        ),
        // A capture of 2 × 10⁷ bytes in 10⁷ steps.
        (
            "{% set c %}{% for i in range(end=10000) %}{% for j in range(end=1000) %}xx{% endfor %}{% endfor %}{% endset %}"
                .to_owned(),
            ValueTooLarge,
        ),
        // Amplifying filters.
        (
            "{% set s = range(end=100000) | join(sep='x') %}{{ s | replace(from='', to=s) }}"
                .to_owned(),
            ValueTooLarge,
        ),
        ("{{ name | replace(from='', to=name) }}".to_owned(), AppSquared),
        (
            "{{ [range(end=100000) for i in range(end=100000)] | length }}".to_owned(),
            ValueTooLarge,
        ),
        // A context value copied past MAX_TEMPLATE_APP_COPIES.
        (
            format!(
                "{{% set s = name %}}{}{{{{ s }}}}",
                "{% set s = s ~ s %}".repeat(13)
            ),
            TooManyCopies,
        ),
        // Growth through a filter in a loop.
        (
            "{% set_global s = 'ab' %}{% for i in range(end=50) %}{% set_global s = s | replace(from='a', to='aa') %}{% endfor %}"
                .to_owned(),
            Superlinear("s".into()),
        ),
        // Two carried values feeding each other: quadratic.
        (
            "{% set_global a = 'x' %}{% set_global b = 'x' %}{% for i in range(end=100000) %}{% set_global a = a ~ b %}{% set_global b = b ~ 'x' %}{% endfor %}"
                .to_owned(),
            Superlinear("a".into()),
        ),
        // A component calling itself (Tera stops at 20 levels: 10⁵²⁰).
        (
            "{% component c(n) %}{% for i in range(end=100000) %}{{ <c n={n} /> }}{% endfor %}{% endcomponent %}{{ <c n={1} /> }}"
                .to_owned(),
            Recursive("c".into()),
        ),
    ] {
        assert_eq!(refused(&source), expected, "{source}");
    }
}

/// What the examples and ordinary templates do passes: loops over context
/// data (nested, per app element), literal lists, `range` sized by the
/// context, counters and linear accumulators, components, conditionals.
#[test]
fn ordinary_templates_pass() {
    for source in [
        "{% for group in groups %}<section>{% for card in group.cards %}<p class='{{ card.kind }}'>{{ card.name | upper }}</p>{% endfor %}</section>{% endfor %}",
        "{% for tab in ['items', 'gear', 'quests'] %}<b data-tab='{{ tab }}'>{{ tab }}</b>{% endfor %}",
        "{% for i in range(start=1, end=settings.cards + 1) %}<div>{{ i }}</div>{% endfor %}",
        "{% for day in range(start=1, end=61) %}<p data-l10n-id='log-entry' data-l10n-args='{{ {\"day\": day} | json }}'></p>{% endfor %}",
        "{% set_global n = 0 %}{% for row in rows %}{% set_global n = n + 1 %}{% set_global names = names ~ row.name ~ ', ' %}{% endfor %}{{ n }} {{ names }}",
        "{% component ui.button(label, class='', focusable=true) %}<button class='{{ class }}'>{{ body }}{{ label }}</button>{% endcomponent %}{% <ui.button label='Go' class='big'> %}<b>!</b>{% </ui.button> %}{{ <ui.button label={name} /> }}",
        "{% if a %}{% set x = a ~ b %}{% elif c %}{% set x = 'c' %}{% else %}{% set x = '' %}{% endif %}{{ x }}",
        "{% set x %}{% for r in rows %}<li>{{ r }}</li>{% endfor %}{% endset %}<ul>{{ x | safe }}</ul>",
        "{{ items if false else items | length }}",
        "{% for k, v in stats %}{{ k }}={{ v | round }}{% if not loop.last %}, {% endif %}{% endfor %}",
        "{% filter upper %}{% for r in rows %}{{ r.name }}{% endfor %}{% endfilter %}",
    ] {
        let source = source.to_owned();
        if let Err(err) = check_one(&source) {
            panic!("refused {source:?}: {err}");
        }
    }
}

/// `include`, `extends` with blocks and `super()`, across a set.
#[test]
fn includes_and_inheritance() {
    let base = "<html>{% block body %}<p>base</p>{% for i in range(end=1000) %}{{ i }}{% endfor %}{% endblock body %}</html>";
    let child = "{% extends 'base.html' %}{% block body %}{{ super() }}{% include 'part.html' %}{% endblock %}";
    let part = "{% for r in rows %}{{ r }}{% endfor %}";
    let sources = vec![
        ("base.html".to_owned(), base.to_owned()),
        ("child.html".to_owned(), child.to_owned()),
        ("part.html".to_owned(), part.to_owned()),
    ];
    let mut tera = new_tera();
    tera.add_raw_templates(sources.clone()).unwrap();
    let report = check("child.html", &sources).unwrap();
    // `super()` counts the base block's loop.
    assert!(report.steps >= 1000, "{report:?}");
    // An include of itself is refused.
    let looping = vec![("a.html".to_owned(), "{% include 'a.html' %}".to_owned())];
    assert_eq!(
        check("a.html", &looping).unwrap_err(),
        CheckError::Recursive("a.html".into())
    );
}

/// The check bounds itself: a chain of includes deeper than it recurses,
/// or scopes copied at every `if` past its work budget, is refused as too
/// complex, not a stack overflow or a slow load.
#[test]
fn the_check_bounds_its_own_work() {
    let chain: Vec<(String, String)> = (0..200)
        .map(|i| {
            (
                format!("{i}.html"),
                format!("{{% include '{}.html' %}}", i + 1),
            )
        })
        .chain([("200.html".to_owned(), "end".to_owned())])
        .collect();
    assert_eq!(check("0.html", &chain).unwrap_err(), CheckError::TooComplex);
    let wide = format!(
        "{}{}",
        (0..5000)
            .map(|i| format!("{{% set v{i} = 1 %}}"))
            .collect::<String>(),
        "{% if a %}x{% endif %}".repeat(5000)
    );
    assert_eq!(
        check("t.html", &[("t.html".to_owned(), wide)]).unwrap_err(),
        CheckError::TooComplex
    );
}

/// Fail closed: a template the check can't read is refused, not passed.
#[test]
fn unreadable_templates_are_refused() {
    let sources = vec![("t.html".to_owned(), "a\n{{ x ".to_owned())];
    assert_eq!(
        check("t.html", &sources).unwrap_err(),
        CheckError::Unreadable("t.html".into(), 2)
    );
}

mod properties {
    use super::*;
    use proptest::prelude::*;

    /// Small templates over a few context names, the forms that grow
    /// output: loops (literal, `range`, context), concatenation,
    /// amplifying filters, captures, `set`/`set_global`.
    fn expr() -> impl Strategy<Value = String> {
        let leaf = prop_oneof![
            Just("a".to_owned()),
            Just("b".to_owned()),
            Just("'xy'".to_owned()),
            Just("3".to_owned()),
            Just("s".to_owned()),
        ];
        leaf.prop_recursive(3, 12, 3, |inner| {
            prop_oneof![
                (inner.clone(), inner.clone()).prop_map(|(l, r)| format!("({l} ~ {r})")),
                (inner.clone(), inner.clone())
                    .prop_map(|(l, r)| format!("({l} | replace(from='x', to={r}))")),
                (inner.clone(), inner.clone()).prop_map(|(l, r)| format!("({l} | join(sep={r}))")),
                inner.clone().prop_map(|e| format!("({e} | upper)")),
                inner.clone().prop_map(|e| format!("({e} | escape)")),
                inner
                    .clone()
                    .prop_map(|e| format!("({e} | split(pat='y'))")),
                inner.clone().prop_map(|e| format!("[{e}, {e}]")),
                inner.prop_map(|e| format!("({e} | indent(width=3))")),
            ]
        })
    }

    fn template() -> impl Strategy<Value = String> {
        let statement = prop_oneof![
            expr().prop_map(|e| format!("{{{{ {e} }}}}")),
            expr().prop_map(|e| format!("{{% set s = {e} %}}")),
            expr().prop_map(|e| format!("{{% set_global s = {e} %}}")),
            Just("text".to_owned()),
        ];
        let block = statement.prop_recursive(3, 10, 3, |inner| {
            let body = prop::collection::vec(inner, 1..3).prop_map(|v| v.concat());
            prop_oneof![
                (0usize..4, body.clone())
                    .prop_map(|(n, b)| format!("{{% for i in range(end={n}) %}}{b}{{% endfor %}}")),
                body.clone()
                    .prop_map(|b| format!("{{% for i in list %}}{b}{{% endfor %}}")),
                body.clone()
                    .prop_map(|b| format!("{{% set c %}}{b}{{% endset %}}{{{{ c }}}}")),
                body.prop_map(|b| format!("{{% if a %}}{b}{{% else %}}x{{% endif %}}")),
            ]
        });
        prop::collection::vec(block, 1..4)
            .prop_map(|v| format!("{{% set_global s = 'ab' %}}{}", v.concat()))
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(400))]

        /// Whatever the check accepts renders within its output bound: the
        /// constant part plus one copy-count per context value's bytes
        /// (`A`: the largest context value written, its list included),
        /// for one app element.
        #[test]
        fn accepted_templates_render_within_the_bound(
            source in template(),
            len in 0usize..6,
            elements in 0usize..2,
        ) {
            let mut tera = new_tera();
            prop_assume!(tera.add_raw_template("t.html", &source).is_ok());
            let Ok(report) = check("t.html", &[("t.html".to_owned(), source.clone())]) else {
                return Ok(());
            };
            let value = "<&".repeat(len);
            // The bound is per app element: a loop over the context's
            // list counts once, so the list has at most one element.
            let list: Vec<String> = (0..elements).map(|_| value.clone()).collect();
            let mut context = tera::Context::new();
            context.insert("a", &value);
            context.insert("b", &value);
            context.insert("list", &list);
            let Ok(rendered) = tera.render("t.html", &context) else {
                return Ok(());
            };
            // The app's magnitude: its longest value as written (the list,
            // formatted), and per element of it.
            let app = (serde_json::to_string(&list).unwrap().len() * 6).max(len * 2 * 6).max(1) as u64;
            let bound = report.output.c.saturating_add(report.output.k.saturating_mul(app));
            prop_assert!(
                rendered.len() as u64 <= bound,
                "{} > {:?} (A = {}) for {}", rendered.len(), report.output, app, source
            );
        }

        /// Everything Tera compiles from the grammar, the check reads.
        #[test]
        fn the_check_reads_what_tera_compiles(source in template()) {
            let mut tera = new_tera();
            prop_assume!(tera.add_raw_template("t.html", &source).is_ok());
            let result = check("t.html", &[("t.html".to_owned(), source.clone())]);
            prop_assert!(
                !matches!(result, Err(CheckError::Unreadable(..))),
                "{result:?} for {source}"
            );
        }
    }
}
