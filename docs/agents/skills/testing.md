# Testing bevy_markup

How to write tests, run them, and find bugs with bevy_markup's testing harness. It
covers unit tests, the headless test vectors and oracles, the property
frameworks, the fuzzers, golden images and mutation testing. `AGENTS.md`
(Testing, Testing TODO, Gotchas, Verification) has the history and the
reasons behind each piece; this file is the how-to.

## Quick reference

| What | Command | Time |
|---|---|---|
| Everything that must pass | `cargo test` | ~20–30 s |
| Everything, per-test timing (optional) | `cargo nextest run --workspace`, then `cargo test --doc` (nextest skips doc tests); `.config/nextest.toml` writes a JUnit report to `target/nextest/default/junit.xml`, which `../codebase-ingestion` records | ~40 s |
| Library unit tests only | `cargo test --lib` | ~1 s |
| Fuzz-harness unit tests | `cargo test --lib --features fuzzing` | ~1 s |
| One test file | `cargo test --test html_ui` | seconds |
| One test | `cargo test --test html_ui layout_oracle` | seconds |
| Warning-free checks | `cargo check --lib`, `cargo build --all-targets`, `cargo doc --no-deps` | |
| Golden images (software Vulkan) | `scripts/golden.sh` (`--update` rewrites references) | ~5 s warm |
| Regenerate CSS/layout oracle data | `scripts/browser_oracle.py [tests/vectors/<name>]` | ~1 s/vector |
| Regenerate Fluent oracle data | `scripts/fluent_oracle.sh [tests/vectors/<name>]` | <1 s |
| Fuzz one target | `scripts/fuzz-libfuzzer.sh <html\|css\|ftl> [seconds]` | 60 s default |
| Other fuzzers | `scripts/fuzz-{honggfuzz,fuzzcheck,test-fuzz}.sh <target> [seconds]` | |
| Mutation testing | `scripts/mutants.sh [--full] [--file src/x.rs]` | ~15 min full run (estimate) |
| Coverage per layer | `scripts/coverage.py [--html] [--layer NAME]` | ~1.5 min warm |

Rules that always hold:
- `cargo test` and the three warning-free checks must pass before any change
  is done. CI enforces them with `-D warnings`.
