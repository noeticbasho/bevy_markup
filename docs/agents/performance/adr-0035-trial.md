# ADR 0035 trial: html5ever against tl

Outcome: ADR 0035 rejected; the decoding cleanup moved to ADR 0036
(htmlize under tl), whose own trial is
[adr-0036-trial.md](adr-0036-trial.md).

| Field | Value |
|---|---|
| Date | 2026-10-29 00:20 +0400 |
| Author | Claude Opus 5.5 (Anthropic), via Claude Code |
| Branch | `cleanup/offload-to-dependencies` at `c91d9a7`; the trial crate was out of tree (not committed) |
| Machine | Arch Linux (kernel 7.2.11), AMD Ryzen 7 9800X3D, rustc 1.98.1 |

The trial measured what parsing bevy_markup's documents costs with
html5ever 0.40.1 against `astral-tl` 0.8.0, before migrating any code,
and checked htmlize 1.1.0 as a decoder for the keep-tl route.

## What was built

A standalone crate (criterion 0.7) with:

- **An arena `TreeSink`** shaped like the one a switch would ship: a
  linked-list arena during the parse (appends, insert-before and
  reparenting are what the tree builder asks for), converted on `finish`
  to child lists, attributes and text as owned `String`s (entities
  decoded by html5ever), and a 64-bit subtree hash per node standing in
  for `same_source`'s raw-text comparison.
- **A null sink**: keeps only element names (the tree builder reads
  them) and drops everything else. It is the floor for any html5ever
  integration.
- **Documents**: the crate's own bench pages, rendered by
  `benches/shared` (`shell` = the `rebuilding` workload, `selectors`,
  `localized`), at 120 and 600 elements.
- `tl` measured as the crate uses it (`parse_owned` on a cloned
  `String`); every arm clones the source the same way.

**Shape check**: the arena's tree for `<html class="hud"><table><tr>…`
has the class on the root `html`, an implied `tbody`, and decoded text
and attribute values; element counts equal tl's plus `html`, `head` and
`body` on all six documents.

## Results

Criterion medians, µs per parse:

| Document | tl | html5ever, null sink | html5ever, arena |
|---|---|---|---|
| shell-120 | 7.8 | 43.8 (5.6×) | 79.2 (10×) |
| shell-600 | 33.2 | 197.5 (5.9×) | 361.2 (11×) |
| selectors-120 | 6.8 | 47.2 (6.9×) | 68.9 (10×) |
| selectors-600 | 34.0 | 234.6 (6.9×) | 340.1 (10×) |
| localized-120 | 11.2 | 103.5 (9.2×) | 148.7 (13×) |
| localized-600 | 55.2 | 525.2 (9.5×) | 743.6 (13×) |

A parse-plus-read variant (both sides walking every element name,
attribute and text, as the build does) charges tl for the work it defers
(attribute parsing is lazy): tl rises about 50 %, html5ever 1–4 %. The
gap narrows but stays wide:

| Document | tl, parse + read | html5ever arena, parse + read |
|---|---|---|
| shell-120 | 11.3 | 76.7 (6.8×) |
| selectors-120 | 10.0 | 67.7 (6.8×) |
| selectors-600 | 50.3 | 332.1 (6.6×) |
| localized-120 | 18.7 | 146.7 (7.9×) |
| localized-600 | 89.9 | 715.4 (8.0×) |

(First run, 4 s per measurement; shell-600's row was lost to output
formatting and wasn't re-run.)

**In frame terms.** `frame/changing/600` (re-render and re-parse each
frame) measured 1.53 ms in the ADR 0034 trial. The null-sink floor adds
about 150–165 µs (+10–11 %; the lower figure charges tl its deferred
attribute reads); the arena as built adds 300–330 µs (+20 %).
Optimizing the arena (tendrils instead of `String`s, no hash) could
recover part of the 160 µs between the two, not the floor. ADR 0034's
budget was 10 %.

## htmlize as the decoder

Checked here first, and measured properly in the ADR 0036 trial.

## Not done

- No bevy_markup code was migrated; the numbers decided the route
  before that.
- html5gum and html5ever's bare tokenizer weren't measured (see the
  ADR's alternatives).
