# 47. Bound quadratic template and Fluent operations before execution

| Field | Content |
|---|---|
| `ADR` | `0047` |
| `Title` | Bound quadratic template and Fluent operations before execution |
| `Date` | 2026-11-12 22:50 +0400 |
| `Author` | GPT-6, via Codex |
| `Commit` | `7298cc8 Document repository review 0009` + uncommitted review_0009 fixes |
| `Status` | Accepted |
| `Related` | Amends ADR 0044's filter work accounting and ADR 0045's FTL check; ADR 0046; bug_0241, bug_0243; UPSTREAM U46, U47 |

## Context

Review_0009 M03 found that Tera 2.4.0's `unique` uses `Vec::contains`
per element. The static template check charged linear byte work for a
quadratic operation. A constant 40,000-item range passed with 320,010
modeled steps but rendered for 2.02 seconds in release. Context-sized
inputs have quadratic work in the app data, contrary to ADR 0046's rule.

M05 found the same shape in both the FTL checker and fluent-syntax
0.11.1: every new named argument scans the previous names. A flat
20,000-argument call passed the nesting check, taking 237 ms in the
release pre-check alone. Changing this crate's scan alone would still
leave the upstream parser exposed.

## Decision

1. `tera_check::Analyzer::filter` charges `unique` for n(n−1)/2
   comparisons, multiplied by `1 + elem.bytes / 16`, in addition to the
   input/output byte charges. Saturating arithmetic and the existing
   `MAX_TEMPLATE_STEPS` limit refuse expensive template-made collections.
   Context-sized collections (`items.k > 0`) fail with `AppSquared`;
   apps can deduplicate their own data before passing it to Tera.
   Symbolic growth passes defer work accounting to the bounded real pass,
   as other operations do.
2. `ftl_check::Parser::get_call_arguments` permits at most
   `MAX_FTL_ARGUMENTS` = 64 arguments per call, named and positional
   combined. It refuses the 65th before parsing that expression or
   scanning duplicate names. The refusal fails the bundle load with its
   line and the limit name, just like the nesting limit. Nested and
   separate calls each get their own limit. This bounds each duplicate
   scan by a constant while keeping total work linear in source size.
3. No dependencies, forks, or runtime interpreter hooks are added.

## Alternatives considered

- Replace the checker's duplicate-name vector with a hash set: would
  improve the checker but leave the identical upstream scan unbounded.
- Increase the linear cost multiplier for `unique`: cannot model
  quadratic scaling. Charging pairs also composes with outer loops.
- Permit context-sized `unique` because the app chose its input size:
  the template chooses the operation and would amplify app data
  quadratically; ADR 0046 rejects that amplification for nested loops.
- Runtime instruction limits or a Tera fork: ADR 0044 rejected these;
  the local load-time checks retain the established approach.

## Consequences

Small constant `unique` calls remain accepted. Calls on context-sized
collections now fail at load, even if a particular context is small or
all its elements happen to be duplicates. This is intentional worst-case
accounting. Ordinary Fluent calls remain unchanged; very wide calls
fail instead of taking unbounded quadratic load time.

`review_0009_unique_charges_comparisons` tests refusal, an outer-loop
multiplier, context input and accepted small constants.
`review_0009_call_arguments_are_bounded` checks the exact boundary,
20,000-argument inputs, nested calls, term calls and upstream parser
agreement for accepted calls. `tests/html_ui.rs::review_0009` checks both
refusals through the real loaders. Regression assertions do not depend
on wall-clock thresholds.