- Every real defect gets a regression test *and* a bug report in
  `docs/agents/bugs/` (see [Bug reports](#bug-reports)).
- Expected values are derived by hand from HTML/CSS/Fluent semantics, never
  pasted from actual output. If a test fails, decide whether the code or the
  expectation is wrong before changing either.
- A new test must fail when its feature is broken. Spot-check that by
  breaking the code once, then restore it.

## The layers

| Layer | Where | Catches |
|---|---|---|
| Unit tests | `#[cfg(test)] mod tests` in `src/*.rs` | logic of pure pieces: cascade matching and property mapping, entity decoding, Fluent args, font fallback, the rebuild state machine |
| Test vectors | `tests/html_ui.rs`, `tests/vectors/<name>/` | the HTML/CSS/Fluent/Tera → Bevy world mapping, through the real pipeline, headless |
| CSS oracle | `browser_oracle` test + `browser.json` | bevy_markup's computed styles vs headless Chromium |
| Layout oracle | `layout_oracle` test + `layout_*` vectors | Bevy UI's real layout rects vs Chromium's |
| Fluent oracle | `fluent_oracle` test + `fluent.html` | bevy_markup's localization vs `@fluent/dom` |
| Metamorphic properties | `tests/properties.rs` (proptest + test-strategy) | relationships that must hold for any input |
| Layout properties | `tests/layout_properties.rs` | flexbox invariants over generated sizes |
| Reference models | `tests/quickcheck.rs` | cascade results vs an independent precedence model |
| Robustness + structure | `tests/arbtest.rs` | arbitrary bytes never panic or wedge; generated documents match an HTML model |
| State machine | `tests/stateful.rs` (proptest-stateful) | rebuild/restyle bugs over random runtime operation sequences |
| Content lint | `tests/content_lint.rs` | problems in the examples' content (`examples/assets/`) |
| Golden images | `tests/golden.rs` (`#[ignore]`d) | real rendering: glyphs, wrapping, 9-slice drawing |
| Fuzzers | `fuzz/`, `honggfuzz/`, `fuzzcheck/`, `test-fuzz/` | panics and hangs in parsing/glue code |
| Mutation testing | `scripts/mutants.sh`, `.github/workflows/mutants.yml` | gaps in all of the above |
| Coverage | `scripts/coverage.py`, nightly `coverage` job | which library lines each layer reaches |

The bugs found so far (`docs/agents/bugs/INDEX.md`) came almost entirely from
the generated-input layers and the oracles, not from hand-written examples.
New features should get a vector *and* a property or oracle case.

## Choosing where a test goes

- **Pure function or a small module** (CSS property → Bevy value, entity
  decoding, the rebuild decision): a unit test next to the code. These run in
  milliseconds and pinpoint the arm that broke. Table-driven tests (one row
  per CSS value) work well for mappings.
- **A mapping visible in the built UI** (a block's runs and styles, a node's
  box): a test vector in `tests/html_ui.rs`.
- **Anything CSS can express**: prefer an oracle vector, so Chromium supplies
  the expected values instead of you.
- **Localization behaviour**: a Fluent oracle vector. Deliberate differences
  from fluent-dom get a hand-written vector instead (see
  `fluent_permissive_markup`).
- **"For any X, Y holds"** (shorthand = longhands, restyle = fresh build, no
  overlap after wrap): a property in `properties.rs`, `layout_properties.rs`
  or `quickcheck.rs`.
- **Runtime sequences** (swap theme, remove an override, toggle the outline,
  in any order): an op in `tests/stateful.rs`.
- **Crash resistance of a parser or glue function**: a fuzz harness in
  `src/fuzz.rs` (and the arbtest robustness properties end to end).
- **Real pixels**: a golden scene, sparingly — it's the most brittle layer.

## The harness: `TestUi` (`tests/common/mod.rs`)

Every pipeline-level test uses `TestUi`. It builds a headless Bevy app
(`MinimalPlugins` + assets + images + `BevyMarkupPlugin`) with a temporary asset
root, no window and no renderer.

```rust
let mut ui = TestUi::new("name", &[("page.html", page), ("style.css", css)])
    .stylesheet("style.css")                  // DefaultStylesheet
    .locale("locales/en-US/main.ftl.ron")     // ActiveLocale (optional)
    .spawn("page.html", TemplateContext::new(), Node::default());
ui.settle().assert_dump(r#"
html-ui
  p
    "Text" serif 20px #ffffff
"#);
```

- `new(name, files)` writes the files (plus `frame.png`, a 32×24 fixture)
  into a fresh temp dir. Fonts are fake handles labelled `serif`,
  `serif-bold`, `serif-italic`, `serif-bold-italic` (family `Spectral`) and
  `mono` (family `Mono`, also `monospace`); anything else shows as `default`.
- `with_layout(name, files, viewport)` adds real Bevy UI layout and text
  measurement against a fixed-size camera. Text then uses Bevy's embedded
  default font (FiraMono, printable ASCII only); no fake families.
- `from_vector(name)` / `from_layout_vector(name, viewport)` load
  `tests/vectors/<name>/`. A `messages.ftl` there becomes the en-US bundle.
- `settle()` updates until every tracked asset is loaded or failed, a new
  update (build or restyle) happened, and nothing changed for 5 frames. It
  panics after 3000 frames with the current dump. Use `settle_quiet()` when a
  change legitimately causes no update. Use `update(n)` + `builds()` to check
  that something does *not* happen.
- `dump()` is one line per entity: `label [border=t,r,b,l] [padding=…]
  [margin=…] [gap=N] [overflow=x,y] [bg=#hex] [slice=file t,r,b,l mode]
  [visual-box=…]`, then for `Text` nodes one indented line per run
  (`"text" face size color`). Labels are `tag#id.class…`, `-` for anonymous
  nodes, `html-ui` for the root.
- `layout_dump()` prints `label x,y wxh` per node (needs `with_layout`);
  `node_rect(world, entity)` gives one rect.
- `load::<T>(path)` tracks an asset for `settle()`. `world_mut()`, `root()`
  and `with_file(path, bytes)` cover the rest.

Pitfalls:
- `settle()` never returns if a stylesheet's `url()` image is missing (the
  sheet loads, its dependency fails). Check the sheet first, as
  `content_lint` does.
- Formatting: `cargo fmt` before committing (AGENTS.md, Conventions);
  it formats the shared harness too, which is fine.
- Each file in `tests/` is its own test binary and can't see the others'
  code: shared helpers go in `tests/common/mod.rs`, marked
  `#[allow(dead_code)]` (not every binary uses every helper).

## Writing test vectors (`tests/html_ui.rs`)

A vector is inputs plus the expected dump. Keep inline vectors small. Put
anything an oracle should check in `tests/vectors/<name>/`: `page.html`,
`style.css`, optionally `messages.ftl`.

Steps:
1. Write the inputs.
2. Derive the expected dump by hand. For layout, FiraMono advances 0.6 em
   per character and lines are 1.2 em tall (20 px text: 12 px per character,
   24 px lines).
3. Run it. If it fails, find out who is wrong before editing anything.
4. Break the feature once and check that the vector fails.

## The oracles

The oracles replace "the expected value comes from the same head as the code"
with an independent reference. Their outputs are committed, so running the
tests needs no browser or Node.

### CSS and layout oracle (`scripts/browser_oracle.py`)

The script loads each vector in headless Chromium as `* { all: unset }` +
`BEVY_MARKUP_CSS` + the vector's `style.css` + `page.html`, and records
`getComputedStyle` per element (and `getBoundingClientRect` for `layout_*`
vectors) into `browser.json`.

- `BEVY_MARKUP_CSS` expresses bevy_markup's own defaults and layout model as CSS: root and
  containers are flex columns, blocks are blocks, nothing shrinks, white
  text in Bevy's default font at line height 1.2, the `li` indent and
  bullet, `pre` with 8 px padding. Keep it in sync with `src/build.rs`.
- Vectors must be plain HTML (no Tera syntax, no `data-l10n-id`). Set
  `color` on `html` (browsers default to black) and `border-style: solid`
  wherever border widths matter (bevy_markup ignores `border-style`).
- Layout vectors (`layout_*`) must not set `font-family`, must be ASCII
  only, and should use font sizes whose 1.2 line height is a whole number:
  Bevy rounds every text node up to a whole pixel, browsers don't.
- `browser_oracle` compares per character (face, size, color) and per
  root/block/container box. `layout_oracle` compares rects within 1 px.
  Deliberate differences are listed above `FIXTURE_SIZE` in
  `tests/html_ui.rs`; add new ones there with a reason.
- After changing a vector: regenerate, review the `browser.json` diff (one
  record per line), commit it.

### Fluent oracle (`scripts/fluent_oracle.sh`)

The script runs `@fluent/dom` in jsdom over each vector's `page.html` +
`messages.ftl` and writes the translated DOM to `fluent.html`, with
fluent-dom's warnings as comments at the top. The `fluent_oracle` test
requires bevy_markup's localized build to equal bevy_markup's build of `fluent.html`, and
prints a line diff (`-` fluent-dom, `+` bevy_markup).

Deliberate differences (no sanitizing of translation markup, escaped string
arguments, fluent-rs number formatting) stay out of oracle vectors and are
pinned by hand-written vectors.

## Property tests

All property tests run on every `cargo test`, so keep case counts low (the
whole suite should stay around 20–30 s). Name each test after the property
and say in its doc comment which bug class it catches.

### proptest + test-strategy (`tests/properties.rs`, `tests/layout_properties.rs`)

```rust
#[proptest(cases = 24)]
fn padding_shorthand_equals_longhands(
    #[strategy(0u8..40)] t: u8,
    #[strategy(0u8..40)] r: u8,
) {
    // build two TestUis, prop_assert_eq! their dumps
}
```

- Prefer **metamorphic** properties: two inputs that must produce the same
  world (shorthand vs longhands, a restyle vs a fresh build, a selector list
  vs expanded rules). They need no expected values.
- Layout properties assert invariants (mirroring, centring, ratios, no
  overlap) with a 1 px tolerance.
- Failures are shrunk and saved to `tests/<file>.proptest-regressions`
  (integration tests) or `proptest-regressions/<module>.txt` (unit tests).
  Commit those files: proptest replays them first on every run.
- More cases for one run: `PROPTEST_CASES=500 cargo test --test properties`.
- proptest is pinned to `=1.5.0` (proptest-stateful needs it); test-strategy
  stays at 0.3.1.

### quickcheck (`tests/quickcheck.rs`)

Use it for structured inputs checked against a small independent reference
model (`cascade_winner_matches_precedence_model`,
`selector_lists_cascade_per_element_and_property`). quickcheck 1.1's `Gen`
RNG is private: build `Arbitrary` impls from `T::arbitrary(g)` and
`g.choose`. The `#[quickcheck]` attribute comes from `quickcheck_macros`.

### arbtest (`tests/arbtest.rs`)

- **Robustness**: arbitrary bytes, lossily decoded, as HTML, CSS, FTL and
  `data-l10n-args`. The pipeline must settle and always leave output (content
  or the `failed to render:` paragraph).
- **Structure-aware**: `generated_documents_build_the_html_model` generates
  well-formed documents and compares the built tree against an HTML model.
- Each property runs for a time budget (`.budget_ms(400)`). On failure,
  arbtest prints a seed: replay with `.seed(0x…)` on the same property while
  debugging.

### proptest-stateful (`tests/stateful.rs`)

Random sequences of runtime operations (`SetN`, `SetTheme`, `SetOwnSheet`,
`RemoveOwnSheet`, `SetLocale`, `ToggleOutline`, `SwapFonts`) against one
long-lived `HtmlUi`, each checked against a reference model after it settles.

To add an operation:
1. Add the variant to `Op` and a generator in `op_generators`.
2. Update the model in `next_state` and the expected dump.
3. Add a precondition if an op could legitimately cause no update.
4. Apply it in the system-under-test runner, then `settle()` (or
   `settle_quiet()` when no update is expected).

Shrinking only removes operations; it doesn't shrink values inside one.
Some failures are timing-dependent (one was 1 in 3): stress-run with
`PROPTEST_CASES=200 cargo test --test stateful` a few times.

### The windowing property (`tests/windowing_model.rs`)

A windowed build (`HtmlWindowed`) against an unwindowed build of the same
document, the crate's own full build as the oracle. `Pair` drives both
in lockstep: `flag` flips the `{% if f %}` rows, `value` sets `{{ v }}`
(short, or long enough to wrap), and `scroll` scrolls both and checks
every one of the next six frames:
- placement: built id'd elements sit where the full build has them (1 px);
- coverage: the viewport's elements are built (exempt: the first frame
  after a content change, bug_0170's known limit; `lag_exempt = false`
  for a test whose change leaves every known height true);
