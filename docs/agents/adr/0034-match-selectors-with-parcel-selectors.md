# 34. Match selectors with lightningcss's selector engine (parcel_selectors)

| Field | Content |
|---|---|
| `ADR` | `0034` |
| `Title` | Match selectors with lightningcss's selector engine (parcel_selectors) |
| `Date` | 2026-10-28 18:35 +0400 |
| `Author` | Claude Opus 5.5 (Anthropic), via Claude Code |
| `Commit` | `cfab1ec Fix bug 179 and 180` |
| `Status` | Accepted (2026-10-28, after the trial; decision 2 as amended by the trial: no arena) |
| `Related` | ADR 0006, 0028, 0029, 0030; bug_0002, bug_0180; review_0005 (I03, I05–I08, I13); UPSTREAM.md U14, U15 |

## Context

**How selectors work today.** lightningcss parses the stylesheet,
selectors included, into `lightningcss::selector::Selector` values
(`parcel_selectors::parser::Selector<'i, Selectors>`). `cascade.rs`
`HtmlStyles::from_sheet` then prints each one back to text
(`selector.to_css_string(PrinterOptions::default())`) and parses that text
again with its own parser: `Selector::parse`, `Compound::parse`,
`AnB::parse`. Selectors it can't read skip their rule (logged at `debug`).
The result is bevy_markup's own types (`Selector`, `Compound`,
`StructuralPseudo`, `AttributeSelector`, `Combinator`), matched by its own
matcher (`Selector::matches`, `Compound::matches`/`matches_ancestor`)
against:

- the element (`HtmlElement`) and its `Pseudo` state;
- its ancestors as `Ancestor`s, nearest first (tag, id, classes, `Pseudo`,
  the `data-*` dataset, `Position`), threaded through the build walk;
- its `ElementState`: position among its siblings and, for the subject
  only, its preceding siblings (`previous_siblings`, a shared
  `SiblingList`).

The string round-trip dates from `cb9010f` (2026-10-04, "Style colors from
.css files"), when only `tag.class#id` was supported. The grammar grew with
each ADR 0028 chunk (structural pseudo-classes, sibling combinators,
`data-*` attribute selectors). The parsing and matching code is roughly 700
lines of `cascade.rs`.

**What it has cost.**

- Two panics of the same kind in the string parser: bug_0002 (a non-ASCII
  selector sliced across a character boundary, 2026-10-04) and bug_0180
  (`:not\0()`: lightningcss reads `not\0` as an unknown pseudo-class and
  prints it as `:not�()`; the parser read `not` and sliced into the
  3-byte `�`). bug_0180 also misread ASCII: `:not.a)` matched as
  `:not(a)`. Both come from two parsers disagreeing about CSS syntax.
- Matcher bugs found by the selector model property
  (`tests/selector_model.rs`, review_0005 I13): I06 (cache keys), I07
  (structural pseudo-classes in ancestor and sibling positions), I08
  (`:not(:hover)` never matched). ADR 0030 was needed to make compounds
  match whole in every position.

**What it can't do** (`AGENTS.md` "Limits and next steps", checked
against the code):

- sibling combinators away from the subject (ancestors' siblings aren't
  threaded), so `.a + .b p` skips its rule;
- `:is()`, `:where()`, `:not()` with a list, `:only-child`,
  `:nth-last-child()`, `:nth-last-of-type()`, `:nth-child(… of S)`;
- attribute selectors on anything but `data-*` (the element model carries
  only the dataset), and only the presence and `=` operators;
- `from_sheet` visits top-level style rules only: at-rules (`@media`
  included) and nested rules (`& > p`) are skipped. This ADR does not
  change that.

