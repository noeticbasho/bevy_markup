# ADR 0036 trial: htmlize against the crate's decoder

| Field | Value |
|---|---|
| Date | 2026-10-29 01:40 +0400 |
| Author | Claude Opus 5.5 (Anthropic), via Claude Code |
| Branch | `cleanup/offload-to-dependencies`, from `c91d9a7`; microbenchmarks in an out-of-tree crate (not committed) |
| Machine | Arch Linux (kernel 7.2.11), AMD Ryzen 7 9800X3D, rustc 1.98.1; Chromium 156 for the oracle |

The trial replaced `decode_entities` and its generated table with
htmlize 1.1.0 and measured correctness against the old decoder and
Chrome, decoding speed, the pipeline, and the build.

## What was built

- `template.rs`: `decode_text` (`htmlize::unescape`) and
  `decode_attribute` (`htmlize::unescape_attribute`), taking
  `impl Into<Cow<str>>` and returning `Cow<str>`. The ~20 call sites use
  the one for what they read: text nodes and Markdown code `decode_text`;
  every attribute value (`style`, `src`, `href`, `is`, `data-*`,
  `data-with`, `data-l10n-id`/`-args`/`-name`, `value`, `name`, the
  element reader) `decode_attribute`.
- Removed: `decode_entities`, `terminated_digits`, `C1_REPLACEMENTS`,
  `src/entity_data.rs` (2,134 lines), `scripts/generate_entities.py`, and
  the content lint's own five-escape decoder (it calls htmlize now). Net
  −2,273 lines in `src/` and `scripts/`.
- Tests: the table-restating unit test became
  `references_decode_as_the_spec_says` (text and attribute expectations
  per input, including the legacy rules), plus
  `decoding_borrows_without_references`; the linear-time test
  (review_0005 I04) now guards htmlize. `entities_decode_through_the_pipeline`
  expects the spec's `&notareal;` → "¬areal;" and keeps an unknown
  reference verbatim.
- New oracle vector `character_references`: legacy, unterminated
  numeric, kept and long named references in text; four attribute values
  checked through attribute selectors (`[data-q="?a=1&copy=2"]` must
  match, so the value stayed verbatim; `[data-n="A"]` for `&#65`; …).

## Correctness

| Check | Result |
|---|---|
| `cargo test` (all binaries) | pass: 488; with `text-input` 494 |
| Browser oracle (Chrome) | all vectors pass, including `character_references` |
| `character_references` on the old decoder | **fails** (text: `&copy2`, `&amp`, `&#65` … left verbatim), so it tests the change |
| Differential, old decoder vs htmlize: 96,630 values (every attribute value and text node in the 16,315-file html fuzz corpus and the bench pages, plus 31,250 generated combinations in both modes) | 80,878 identical; every one of the 15,752 differences is a legacy name or a numeric reference without `;` (10,233 and 5,519); none unexplained |
| cargo-fuzz `html` | 300 s, 770,818 runs, no crash or timeout |
| Warning-free `check --lib` (± `text-input`), `build --all-targets`, `doc` | clean |

## Decoding speed

Criterion medians. `decode_entities` compiled verbatim from `c91d9a7`
next to htmlize; each input decoded in the mode its call site uses.

| Input | `decode_entities` | htmlize (`phf`) | htmlize (`fast`) |
|---|---|---|---|
| class names, no `&` | 12.4 ns | 8.1 ns | 8.1 ns |
| text, no `&` | 13.0 ns | 8.6 ns | 8.7 ns |
| escaped JSON attribute (`data-with`) | 2.31 µs | 203 ns | 111 ns |
| text with five references | 831 ns | 127 ns | 88 ns |
| 200 × `&lt;b&gt;` | 190.6 µs | 7.3 µs | 4.3 µs |
| every value in shell-600 | 21.1 µs | 12.0 µs | 12.0 µs |
| every value in selectors-600 | 10.7 µs | 6.3 µs | 6.2 µs |
| every value in localized-600 | 35.3 µs | 19.4 µs | 19.4 µs |

The old decoder's cost on references came from its longest-match loop
(a binary search per candidate end position). The two htmlize backends
tie on whole documents; `fast` wins only on reference-dense values.

## Build

Minimal binary using both functions, release, stripped:

| | Clean build | Binary |
|---|---|---|
| no htmlize | — | 348,568 B |
| `unescape` (`phf`) | 1.4 s | 570,640 B |
| `unescape_fast` | 5.0 s | 559,368 B |

Chosen: `unescape` — the same document speed, 3.6 s less clean build.

## Pipeline

`benches/pipeline.rs`, criterion medians, `--baseline before-htmlize`
(saved on `c91d9a7` just before the change):

| Bench | Before | After |
|---|---|---|
| frame/changing/120 | 502.8 µs | 463.6 µs |
| frame/changing/600 | 1.557 ms | 1.547 ms |
| frame/rebuilding/120 | 2.430 ms | 2.453 ms |
| frame/rebuilding/600 | 10.78 ms | 10.97 ms (re-run: 10.78 ms, +0.0 %) |
| frame/selectors/600 | 6.136 ms | 6.208 ms (re-run: +1.9 %) |
| frame/localized/600 | 3.788 ms | 3.706 ms |
| frame/markdown/600 | 19.90 ms | 19.57 ms |
| parse/600 | 43.7 µs | 50.5 µs (re-run: 44.6 µs, +1.6 %, p = 0.52) |

All within ±2 % on re-run, which is noise for this suite: `parse` runs
no decoding at all and still moved 15 % in the first run. Decoding is a
small share of a frame (12 µs of a 1.5–11 ms frame at 600 elements), so
its 1.8× speed-up doesn't show either.

## Not done

- Golden images weren't run locally (lavapipe; the nightly runs them).
- Divan allocation counts weren't compared; the borrowing `Cow` returns
  can only remove allocations.