- unique `WindowKey`s;
- tightness, at the last frame: nothing built outside the band
  (`tight = false` where a grid or row is never culled inside by design;
  `tight_px`, default 31 — the largest generated margin plus 1 px — is
  1 for margin-less pages, where a row left behind by one step shows).

`tests/html_ui.rs`'s `tightness_errors` is the same check for the
`review_0006` helpers (margin 0 only).

`case(seed)` generates a document and eight steps from a seed (xorshift,
so a seed reproduces). For a failure, take the seed's page and steps from
the panic message and minimize them. Make the page a file the test reads,
and let a script remove top-level and section-level chunks while the
failure persists (no rebuild per try). The minimized case becomes a named
`Pair` test. Run it widely after windowing changes:
`PROPTEST_CASES=400 cargo test --release --test windowing_model` (~85 s).
The mutation run over the windowing diff (review_0006's follow-up) is
`scripts/mutants.sh` with `--cargo-arg=--test=windowing_model` in place
of `stateful` and `--in-diff` of `git diff 902d81c -- src/build.rs`, at
`PROPTEST_CASES=16` (~30 min, 4 jobs).

### Unit tests and the rebuild state machine

The decision of when to rebuild or restyle is a pure function
(`RebuildState::decide` in `src/rebuild.rs`). Test new update triggers there,
not through full apps: `rebuild::tests` checks every load-state combination
and a proptest over frame sequences ("no change is lost, none is invented").
Proptest works inside `#[cfg(test)]` modules too.

## Content lint (`tests/content_lint.rs`)

It checks the examples' content in `examples/assets/` (committed): locale message parity, templates
render with the examples' data, every `data-l10n-id` resolves, no hard-coded
text, CSS parses and its `url()`s exist, and a generated pseudo-locale
(`en-XA`) leaves no untranslated text.

New template: add it to `PAGES`. A deliberately unlocalized template gets
`unlocalized: Some(reason)`.

## Golden images (`tests/golden.rs`, `scripts/golden.sh`)

Three scenes in `tests/golden/<scene>/` are rendered offscreen by full Bevy
and compared with `expected.png`. The script pins Mesa's lavapipe (software
Vulkan, bit-identical run to run); on Arch without it installed, it fetches
the matching package into `target/golden-lavapipe/`.

