//! The port against fluent-syntax itself: the same entries on generated
//! FTL, and the limit safe for the real parser and resolver on a 2 MB
//! thread.

use super::*;
use proptest::prelude::*;

/// fluent-syntax's entries for `source` (its own parser, unpatched).
fn real_entries(source: &str) -> Vec<Entry> {
    let resource = match fluent_syntax::parser::parse_runtime(source) {
        Ok(resource) => resource,
        Err((resource, _)) => resource,
    };
    resource
        .body
        .iter()
        .filter_map(|entry| match entry {
            fluent_syntax::ast::Entry::Message(message) => {
                Some(Entry::Message(message.id.name.to_owned()))
            }
            fluent_syntax::ast::Entry::Term(term) => Some(Entry::Term(term.id.name.to_owned())),
            fluent_syntax::ast::Entry::Junk { content } => {
                let start = content.as_ptr() as usize - source.as_ptr() as usize;
                Some(Entry::Junk(start, start + content.len()))
            }
            _ => None,
        })
        .collect()
}

fn port_entries(source: &str) -> Vec<Entry> {
    let mut parser = Parser::new(source.as_bytes());
    parser.parse().expect("within the limit");
    parser.entries
}

/// Nested `levels` deep by one of the parser's recursion cycles.
fn nested(kind: &str, levels: usize) -> String {
    match kind {
        "braces" => format!("x = {}\"a\"{}\n", "{".repeat(levels), "}".repeat(levels)),
        "calls" => format!(
            "x = {{ {}1{} }}\n",
            "F(".repeat(levels - 1),
            ")".repeat(levels - 1)
        ),
        "selects" => {
            let mut inner = "deep".to_owned();
            for _ in 0..levels {
                inner = format!("{{ $n ->\n *[other] {inner}\n}}");
            }
            format!("x = {inner}\n")
        }
        _ => unreachable!(),
    }
}

#[test]
fn incomplete_escapes_before_non_ascii_are_refused() {
    for bad in [
        "u = {\"\\U\u{fffd}\"}\n",
        "u = { \"\\u12é\" }\n",
        "cmd = Wpisz { \"\\użyj\" }\n",
    ] {
        assert!(
            matches!(check(bad), Err(FtlCheckError::BadEscape(_))),
            "{bad:?}"
        );
    }
    for fine in [
        "path = C:\\Users\\me\n",
        "u = { \"\\u00e9\\U01F600\" }\n",
        "x = \\u12 é\n",
        // bug_0236: in text a `\u` is plain characters.
        "x = \\Ué\n",
        "path = Folder C:\\użytkownik\n",
        "cmd = Wpisz \\użyj w konsoli\n",
        "x = \\u12é\n    .attr = C:\\użytkownik\n",
    ] {
        assert_eq!(check(fine), Ok(()), "{fine:?}");
    }
}

/// Each recursion cycle: up to the limit passes, one more is refused.
#[test]
fn nesting_is_bounded_on_every_cycle() {
    for kind in ["braces", "calls", "selects"] {
        assert_eq!(check(&nested(kind, MAX_FTL_NESTING)), Ok(()), "{kind}");
        assert!(
            matches!(
                check(&nested(kind, MAX_FTL_NESTING + 1)),
                Err(FtlCheckError::TooDeep(_))
            ),
            "{kind}"
        );
    }
    // Deep in an attribute, after a junk entry, behind a comment: still
    // seen. A huge depth is refused without recursing that deep.
    let deep = nested("braces", MAX_FTL_NESTING + 1);
    for source in [
        format!("ok = fine\n    .attr = {}\n", &deep[4..]),
        format!("broken = }}\n{deep}"),
        format!("# comment\n{deep}"),
        nested("braces", 100_000),
        nested("calls", 100_000),
    ] {
        assert!(matches!(check(&source), Err(FtlCheckError::TooDeep(_))));
    }
}

