# review_0009 — Code review 2026-11-12

| Field | Value |
|---|---|
| ID | review_0009 |
| Commit reviewed | `9811ccc5bf16ea4e1ca57bd3052fe21d4fc79975` |
| Reviewer | Codex |
| Toolchain | rustc 1.99.1 (58cf8f9c3 2026-10-13), Linux |
| Scope | Repository review, concentrating on untrusted input bounds and UI interaction correctness after review_0008 |
| Result | Five confirmed findings: two high-severity memory-bound bypasses, two quadratic-work problems, one focus/activation defect |
| Changes | This report only; no implementation fixes |

## Coverage and method

Consulted the previous reviews (the findings inventory of reviews 0001–0006,
and the detailed findings and disposition of 0007–0008), Cargo features and
CI, and ADRs 0044 and 0046. Examined the template abstract interpreter's
expression, filter, map, loop, growth and control-flow handling against the
installed Tera 2.4.0 source. Also inspected focus selection and activation,
anchor placement, the Fluent loader and Markdown cache, the Fluent
checker's argument scan, the CSS loader's guarded parsing, and the SVG
check's stylesheet work accounting.

Probes used two paths:

- A temporary release-mode executable imported the repository's actual
  `tera_check/mod.rs` and `ftl_check.rs` by absolute path, with Tera 2.4.0
  and nom 8. It printed check reports and timed bounded real renders.
- Temporary integration tests used `tests/common::TestUi`, the actual asset
  loaders, and `HtmlFocus`. These confirmed M01, M02, M04 and the load path
  for M05. The temporary integration file was removed afterwards.

The unsafe-sized variants below were **checked but never rendered**.
Actual renders deliberately stopped at tens of megabytes. Memory-abort
consequences are inferred from the demonstrated exponential growth, not
from deliberately exhausting this machine.

This was not an exhaustive audit of every source line. No new fuzz campaign,
mutation run, browser comparison, GPU golden run, or windowed example run
was performed. SVG's full renderer cost model, dependency vulnerabilities,
and all platform/feature combinations were not independently audited.
Passing the tests below does not establish those properties.

## Validation

All commands ran offline against the installed dependency set.
Test commands used `CARGO_PROFILE_DEV_DEBUG=0 RUSTFLAGS='-D warnings'`.

| Command | Result |
|---|---|
| `cargo check --lib --offline` | Pass, minimal library features |
| `cargo check --lib --features text-input --offline` | Pass, warnings denied |
| `cargo test --lib --tests --offline` | Pass: 541 passed, 2 ignored, including the configured grid-names example test |
| `cargo test --lib --features fuzzing --offline` | Pass: 281 passed; includes SVG/text and the fuzz-harness unit tests |
| `cargo test --features text-input --lib --test signals --offline` | Pass: 218 library tests and 32 signal tests |
| `cargo doc --no-deps --offline` | Pass, Rust and rustdoc warnings denied |
| Temporary loader/focus probes | Pass: 3 tests, confirming the defective behavior described below |

The ignored baseline tests were the GPU golden test and the documented
parley inline-image line-metrics case. The full CI command matrix was not
repeated. Baseline and feature tests passed despite all five findings.

## Findings

### M01 · high · `continue` hides exponential `set_global` growth from the safety check

**Location:** `src/tera_check/analysis.rs:678`, with the final-environment
growth calculation at `:926`.

`nodes_in` treats `Break` and `Continue` as zero output and keeps executing
the rest of the statement list in its abstract environment. `growth` then
judges only that environment's final values. Consequently an assignment
that Tera never reaches can erase the evidence of exponential growth.

Reproduction, requiring no context data:

```jinja
{% set_global s = 'x' %}
{% for i in range(end=25) %}
  {% set_global s = s ~ s %}
  {% continue %}
  {% set_global s = '' %}
{% endfor %}
<p>{{ s | length }}</p>
```

The real loader accepts this and the UI displays `33554432`: a 32 MiB
intermediate string, beyond `MAX_TEMPLATE_VALUE_BYTES` (16 MiB). The
rendered HTML is tiny, so `BoundedOutput` provides no protection.

In the release checker probe, the 20-iteration version was accepted with
437 modeled steps and rendered `1048576`. Changing the count to 50 was
also accepted, with 1,824 modeled steps. Actually rendering that version
would attempt to build a 2^50-byte string (1 PiB); it was not run.

**Impact:** a small hostile template bypasses both the growth restriction
and value-size bound and can abort the process through allocation failure.
This is a different bypass from review_0008 L02's component-body traversal.

**Fix direction:** carry control-flow outcomes through statement lists and
branches. Preserve environments at continue/break exits and account for
them at the enclosing loop. Stopping at an unconditional `continue` fixes
this example, but conditional exits also need their own joined state.
Add regression cases with an unreachable reset after both unconditional
and conditional `continue` statements.

### M02 · high · Map iteration keys are bounded by the values' sizes

**Location:** `src/tera_check/analysis.rs:1196–1205` (`entries`),
`:818–826` (`loop_scope`), and `:252` (`Av::element`).

For an ordinary map expression, `entries` includes the key length in total
map bytes but only the **value** size in `out.elem`. `loop_scope` uses
that element bound for both variables of `for k, v in map`. A long key
paired with a small value is therefore modeled as a small string when
the template manipulates `k`.

Generate this template (Python):

```python
key = 'k' * 100_000
source = (
    "{% set m = {'" + key + "': 1} %}"
    "{% for k,v in m %}{% set s = k %}"
    + "{% set s = s ~ s %}" * 8
    + "<p>{{ s | length }}</p>{% endfor %}"
)
```