- A pixel differs above channel delta 8; a scene fails above 0.05%
  differing pixels. One changed letter is about 0.1%.
- Failures write `target/tmp/golden/<scene>.{actual,diff}.png`.
- `scripts/golden.sh --update` rewrites the references. Look at the PNGs
  before committing them.
- The canonical renderer is CI's runner (2026-10-08): when a runner image
  change fails the nightly's `Golden images (lavapipe)` job, dispatch
  `Update golden references (lavapipe)` in `nightly.yml`, download its
  `golden-references` artifact (`gh run download <run> -n
  golden-references`), copy the PNGs over `tests/golden/<scene>/`, and
  commit.
- Use only committed inputs: Bevy's default font and `tests/fixtures/`.
- Keep it to a few scenes: it's a smoke check, not a spec.

## Fuzzing

All fuzzers drive the `#[doc(hidden)]` `bevy_markup::fuzz` harness (feature
`fuzzing`), which calls the internal glue directly: `render_html`,
`cascade`, `translate`. Contract: arbitrary input may return `Err` or any
output, but must never panic, hang or abort.

Input formats (bytes are decoded lossily, capped at 64 KiB):
- `html`: template source, optionally `\n---\n` + a JSON context.
- `css`: the whole input is a stylesheet.
- `ftl`: `FTL body\n---\nmessage id\n---\nargs JSON`.

