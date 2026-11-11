# 44. Every input is untrusted; templates are bounded by a static check at load

| Field | Content |
|---|---|
| `ADR` | `0044` |
| `Title` | Every input is untrusted; templates are bounded by a static check at load |
| `Date` | 2026-11-11 17:54 +0400 |
| `Author` | Claude Opus 5.5 (Anthropic), via Claude Code; the threat model and the static check are the user's decisions ("They are treated as untrusted. Even if they don't come from mods — defense in depth is good"; "add a static pre-check … at load time of the Tera asset … implement our own [parser] … using `nom`") |
| `Commit` | `3ce6efe File review_0007's findings as open bug reports bug_0204-0221` + uncommitted: the review_0007 fixes |
| `Status` | Accepted |
| `Related` | ADR 0034 (selector allowlist), ADR 0038/0040 (SVG bounds), ADR 0039 (CSS pre-scan); bug_0209, bug_0213, bug_0220; review_0007 K06, K10, K16; UPSTREAM U39 |

## Context

The crate bounded some inputs as hostile (CSS nesting, DOM depth, grid
tracks, Tera operator chains, SVG) and not others, and nothing said which
inputs were untrusted (review_0007 K16). README promised it for SVG files
only. Three findings depended on the answer: nested `:is()` selectors
costing depth^levels (bug_0213), the sum of SVG rasters (bug_0209) and
template output (bug_0220).

Tera 2.4 bounds `range` (10⁵ items), component recursion (20) and its
parser (40 levels, 256 operations per expression), but not a render's
work or memory. Probed: two nested `range(end=100000)` loops printing
nothing run for minutes; `{% set_global s = s ~ s %}` in a 40-step loop,
or twenty unrolled `{% set s = s ~ s ~ s ~ s %}`, ask for a terabyte and
abort the process; `[a, a]` nested in a loop is exponential when written.

## Decision

1. **Every input is untrusted**: templates (HTML and Markdown), CSS, FTL,
   SVG and images, whether they ship with the app or come from mods. Each
   gets bounds on work and memory, refusing what passes them with an
   error naming the limit. The app's `TemplateContext` data is the app's:
   its size is trusted.
2. **Templates: a static check at load** (`src/tera_check/`), once per
   load and hot reload, after Tera compiles the set (`template.rs`
   `load_template`; the fuzz harness runs it too). Tera's parser is
   private, so the module ports Tera 2.4's lexer (`nom`) and parser
   (precedence included) and runs an abstract render over bounds: each
   value's written size, its item count, one item's size and its numeric
   magnitude, as `c + k·A` (`A`: one app value, `k` copies of it). A loop
   over app data counts one iteration (bounds are per app element); a loop
   over template-made values multiplies by its bound (`range` with literal
   or computed arguments, literals, `split`). `set_global` in a loop is
   run once with symbols: each carried value must be a constant plus at
   most one carried value, so it grows linearly, and is bounded by start +
   iterations × that constant. Includes, `extends` with blocks and
   `super()`, and components are evaluated inline; recursion is refused.
   Limits: `MAX_TEMPLATE_STEPS` (2²⁵, ~0.45 s of release rendering),
   `MAX_TEMPLATE_VALUE_BYTES` (16 MiB per built value),
   `MAX_TEMPLATE_APP_COPIES` (4 096 copies of one app value). A template
   the check can't read, or that uses a filter or function it doesn't
   model, fails the load (fail closed).
3. **Rendered output** stays capped at render time
   (`template::MAX_RENDERED_BYTES`, 16 MiB), behind the check.
4. **CSS**: a combinator may appear inside one level of `:is()`/
   `:where()`/`:not()`, not in an argument nested in another (bug_0213).
5. **SVG rasters**: `svg::MAX_SVG_RASTER_BYTES` (256 MiB) per cache
   (display-size redraws, inline images; bug_0209).

## Alternatives considered

- **Vendor Tera with render limits** (an instruction budget, a value-size
  cap and byte accounting in its interpreter; tried in this session):
  bounded everything at run time, but cost 13% of render time until
  batched (then ~10%), and `[patch]` doesn't reach crates that depend on
  bevy_markup, so their apps would run unpatched Tera. Removed in favour of
  the check, which costs nothing per render.
- **Render on a background task with a timeout**: can't stop the work or
  a failed allocation, only hide the frame drop.
- **Bound only the output** (the writer cap alone): misses loops that
  print nothing and values built in memory (`set`, captures).
- **Trust templates and CSS** (document it): the answer the user rejected.

## Consequences

- The project's templates pass unchanged (`content_lint` loads every
  example page through the loader).
- Some valid templates are refused: a `set_global` value that goes
  through a growing filter in a loop (`s | replace(…)`), or two carried
  values feeding each other; a filter combining two context values
  (`name | replace(from="", to=name)`). The error names the variable or
  the limit.
- The check ports Tera's grammar: a Tera upgrade can add syntax the check
  refuses (fail closed). Re-run `tera_check` tests and the fuzz targets on
  every Tera bump; `the_check_reads_what_tera_compiles` compares the two
  parsers on generated templates, `accepted_templates_render_within_the_bound`
  compares the bound with Tera's real output.
- A loop over app data isn't bounded by the check: an app sending 10⁶ rows
  renders 10⁶ rows, by its choice.