**What lightningcss already provides.** parcel_selectors 0.28.3, the
selector crate lightningcss depends on (a fork of Servo's `selectors`), has
a public matching engine:

- `parcel_selectors::matching::matches_selector(selector, offset, hashes,
  element, context, flags_setter)` and `matches_selector_list`;
- the `parcel_selectors::Element` trait, which the caller implements:
  `parent_element`, `prev_sibling_element`, `next_sibling_element`,
  `has_local_name`, `has_id`, `has_class`, `attr_matches`,
  `match_non_ts_pseudo_class`, `is_empty`, `is_root`, plus shadow-DOM,
  slot and part methods that can answer "no";
- `MatchingContext`, an ancestor Bloom filter (`bloom`), an
  `NthIndexCache`, and `ElementSelectorFlags` reported through
  `flags_setter` (`HAS_SLOW_SELECTOR`, `HAS_SLOW_SELECTOR_LATER_SIBLINGS`,
  `HAS_EDGE_CHILD_SELECTOR`, `HAS_EMPTY_SELECTOR`), which Servo uses for
  restyle invalidation;
- spec specificity (`Selector::specificity`).

**Hazards in that engine.** Its matcher panics on parts lightningcss
produces from ordinary CSS (parcel_selectors 0.28.3 `matching.rs`):

| Part | CSS | Matcher |
|---|---|---|
| `Component::NthOf` | `:nth-child(2 of .a)` | `todo!()` (line 797) |
| `Component::Has` | `p:has(> img)` | `unreachable!()` (line 816) |
| `Component::Nesting` | `& > p` (nested rules) | `unreachable!()` (line 816) |
| `Component::Nth` with `NthType::Col`/`LastCol` | `:nth-col(2)` | `todo!()` (line 796) |

A probe against lightningcss 1.0.0-alpha.72 (2026-10-28) confirmed that it
produces each of these from a stylesheet.

**What else reads bevy_markup's selector types.** Several incremental-update
mechanisms inspect them, not just the matcher:

- `SelectorReads` (`HtmlStyles::reads`): what the sheet's selectors read
  (dataset, siblings, structure), which feeds the style cache key and the
  skeleton's reuse rule (review_0005 I05, I06);
- `HtmlStyles::pseudo_reactive` / `Selector::matches_ignoring_pseudo`:
  ADR 0029's reactivity record. It asks whether a selector *could* match if
  every pseudo-class went either way, using three-valued logic (`Tri`), so
  `:not(:hover)` stays unknown;
- the sibling restyle when an interaction state flips (ADR 0028), and
  `build.rs` `SiblingChange` for the skeleton.

## Decision

Proposed: match selectors with parcel_selectors' engine, and delete the
string round-trip and the hand-written parser and matcher.

1. **Direct dependency, pinned to lightningcss's.** Add
   `parcel_selectors = "=0.28.3"`, the version lightningcss 1.0.0-alpha.72
   requires, so the types unify. Bump the two together. Rules keep
   lightningcss's owned `Selector<'static>` instead of bevy_markup's
   `Selector`.

2. **An element arena per build implements `Element`.** The build walk
   records each element once: tag, id, classes, attributes, `PseudoState`,
   the parent and the previous/next element sibling (as indices), whether
   it is empty, and whether it is the root. `Element` is implemented for
   `(&Arena, index)`. With parent and siblings in reach, sibling
   combinators work anywhere, along with `:only-child`, `:nth-last-*`,
   `:is()`, `:where()` and `:not()` with a list.
   - `match_non_ts_pseudo_class` maps lightningcss's `PseudoClass::Hover`,
     `Active`, `Focus`, `FocusVisible` and `FocusWithin` to the element's
     `PseudoState`. Nothing else reaches it (decision 3).
   - The arena carries all attributes, so attribute selectors work on any
     attribute with every operator. `HtmlElement::dataset` stays the public
     API for `data-*`.
   - Shadow DOM, slots, parts and pseudo-elements answer "no".

3. **An allowlist at load, required for safety.** A visitor over each
   selector's components skips the rule (logged at `debug`, as today) when
   it holds:
   - `NthOf`, `Has`, `Nesting`, `:nth-col()`/`:nth-last-col()`: the
     matcher panics on them (table above);
   - pseudo-elements, namespaces, `::part`/`::slotted`/`:host`;
   - unknown, custom or unsupported pseudo-classes.

   Dropping an invalid or unsupported selector's rule is also what browsers
   do. A unit test feeds each panicking form through `from_sheet` and the
   css fuzz target, so a lightningcss bump that adds new parts can't let one
   through unnoticed.

4. **Re-derive the analyses from components.** `SelectorReads` comes from
   the same visitor: attributes, sibling combinators (now at any depth),
   nth and `:empty` parts. The style cache key keeps its shape, but must
   cover the new reads: an ancestor's preceding siblings once sibling
   combinators work away from the subject, and following siblings for
   `:nth-last-*` and `:only-child`.

5. **Verify against what exists, before and after.**
   - `tests/selector_model.rs` keeps its reference model and extends its
     grammar to the new forms. It now checks the `Element` implementation
     rather than a hand-written matcher.
   - The oracle vectors (`cascade`, `combinators`, `sibling_combinators`,
     `structural_selectors`) must pass unchanged. New vectors cover `:is()`,
     `:where()`, sibling combinators away from the subject, and
     `:nth-last-*`, each checked against Chrome.
   - Benchmarks: the criterion and divan pipeline suites and the
     testbench's `selector-list`, before and after. Target: no regression
     above 10 %. If it is exceeded, use parcel's ancestor Bloom filter
     (`AncestorHashes` stored per rule) before going further.

## Alternatives considered

- **Keep the hand-written parser and matcher** (the state after
  bug_0180's fix). The cheapest option. The parser stays a second reading
  of CSS syntax that has to agree with lightningcss's; two panics so far
  came from disagreement. Every new selector feature stays hand work in
  both the parser and the matcher. Rejected for the long term.
- **Convert lightningcss's components into bevy_markup's types, keeping its
  matcher.** This removes the string round-trip and with it the parser bug
  class, and leaves every analysis untouched: moderate work, low risk. The
  matcher's limits remain (sibling combinators only next to the subject, no
  `:is`/`:where`), and so does the matcher-bug surface review_0005 found
  (I06–I08). The project owner chose the engine as the long-term solution
  (2026-10-28). This remains the fallback if the reactivity question below
  has no acceptable answer.
- **Servo's `selectors` crate.** It would need its own parse of each
  selector (lightningcss's types are parcel's), meaning a second parser
  again. Rejected.
- **Implement `Element` directly on the `tl` DOM.** That avoids building an
  arena, but `tl` nodes carry neither the interaction state nor positions,
  attribute lookups go through `tl`'s attribute list on every call, and the
  walk already computes the needed facts. `[INFERENCE]`: an arena is
  simpler and faster; to confirm in the spike.

## Consequences

**Gains.**

- The string round-trip and roughly 700 lines of parsing and matching go
  away, and with them the bug class behind bug_0002 and bug_0180.
- Selectors match the way lightningcss parsed them, with spec specificity.
- New selector support comes mostly from the allowlist, not from new code:
  sibling combinators at any depth, `:is()`, `:where()`, `:not()` with a
  list, `:only-child`, `:nth-last-*`, and attribute selectors on every
  attribute with every operator.

**Costs.**

- A direct dependency on parcel_selectors, pinned to lightningcss's version
  and bumped with it. ADR 0002 limits dependencies; this one is already in
  the tree.
- Four panicking match arms to keep out of the matcher (decision 3), and a
  test to keep them out across upgrades.
- An arena per build: memory and construction time, to measure.
- `SelectorReads`, the sibling restyle, `SiblingChange` and the style cache
  key are rewritten over components.

**Open questions, to settle in a spike before acceptance.**

1. **Reactivity under negation (ADR 0029).** `pseudo_reactive` asks whether
   a selector could match if every pseudo-class went either way. parcel
   evaluates `:not()` by negation, so forcing pseudo-classes true or false
   doesn't answer "either way" for `:not(:hover)`. Candidates:
   - build, at load, a copy of each pseudo-referencing selector with its
     pseudo-class parts removed, and match that (whether parcel's builder
     allows this is `[INFERENCE]`);
   - match twice, with pseudo-classes forced true and forced false. That is
     exact only when a selector holds a single pseudo-class;
   - derive reactivity from `ElementSelectorFlags` and a state-dependency
     map, as Servo does;
   - fall back to marking more elements reactive: correct, but more
     restyles.
2. **Cache keys for the new reads.** Which sibling identities each element's
   key must fold in, ideally only when the sheet's selectors read them
   (decision 4).
3. **Performance.** Unknown until measured (decision 5).
4. **`:has()`** stays out: parcel has no matcher for it, and it would need
   invalidation from descendants.

**Unchanged.** The bug_0179 nesting pre-scan (UPSTREAM.md U14, U15) is
still needed: lightningcss parses before any of this runs. Unsupported
selectors still skip their rule, and the public API does not change.

**Trial (2026-10-28).** Run on `spike/adr-0034-parcel-selectors`; full
results in `docs/agents/performance/adr-0034-trial.md`.

- Correctness: all tests and Chrome oracle vectors pass; the selector
  model property passes 5000 cases; a differential run over 28 237 css
  corpus inputs found one difference, an old-matcher bug
  (`:nth-child(0)` matched the root). A new `selectors_level4` vector
  agrees with Chrome and fails on `main`. 300 s of css fuzzing: no crash.
- Performance: the new `selectors` benchmark is 8.4 % faster at 120
  elements and 1.6 % at 600; nothing regressed beyond noise.
- Decision 2 changed: no arena. `Ancestor` carries a `SiblingLink` into
  its parent's `SiblingList`, and stand-in siblings from `Position` cover
  nodes without a list.
- Open question 1 answered: a pseudo-class answers
  `!context.in_negation()` in reactivity mode, exact while no `:not()`
  nests in another (rejected by the allowlist). Question 2: no key
  change was needed for the forms turned on. Question 3: no regression.
- Found: parcel's checked `i32` `An+B` arithmetic answers no for
  `:nth-child(n-2147483647)` (CSS: every element).
- Not done: sibling combinators away from the subject (needs restyle
  invalidation beyond ADR 0029), non-`data-*` attributes, `:root`.