| Driver | Run | Notes |
|---|---|---|
| cargo-fuzz (libFuzzer + ASan) | `scripts/fuzz-libfuzzer.sh <target> [s]` | the main one; also nightly in CI. Passes `--target` (a prebuilt cargo-fuzz defaults to musl, which ASan rejects) |
| honggfuzz | `scripts/fuzz-honggfuzz.sh <target> [s]` | vendored, patched crate; needs the linker override the script sets |
| fuzzcheck | `scripts/fuzz-fuzzcheck.sh <target> [s]` | vendored, patched for LLVM 21+ coverage records |
| test-fuzz (AFLplus) | `scripts/fuzz-test-fuzz.sh <target> [s]` | corpus seeds come from plain `cargo test` runs |

All need a nightly toolchain. Crashes and working corpora live in gitignored
directories (`fuzz/corpus/`, `fuzz/artifacts/`, `hfuzz_workspace/`,
`test-fuzz/target/`).

The cargo-fuzz targets also read committed seeds, `fuzz/seeds/<target>/`
(minimized, read-only: libFuzzer writes new inputs only to the first
corpus directory, `fuzz/corpus/<target>/`). First seeded 2026-10-05 from a
local corpus (2940 html, 4868 css, 1181 ftl inputs; 580 KB). To refresh
them, merge the seeds, your local corpus and CI's latest corpus into a
fresh directory and replace the seeds (inputs are named by content hash,
so git only stores the new ones). CI's corpus is the nightly fuzz job's
`fuzz-corpus-<target>` artifact (kept 90 days; the cache it also saves
can't be downloaded):

```sh
cd fuzz && host=$(rustc +nightly -vV | sed -n 's/^host: //p')
run=$(gh run list --workflow nightly.yml --status success --limit 1 --json databaseId -q '.[0].databaseId')
for t in html css ftl; do
  rm -rf /tmp/ci-$t /tmp/seed-$t && mkdir -p /tmp/seed-$t
  gh run download "$run" -n fuzz-corpus-$t -D /tmp/ci-$t
  cargo +nightly fuzz run --target "$host" $t /tmp/seed-$t seeds/$t corpus/$t /tmp/ci-$t -- -merge=1
  rm -rf seeds/$t && mv /tmp/seed-$t seeds/$t && rm -rf /tmp/ci-$t
done
```

The nightly coverage job runs after the fuzz jobs (`needs: fuzz`, also
when one failed) and restores the corpus caches they just saved, so its
fuzz-corpora layer measures seeds plus everything CI has found so far.

To add a fuzz target: add the glue function to `src/fuzz.rs`, then a target
per driver. Regression tests for fuzz-found crashes go in `src/fuzz.rs`'s
tests (run with `--features fuzzing`) or next to the fixed code.

## Mutation testing (`scripts/mutants.sh`)

cargo-mutants makes small changes to the library code and reports any that no
test catches.

- `scripts/mutants.sh` runs unit + `html_ui` + `stateful` (the same set as
  the CI run); `--full` adds the property suites; `--file src/x.rs`
  narrows it. Results land in `target/mutants.out/`
  (`caught/missed/timeout/unviable.txt`); reruns skip caught mutants.
- It's fast because of three measured settings (see `[profile.mutants]` in
  `Cargo.toml`): only the tests that run are built (`--cargo-arg`, not
  `--cargo-test-arg`, which reaches only the test run), no debug info with
  optimized dependencies, and Bevy linked dynamically. A mutant costs about
  4–8 s build + 0.5 s test after a one-time ~150 s dependency build per job.
