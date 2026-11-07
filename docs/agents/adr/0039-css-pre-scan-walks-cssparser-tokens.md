# 39. The CSS nesting pre-scan walks cssparser's tokens

| Field | Content |
|---|---|
| `ADR` | `0039` |
| `Title` | The CSS nesting pre-scan walks cssparser's tokens |
| `Date` | 2026-11-07 18:55 +0400 |
| `Author` | Claude Opus 5.5 (Anthropic), via Claude Code; proposed by Nikita Chashchinskii |
| `Commit` | `3b89a29 Fix bug 184: end unquoted URLs as cssparser does in the nesting pre-scan` + uncommitted (this change) |
| `Status` | Accepted |
| `Related` | bug_0092, bug_0106, bug_0179 (why the pre-scan exists), bug_0183, bug_0184 (its two disagreements with the tokenizer); UPSTREAM U14, U15; ADR 0036 (in-domain crates replacing maintained code), 0038 (the same principle for SVG: check with the parser's own parser) |

## Context

`cascade::nesting_over_limit` rejects CSS text whose blocks nest past
`MAX_CSS_NESTING` (32), or whose parentheses nest past
`MAX_CSS_PAREN_NESTING` (8), before lightningcss parses it. lightningcss
recurses once per block without a bound (UPSTREAM U15), and parses nested
`light-dark(` and `calc(var(…))` in exponential time (U14, bug_0179). The
scan runs on every stylesheet source, every `style=` attribute and every
substituted `var()` value.

It was a hand-written tokenizer over bytes. Its contract was "never count
less than cssparser" (cssparser 0.37 is lightningcss's tokenizer); where
it counts less, a hostile text reaches the exponential parse. It reached
~300 lines (`css_name`, `css_escape`, `css_ident_start`, numbers,
strings, comments, `<!--`, and since bug_0184 the three states of an
unquoted URL) and still failed the contract twice:
- **bug_0183:** a one-byte look-behind decided what a `url(` is;
- **bug_0184:** a `\)` after an unquoted URL's whitespace.

Each fix copied more of cssparser's `tokenizer.rs`. cssparser was
already in the tree (lightningcss depends on `=0.37.0`) and a
dev-dependency, as the property test's oracle.

## Decision

The pre-scan walks cssparser's own tokens. cssparser 0.37.0 moves from
`[dev-dependencies]` to `[dependencies]`, pinned to lightningcss's
version, so there is one copy.

`nesting_over_limit` (`src/cascade.rs`) runs a `cssparser::Parser` over
the text. For each `Function` or `ParenthesisBlock` token, and each
`[` / `{` block, it adds to the counts. It returns `true` as soon as
either count passes its limit, and otherwise enters the block with
`parse_nested_block`.

The recursion is bounded by the limits: at most 33 levels. Whatever the
walk doesn't enter, cssparser skips with `consume_until_end_of_block`, a
loop over an explicit stack (`parser.rs`). The hand-written helpers are
deleted.

## Alternatives considered

- **Keep the hand-written scan.** It's faster (table below) and has no
  dependency, but its correctness is "mirror another tokenizer
  byte-for-byte", which failed twice. The property test catches such a
  failure only when a random seed happens to reach it: bug_0184 surfaced
  on an unrelated run, not in CI.
- **Use cssparser's `Tokenizer` directly.** It's crate-private in 0.37
  (`lib.rs` exports `Parser`, `ParserInput`, not the tokenizer).
- **Bound lightningcss instead.** Its recursion and its exponential
  re-parse are upstream defects (U14, U15) with no released fix, and a
  timeout can't interrupt a parse running on an asset thread.

## Consequences

- The pre-scan can't disagree with the tokenizer about what a block is:
  strings, comments, escapes, names, CDO/CDC and `url(` tokens are
  cssparser's. About 300 lines of tokenizer and their upkeep are gone.
- Measured in a release test build, best of five (Ryzen 7 9800X3D):

  | Input | Hand-written | cssparser walk |
  |---|---|---|
  | 1 MB stylesheet (the examples' CSS, repeated) | 1.6 ms | 3.4 ms |
  | 100,000 `style` attributes (`width: calc(…); color: rgb(…)`) | 9.6 ms | 28 ms |
  | rejecting 5,000-deep `light-dark(`, ×1000 | 0.17 ms | 125 ms |

  Both scans agreed on the 1 MB sheet. Attribute values cost ~280 ns
  instead of ~95 ns.

  A reject is now one linear pass over the input (~450 MB/s): after the
  walk stops, cssparser finishes skipping the rest. The old scan returned
  at the first block past the limit. Rejected text is hostile, and the
  cost stays linear.
- Tests:
  - `the_pre_scan_never_undercounts_the_tokenizer` now pins the walk
    (its early exit and two counts) against a full walk of the same
    tokens.
  - The bug_0183 and bug_0184 spelling tests stay as vectors.
  - `the_pre_scan_recursion_is_bounded` rejects a million unclosed blocks
    of each kind on a 2 MB test thread.
  - The committed `css` fuzz seeds replay cleanly, and a 2-minute
    cargo-fuzz `css` run (270,745 inputs) found nothing.
- The pin must follow lightningcss. If a lightningcss release moves to
  cssparser 0.38, `=0.37.0` doesn't fail the build: Cargo keeps both
  versions, and the scan would tokenize with a different version than
  the parser. On every lightningcss bump, check `cargo tree -i cssparser`
  shows one version.