The checker accepts it with 7,448 modeled steps. The real asset loader
accepts it too, and the UI displays `25600000`, demonstrating a 25.6 MB
intermediate value beyond the 16 MiB limit. Twelve doublings are still
accepted (24,744 modeled steps), although the real string would be
409.6 MB. That larger variant was not rendered.

**Impact:** the template's key can seed repeated unrolled amplification
without the actual string size reaching the abstract value bound. As in
M01, printing only its length evades the output cap.

**Fix direction:** track key and value bounds separately, or conservatively
include the key length in the common element bound. Audit map spreads
and map comprehensions as well. `Av::lit(Lit::Map)` already includes key
lengths; the ordinary expression path needs equivalent protection.

### M03 · medium (DoS) · `unique` is quadratic but charged linear work

**Location:** `src/tera_check/analysis.rs:1372` and the linear byte charges
in `filter` at `:1321` and `:1407`.

The model groups `unique` with `reverse` and `sort`, retaining the input
bound and charging bytes scanned/written. Tera 2.4.0's
`src/filters.rs:615–629` implements uniqueness by calling `res.contains(v)`
for each input item. Distinct values require n(n−1)/2 equality comparisons.

```jinja
{% set x = range(end=40000) | unique %}{{ x | length }}
```

Release measurements on this machine:

| Range length | Modeled steps | Actual render |
|---|---:|---:|
| 10,000 | 80,010 | 123 ms |
| 20,000 | 160,010 | 498 ms |
| 40,000 | 320,010 | 2.02 s |

Doubling the input quadruples render time while doubling the modeled
cost. The largest example uses less than 1% of `MAX_TEMPLATE_STEPS`.
Repeating it in an outer loop multiplies that unaccounted work; output
can remain just a length, or empty. Rendering runs in the UI's render
system, so this is an application stall.

**Fix direction:** model this filter's worst-case comparisons and the cost
of comparing its elements; refuse superlinear work on context collections
unless a separate enforceable runtime limit exists. Merely changing the
returned value bound cannot fix the work accounting. Add an adversarial
filter-cost test with distinct inputs, rather than only output-size tests.

### M04 · medium · A `display:none` button receives initial focus and can be activated

**Location:** `src/focus.rs:484–500`, `:577–589`, and the initial-focus
fallback at `:303–340`.

`sync_navigation` gives elements `AutoDirectionalNavigation` according to
their root's scope/visibility. It applies `displayed_none` only when
assigning `TabIndex`. Focus repair and its fallback use the broader
directional-navigation set. `visible_in_hierarchy` reads `Visibility`,
which does not exclude `Node::display == Display::None`.

Minimal page:

```html
<button id="hidden" style="display:none" data-on-click="hidden">Hidden</button>
<button id="shown" data-on-click="shown">Shown</button>
```

A real headless `TestUi` settled this page and ran three additional
updates. `InputFocus` still named `hidden`. `HtmlFocus::tab_order()`
contained only the shown button, but calling
`HtmlFocus::activate(ActivationInput::Synthetic)` emitted `["hidden"]`.
This is an actual signal from the invisible control, not just a stale
focus decoration.

**Impact:** Enter/gamepad activation can execute an invisible action on
first load. The same eligibility mismatch also prevents focus repair from
rejecting an already-focused control solely because it becomes
`display:none`.

**Fix direction:** use a shared visibility/display eligibility predicate
for navigation membership, initial selection, remembered/autofocus
restoration and current-focus repair. Keep explicitly focused link spans
working while checking their containing nodes. Test initial hidden
controls, hidden ancestors, and hiding the currently focused control.

### M05 · medium (DoS) · The Fluent safety checker performs unbounded quadratic duplicate-argument checks

**Location:** `src/ftl_check.rs:521–548`, particularly `names.contains`
at `:542`; reached from `src/l10n.rs:212` before Fluent parses a resource.

Every named argument is compared against all previously seen names using
a `Vec`. A flat function call with distinct named arguments stays at tiny
nesting depth and passes the check, while the scan performs quadratic
work. There is no argument-count or total-work budget.

Generate the resource:

```python
n = 20_000
ftl = 'm = { FUNC(' + ', '.join(f'a{i:06}: 0' for i in range(n)) + ') }'
```

The actual checker, compiled in release mode, returned `Ok(())` for all
three cases:

| Arguments | Resource bytes | Check time |
|---|---:|---:|
| 5,000 | 60,012 | 14.8 ms |
| 10,000 | 120,012 | 59.6 ms |
| 20,000 | 240,012 | 237 ms |

The real `LocaleBundle` loader accepted the 20,000-argument resource via
`(locale: "en-US", resources: ["en.ftl"])`; the debug integration probe
took 1.78 s including app setup and settling. No function registration or
message formatting is necessary to incur the load-time work.

**Impact:** a flat, shallow FTL resource makes the safety pre-check itself
superlinear, outside its nesting protection. Increasing argument count
continues that growth; a single blocked load is off the main thread, but
many such resources can occupy asset workers.

**Fix direction:** bound arguments/work before the duplicate scan. A hash
set improves this checker but must be paired with a bound protecting the
subsequent Fluent parser, whose matching duplicate-name scan is also
linear per argument. Include wide calls in the adversarial input tests;
the current nesting and parser-equivalence tests do not establish a work
bound.

## Recommended order

1. M01 and M02: both allow oversized intermediate values and potentially
   process-wide allocation failure. Add the demonstrated loader-level
   probes as regression tests as well as abstract-interpreter tests.
2. M04: ordinary content can silently activate an invisible action.
3. M03 and M05: charge or refuse the actual quadratic operations before
   running them.

The recurring gap is that parser equivalence and rendered-output bounds
do not establish resource safety. Control-flow exits, map keys, and
built-in operation costs need explicit adversarial coverage. All findings
remain open in this report; no fixes or separate bug reports were filed
during this review.