- The script moves the fuzz drivers' build caches aside during the run
  (cargo-mutants copies the tree and ignores nested `.gitignore`s) and puts
  them back on exit.
- `-j` and `--minimum-test-timeout` are set by the script: use
  `BEVY_MARKUP_MUTANTS_JOBS` and `CARGO_MUTANTS_MINIMUM_TEST_TIMEOUT`.
- To rerun specific survivors: `--re '^(src/build\.rs:625:9|…):'` anchored on
  `file:line:col`. cargo-mutants 27.1 doesn't apply `--re` to
  struct-field-deletion mutants; they always run.
- Under heavy parallel load a mutant can hit the test timeout without being
  a hang. Rerun timeouts alone with a longer limit before triaging.
- For one change set: `--in-diff <file>` restricts the mutants to the
  diff's lines (see "Methods from review_0005", 9). The script's
  `bevy/dynamic_linking` needs the `bevy_dylib` crate in the registry;
  offline, run a copy without it.

Triage every survivor: write a test that catches it, or document in
`AGENTS.md` (Testing TODO 6) why the mutant changes no behaviour (typically:
it only changes whether a `debug!` line is logged). Copy survivors into
`AGENTS.md`: `target/mutants.out/` is overwritten by every run. For the
twice-weekly CI run: `gh run download <run-id>` and read each shard's
`missed.txt`.

## Benchmarks (`benches/`)

Criterion (`benches/pipeline.rs`) and Divan (`benches/pipeline-divan.rs`)
over the update pipeline (ADR 0018/0019). The workloads: idle (nothing
changes), changing (one value per frame), rebuilding (every element
changes), localized and markdown (a 600-section localized Markdown
document), plus the pure stages (render, parse) and the interaction
harnesses (hover, focus navigation). Read the two together: Criterion for
times, Divan for the allocation counts that explain them.

Cross-session drift is real (±10–27% was measured on the pure
`render`/`parse` stages during the ADR 0021 work) — performance claims
need a same-session A/B (the `frame/*` filter + the two-binary
on/off pattern), not a comparison against an old stored baseline.

## Coverage (`scripts/coverage.py`)

Source-based line coverage (`cargo-llvm-cov`) of the library code in `src/`,
per testing layer and merged. Each layer runs alone; the table shows the
lines it covers and the lines *only* it covers (what dropping that layer
would lose). Layers: `unit`, `vectors+oracles`, `properties`, `arbtest`,
`stateful`, `content-lint`, `fuzz-corpora`.

```sh
scripts/coverage.py                 # table + target/coverage/uncovered.txt
scripts/coverage.py --html          # + target/coverage/html/index.html
scripts/coverage.py --layer stateful --layer unit
```

- Needs `cargo install cargo-llvm-cov` and
  `rustup component add llvm-tools-preview`.
- `src/fuzz.rs` and `#[cfg(test)]` modules are excluded: they aren't library
  code (test modules must stay at the end of their file for this).
- The `fuzz-corpora` layer replays the local cargo-fuzz corpora through the
  same harnesses (`tests/fuzz_corpus.rs`, `#[ignore]`d, feature
  `fuzzing`). It covers what fuzzing has reached so far: the committed
  seeds everywhere, plus the local working corpus where there is one.
- Golden images and doc tests aren't measured.
- Read `uncovered.txt` for gaps, but remember that coverage only shows a
  line *ran*, not that a test checked its result. Mutation testing measures
  checking. They complement each other: mutants can't target an arm of an
  exhaustive `match`, coverage shows whether that arm ever runs.

