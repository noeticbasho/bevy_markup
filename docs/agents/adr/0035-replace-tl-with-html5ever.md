# 35. Replace tl with html5ever

| Field | Content |
|---|---|
| `ADR` | `0035` |
| `Title` | Replace tl with html5ever |
| `Date` | 2026-10-29 00:20 +0400 |
| `Author` | Claude Opus 5.5 (Anthropic), via Claude Code |
| `Commit` | `c91d9a7 Add HTML Tab order: HtmlFocus::tab and Bevy TabIndex/TabGroup` |
| `Status` | Rejected (2026-10-29, after the trial: parsing would cost 6–9× tl on the per-change path; the decoding cleanup it promised is taken by ADR 0036 instead) |
| `Related` | ADR 0001, 0027, 0029, 0034, 0036; bug_0019; UPSTREAM.md U11; [trial report](../performance/adr-0035-trial.md) |

## Context

**What parses HTML today.** `tl` (as `astral-tl` 0.8.0, astral-sh's
maintained fork, UPSTREAM U11) turns each rendered template into a flat
node arena that borrows the source text. It is a tag-soup tokenizer, not
an HTML5 parser: it builds the tree exactly as written, with none of the
spec's tree construction (no implied `tbody`, `<p>` isn't closed by a
following block, `<div/>` closes) and no character-reference decoding.

The crate fills the decoding gap itself: `template.rs` `decode_entities`
(about 120 lines) over `src/entity_data.rs`, a 2,134-line table generated
by `scripts/generate_entities.py` from WHATWG's `entities.json` (ADR
0027).

`tl::NodeHandle` is the crate's node key: specs, reconciliation pairing,
window culling and pseudo-state maps all key on it (about 125 uses in
`build.rs`), and `same_source` compares two subtrees by their raw source
text, which only works because `tl` borrows the source. `HtmlDocument::dom`
returns a `tl::VDom`, and the crate re-exports `tl`.

**Where parsing sits in the frame.** Every content change re-renders the
template and re-parses it (an identical render is skipped). A HUD whose
counter changes each frame parses each frame: the `frame/changing/600`
bench, 1.53 ms per frame in the ADR 0034 trial, includes one 33 µs parse.

## Decision (rejected)

Replace `tl` with html5ever, the spec parser Servo uses. Parse each
template as a document (`parse_document`: a top-level `<html class>`
merges onto the root as in Chrome, content lands in `<body>`; Fluent
fragments with `parse_fragment` in their element's context) into a
crate-owned arena (`NodeId(u32)`, child lists, decoded attributes) built
by a `TreeSink`, with a per-node subtree hash replacing `same_source`;
migrate the `tl` call sites (about 180) to it.

Expected gains: spec tree construction (the oracle's `tbody` special case
goes), spec character references (the decoder and the table go), no
`unsafe` `parse_owned`.

## Why rejected

The trial (report linked above) parsed the crate's bench pages with both:

- html5ever alone, with a sink that keeps nothing, takes 198 µs on the
  600-element page against tl's 33 µs (6–9.5× across the pages); with an
  arena like the one the switch would ship, 361 µs.
- That adds 10–20 % to `frame/changing/600`, at or above the 10 % budget
  ADR 0034 held, on the path every content change takes.
- The gains don't need the parser. The decoder and its table — most of
  the code the switch would remove — go with a decoding crate under tl
  (ADR 0036). Tree construction is the one gain left, and no template in
  the repository hits a divergence it would fix.

## Revisit when

- parsing leaves the per-change path (e.g. a template-level diff that
  re-parses only changed regions), or
- tag-soup divergences (no implied `tbody`, no implicit `</p>`, `<div/>`
  closing) start costing real bugs.

The trial's sink and benchmark (out of tree, described in the report) are
the starting point.

## Alternatives considered

- **html5ever's tokenizer without its tree builder**: still several times
  tl, and keeps the crate's tree logic, so only the decoder would go.
- **html5gum** (a WHATWG tokenizer with decoding): the same trade; not
  measured.
- **scraper** (html5ever plus an `ego-tree` arena): html5ever's cost plus
  servo's `selectors`, a second selector engine next to
  `parcel_selectors` (ADR 0034).