/// What the check accepts, the real parser parses and the resolver formats
/// on a 2 MB thread, the stack of Bevy's asset and task threads, in this
/// (debug) build: every cycle at the limit, and the cycles interleaved.
#[test]
fn the_limit_fits_a_two_megabyte_stack() {
    let mut sources: Vec<String> = ["braces", "calls", "selects"]
        .iter()
        .map(|kind| nested(kind, MAX_FTL_NESTING))
        .collect();
    // Interleaved: a placeable (1 level), a call (1), a select whose
    // variant holds a placeable (2), up to exactly the limit.
    let mut mixed = "1".to_owned();
    let mut depth = 1; // the outer `{ … }`
    for step in 0.. {
        let (cost, next) = match step % 3 {
            0 => (1, format!("{{ {mixed} }}")),
            1 => (1, format!("NUMBER({mixed})")),
            _ => (2, format!("{{ $n ->\n *[other] {{ {mixed} }}\n}}")),
        };
        if depth + cost > MAX_FTL_NESTING {
            break;
        }
        depth += cost;
        mixed = next;
    }
    assert!(depth >= MAX_FTL_NESTING - 1);
    sources.push(format!("x = {{ {mixed} }}\n"));
    for source in sources {
        assert_eq!(check(&source), Ok(()), "accepted");
        let formatted = std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(move || {
                let resource = fluent::FluentResource::try_new(source).ok()?;
                let locale: unic_langid::LanguageIdentifier = "en".parse().ok()?;
                let mut bundle = fluent::FluentBundle::new(vec![locale]);
                bundle.add_resource(resource).ok()?;
                let message = bundle.get_message("x")?;
                let mut errors = Vec::new();
                let args = fluent::FluentArgs::new();
                Some(
                    bundle
                        .format_pattern(message.value()?, Some(&args), &mut errors)
                        .into_owned(),
                )
            })
            .unwrap()
            .join()
            .expect("no stack overflow");
        assert!(formatted.is_some());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2000))]

    /// The escape refusal is exactly fluent-syntax's panic: refused if and
    /// only if `parse_runtime` panics on the resource (bug_0005), in
    /// string literals and text alike (bug_0236).
    #[test]
    fn bad_escapes_are_refused_exactly_when_fluent_syntax_panics(
        pieces in prop::collection::vec(
            prop::sample::select(vec![
                "x = ", "\n", "{ ", " }", "{", "}", "\"", "\\u", "\\U", "\\", "1", "12",
                "1234", "abcdef", "é", "ż", "\u{fffd}", "text ", "-t = ", ".a = ", "  ",
                "{ \"\\u", "{ \"\\U12", "\" }", "$x ->\n *[a] ", "F(", ")",
                // Whole literals around the boundary.
                "{ \"\\u1é\" }", "{ \"\\Uż\" }", "{ \"\\u123é\" }", "{ \"\\u1234é\" }",
                "{ \"\\U12345ż\" }", "{ F(\"\\uż\") }", "{ $x ->\n *[a] { \"\\u1ż\" }\n}",
            ]),
            0..16,
        )
    ) {
        let source = format!("x = {}", pieces.concat());
        let panics = std::panic::catch_unwind(|| {
            let _ = fluent_syntax::parser::parse_runtime(source.as_str());
        })
        .is_err();
        let refused = matches!(check(&source), Err(FtlCheckError::BadEscape(_)));
        prop_assert_eq!(refused, panics, "{:?}", source);
    }

    /// The port reads every resource as fluent-syntax does: the same
    /// messages, terms and junk spans, so error recovery restarts where
    /// the real parser restarts and no entry escapes the depth count.
    #[test]
    fn the_port_finds_fluent_syntaxs_entries(
        pieces in prop::collection::vec(
            prop::sample::select(vec![
                "msg = ", "-term = ", "other = ", "\n", "\r\n", "  ", " ", "{", "}", "{ ",
                " }", "$x", "$", "\"str\"", "\"", "\"a\\\"b\"", "\"\\u00e9\"", "\"\\u1\"",
                "F(", "NUMBER(", "f(", ")", "a: ", "a:", "1", "-1.5", "2.", " -> ", "->\n",
                " ->\n", "[one]", "*[other]", "[1]", "*", "[", "]", ".attr = ", ".", "# c\n",
                "## g\n", "#\n", "text", "é", ",", "x.y", "-t", "-t.a", "-t(a: 1)", "   .a = v\n",
                "\\", "=",
                // Whole constructs, for the errors only they reach.
                "{ f() }", "{ F(1) }", "{ F(a: 1, b: 2) }", "{ F(a: 1, 2) }",
                "{ F(a: 1, a: 2) }", "{ F(a: 1, b) }", "{ F(a: $x) }", "{ -t.a }",
                "{ -t(a: 1) }",
                "{ msg.attr }", "{ { $x } }", "{ 1 }", "{ \"s\" }",
                "{ msg ->\n *[a] x\n}", "{ msg.a ->\n *[a] x\n}", "{ -t ->\n *[a] x\n}",
                "{ -t.a ->\n *[a] x\n}", "{ { 1 } ->\n *[a] x\n}",
                "{ $x ->\n [a] x\n *[b] y\n}", "{ $x ->\n *[a] x\n *[b] y\n}",
                "{ $x ->\n [a] x\n}", "{ $x -> *[a] x }", "{ $x ->\n *[-1.5] x\n}",
            ]),
            0..40,
        )
    ) {
        let source: String = pieces.concat();
        prop_assert_eq!(port_entries(&source), real_entries(&source), "{:?}", source);
    }
}