## Discovering bugs: the workflow

1. **Find a candidate.** A failing property, an oracle difference, a fuzz
   crash, a mutation survivor, or a stateful sequence.
2. **Reproduce it deterministically.**
   - proptest: the shrunk case is printed and saved to the regressions file.
   - arbtest: replay the printed seed.
   - cargo-fuzz: `fuzz/artifacts/<target>/crash-…`; minimize with
     `cargo +nightly fuzz tmin <target> <artifact>`.
   - stateful: the minimal op sequence is printed; replay it as a small
     throwaway test if it's timing-dependent.
   - oracle diff: the failure names the element and both values.
3. **Decide who's wrong.** Oracle and spec beat bevy_markup unless the difference is
   deliberate. If it is, list it as an allowed difference with a reason.
4. **Write the regression test first**, watch it fail, then fix the code.
   Prefer the fastest layer that catches it (a unit test over a full app).
5. **Check the fix doesn't just move the bug.** Run the whole suite; for
   rebuild/load-state bugs also stress the state machine.
6. **File the bug report** (below).

Typical bug classes found so far, and the layer that found them:
- Change detection and rebuild triggers (removal events, failed or reloading
  assets, signals lost while loading): proptest-stateful.
- Byte slicing across multi-byte characters: the fuzzers.
- Semantic mismatches with CSS/Fluent (font fallback, `box-sizing`, missing
  overlays): the oracles.
- Whitespace and document order: the structure-aware arbtest model.
- Coverage gaps and a wrapping bug: mutation-testing triage.
- Stale reuse after a sibling change, quadratic sibling lists, results
  that depend on document order, lost content reuse: the methods below
  (review_0005).

## Methods from review_0005 (and how to apply them)

What found or would have caught the review_0005 defects, in the order to
reach for them:

1. **Falsify every guard.** A test that cannot fail is not a test. Break
   the code it guards (revert the fix, or ablate the feature by hand) and
   watch it fail; only then trust its green. Ones that passed with the
   bug present, all found this way:
   - `sibling_flip_restyles_the_following_sibling` asserted only the end
     state, and its hover never held: picking resets a hand-inserted
     `PseudoState` (bug_0148).
   - The testbench's `reuse` check read `pairing_health()`, which starts
     at `(true, 0)`, so it passed with no pairing at all. It now reads
     `last_reuse()`.
   - Its order test gave each row an `id`, which keys the cache apart and
     hides I06.
   - Its sibling rule sat away from the subject (`.a + .b .k`), which
     skips the rule, so the rule never matched.
   - Its budget used the virtual clock, which clamps every frame to 250 ms.
   - Its `overflow: auto` probe flipped a `border-radius`, which
     `ComputedNode` carries, so the promotion re-ran regardless.
2. **Compare the incremental path with a from-scratch one.** A reused,
   restyled or keyed UI must equal a fresh build of the same state:
   `spawn_two` + `assert_same_after`, `reuse_and_fresh`, the testbench
   twins. Use shapes where the shortcut fires: equal child counts (a
   length change disables the pairing), elements nested in a container
   (bug_0150), and sibling- and position-dependent rules.
3. **A fresh twin shares matching bugs: use a transform or a model.**
   - A metamorphic transform changes the input in a way whose effect on
     the output is known. Reversing order-independent rows must leave each
     row's result alone; that catches a cache keyed on too little (I06),
     which a same-order twin reproduces exactly.
   - A reference model: `tests/selector_model.rs` matches random small
     documents × random selectors against a hand-written matcher (it found
     I06, I07 and I08 at once).
   - Give the model elements that differ *only* in what the feature reads:
     no ids, repeated classes, the same position in different parents.
4. **Assert the optimization happens, not just the result.** Lost reuse is
   invisible to result comparisons, because a build that reuses nothing
   gives the same result. Assert `Skeleton::last_reuse()` / `pairing_health()`.
   A benchmark only guards the shapes it renders: bug_0150 (no reuse inside
   containers) hid behind a top-level-only workload until
   `changing-nested` was added.
5. **Scaling, not wall time.** Take the best of three at N and at 4N; the
   larger must cost at most 8× the smaller (I03, I04 were quadratic). In an
   app, measure with `Time<Real>` and hide the UIs not being measured.
