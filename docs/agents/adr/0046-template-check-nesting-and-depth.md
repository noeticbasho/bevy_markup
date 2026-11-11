# 46. The template check: loops over context data nest only over their element; one depth budget

| Field | Content |
|---|---|
| `ADR` | `0046` |
| `Title` | The template check: loops over context data nest only over their element; one depth budget |
| `Date` | 2026-11-12 03:15 +0400 |
| `Author` | Claude Opus 5.5 (Anthropic), via Claude Code, fixing review_0008's findings on request ("fix all the bugs"); the nesting rule is review_0008 L04's fix direction |
| `Commit` | the commit after `4af00b9 Fix CI on Rust 1.99: reserve SVG raster bytes without fetch_update` |
| `Status` | Accepted |
| `Related` | Amends ADR 0044 §2 (the loop accounting) and ADR 0045 §2 (the escape scan); bug_0223, bug_0226, bug_0227, bug_0228, bug_0236; review_0008 L01, L04–L06, L14 |

## Context

ADR 0044 counts a loop over app data as one iteration: bounds are "per
app element", the app chose how much data to send. Nested, that stayed
one per level, so n items cost n^levels while the check reported a
handful of steps: a 165-byte template over a 30-item list rendered for
223 ms at five levels and ~7 s at six; a context number in `range(end=hp)`
counted once too, 3.5 s at four levels (review_0008 L04, bug_0226).

Separately, the analysis recursed once per statement level across
includes with only templates and components counted, and 24 small files
of nested loops overflowed the asset thread's 2 MB stack — an abort
from the check itself (L01, bug_0223).

## Decision

1. **Nesting over app data.** Every value records the app loop whose
   element it is part of (`Av::origin`: kept through attributes, items
   and filters, lost when two origins mix). A loop or comprehension over
   app data inside another app loop must run over the innermost app
   loop's element (`for group in groups` → `for card in group.cards`);
   any other is refused, `CheckError::NestedAppLoops`. Nested that way,
   the iterations sum to the app's data, so "per app element" stays
   true. A second loop over the same element's data
   (`for c in a.cards` → `for d in a.cards`) is refused too: only the
   innermost loop's element qualifies.
2. **`range` over context numbers** counts `RANGE_MAX` (10⁵) items: a
   number isn't a collection the app sized.
3. **Size costs steps.** Building or scanning a value charges its bytes
   ÷ 16 wherever it happens (operators, literals, comprehensions,
   calls, filter results), as filters already did (bug_0227).
4. **One depth budget**, `MAX_DEPTH` (96), over every level the analysis
   recurses through — statement lists and expressions, across includes,
   components and captures — refused as `TooComplex`. A level costs
   ~14 KB of a debug build's stack (measured), 1.4 MB in all.
5. **FTL escapes (amends ADR 0045 §2).** The file-wide scan for a `\u`
   escape before a non-ASCII byte refused valid text (`C:\użytkownik`,
   bug_0236). The port refuses the escape where fluent-syntax reads it,
   in its string-literal path; a property checks the refusal against
   fluent-syntax's own panic, both ways.

## Alternatives considered

- **Allow two levels of app loops** (rows × columns from independent
  context lists): a hostile template squares any list an app sends
  (10⁴ items → 10⁸ iterations). Tables whose rows hold their cells pass.
- **Track `A²` in the bounds** (a quadratic term next to `k·A`): the
  steps would still have no bound, which is the problem.
- **Run the check on a thread with a large stack**: hides the depth; the
  render that follows recurses too, and Tera's own limits are per file.

## Consequences

- A template looping over one context list inside a loop over another
  fails to load, with a message naming the fix. The examples pass
  (`content_lint`).
- `range(end=n)` with a context `n` and a heavy body can pass
  `MAX_TEMPLATE_STEPS` (335 steps per iteration).
- Accumulators appended in loops are charged their final size each
  iteration: 10⁵ appends of 100 bytes are refused (they took 13–17 s).
- Templates nesting more than ~90 levels across their includes are
  refused (Tera allows 40 per file).
- A `set_global` counter or accumulator in nested loops is accepted
  (one copy per variable, bug_0228); a filter combining two context
  values stays refused (ADR 0044).