6. **Quiet when idle.** An idle UI fed identical inputs must stop building,
   restyling and writing `Node`s. That catches change loops (a write that
   re-triggers its own system: G15, I09's promotion).
7. **Lint the source** for what keeps shipping (`tests/source_lint.rs`:
   debug prints, file writes outside tests). Three debug prints and a
   `/tmp` log file shipped before the lints existed.
8. **Probe, then delete.** To learn which path runs, add a temporary
   `eprintln!` at the gate (`GATEPROBE …`) and run the one test with
   `--nocapture`. The lint makes sure the probe never ships.
9. **Mutation-test the diff.** Run `cargo mutants --in-diff <diff>` in a
   clean worktree. Failing reproduction tests would break its baseline, so
   apply only the passing tests there. Put new unit tests where the diff's
   hunks don't move (or regenerate the diff), and add test binaries with
   `--cargo-arg=--test=<name>`. Every surviving mutant is a test to write
   or an equivalent to document.

## Bug reports

Every real defect gets `docs/agents/bugs/bug_NNNN.md` (next number in
discovery order) and a row in `docs/agents/bugs/INDEX.md`. Include:
- status and severity (reachable from user data = high, even if it only
  panics), component, symptoms (what's observable, not the cause)
- discovery: tool or target, seed or op sequence, commit, agent/model, date
- a minimal reproduction (minimized, not a raw fuzzer artifact)
- root cause, fix (with commit), and the regression test that guards it
- for upstream bugs: affected and pinned versions, and where the local patch
  is wired (`[patch.crates-io]` + a fork branch or a vendored copy)

Use the existing reports as templates.

A defect in a dependency or tool (whether found as a bevy_markup bug or while
setting up a tool) also gets an entry in `docs/agents/bugs/UPSTREAM.md`:
reproduction, the local workaround and where it lives, a PR sketch, and a
status from unreported to fixed upstream. That's the list to work from when
reporting upstream or removing a vendored patch.

## CI

- `.github/workflows/ci.yml` (every push and PR, blocks merges): the
  warning-free checks, `cargo test`, the `fuzzing`-feature tests, and that
  `fluent.html` matches what the script generates.
- `nightly.yml`: golden images on Ubuntu's lavapipe (the canonical
  renderer; a manual `Update golden references (lavapipe)` job,
  workflow_dispatch only, re-records the references on it), the browser
  oracle against the runner's Chrome, 15 min of cargo-fuzz per target (RSS limit raised to 4 GB), and builds of
  the other fuzz drivers. Failures are reports to triage, not blockers. The
  fuzz corpora carry over between nights (GitHub cache
  `fuzz-corpus-<target>-*`, minimized with `cargo fuzz cmin` on Sundays).
- `mutants.yml` (Sundays and Wednesdays, 4 shards): informational. Survivors are in each
  shard's job summary and the `mutants-shard-N` artifacts.
- `cache.yml` (every push to main, and daily before the schedules): builds
  dependency caches once so jobs don't each build Bevy from scratch.

Shared build caches (`Swatinem/rust-cache` `shared-key`):

| Cache | Built by | Restored by |
|---|---|---|
| `dev` | ci.yml `test` | nightly `golden`, `golden-update` |
| `mutants` | cache.yml `mutants` | the 4 mutants shards |
| `fuzz` | cache.yml `fuzz` | nightly `fuzz` (3 targets) |

Consumers restore with `save-if: false`; the producer builds only when the
exact entry is missing and otherwise just restores it, which also keeps it
from GitHub's 7-day eviction. The key is the toolchain version plus every
`CARGO*`/`RUST*`/`CC*` variable set when rust-cache runs plus the
manifests and lockfiles, so producer and consumer must agree on all of them:
change a job's `env` or toolchain only together with its pair (comments in
the workflows mark them). A consumer that misses logs a `::warning::` and
builds from scratch. The fuzz jobs use a dated nightly (`BEVY_MARKUP_NIGHTLY` in
nightly.yml and cache.yml; `scripts/fuzz-libfuzzer.sh` honours it), because
a floating nightly changes the key every day. Coverage (instrumented
`RUSTFLAGS`) and the fuzz-driver checks keep their own per-job caches;
nothing else builds the way they do.

Trigger the scheduled ones by hand with `gh workflow run nightly.yml` or
`gh workflow run mutants.yml`, and read logs with
`gh run view --job=<id> --log-failed`.
