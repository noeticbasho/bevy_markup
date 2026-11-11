# 45. Fluent resources are checked at load; the fluent-syntax fork goes

| Field | Content |
|---|---|
| `ADR` | `0045` |
| `Title` | Fluent resources are checked at load; the fluent-syntax fork goes |
| `Date` | 2026-11-11 20:58 +0400 |
| `Author` | Claude Opus 5.5 (Anthropic), via Claude Code; the approach is the user's ("would it be possible to do similar pre-checks for the fluent dependency, so we could get rid of the patch for that as well? On asset load stage?") |
| `Commit` | `d7ddbe2 Fix review_0007's bugs 204-221; treat every input as untrusted (ADR 0044)` + uncommitted: this change |
| `Status` | Accepted |
| `Related` | ADR 0044 (every input is untrusted; the template check); bug_0005, bug_0015; UPSTREAM U1, U2 |

## Context

fluent-syntax 0.11.1 (what `fluent` 0.16 requires) has two bugs reachable
from any FTL file: an invalid `\u`/`\U` escape cut off by a multi-byte
character panics (U1, bug_0005), and nested placeables, select variants and
call arguments recurse without a limit, overflowing a 2 MB asset thread at
a few hundred levels (U2, bug_0015). A fork with both fixes was wired in
with `[patch.crates-io]` in the root and the four fuzz manifests. `[patch]`
applies only to the top-level project, so every app depending on
bevy_markup had to repeat it (README), or ran the unfixed parser.

ADR 0044 made every input untrusted and bounded templates with a static
check at load instead of a patched dependency.

## Decision

1. `src/ftl_check.rs` checks each `.ftl` resource once per load (and hot
   reload), before `FluentResource::try_new`, in the bundle loader, the
   fuzz harness and the Markdown fuzz path. A refused resource fails the
   bundle's load with its line.
2. **Escapes (U1):** `bad_escape` refuses a `\u` or `\U` followed by fewer
   hex digits than it needs and then a non-ASCII byte, anywhere in the
   file. That's the only input reaching the panicking slice; the scan
   needs no context (in text it refuses a file that's fine, which only
   `\u` directly before a non-ASCII letter does).
3. **Nesting (U2):** a port of fluent-syntax 0.11.1's `parse_runtime` and
   everything it calls: the same pointer moves and the same errors at the
   same positions, so Junk recovery restarts where fluent-syntax restarts,
   with no tree. It counts the active placeable (`{`) and call-argument
   (`(`) levels, which every recursion cycle passes through (the fork
   counted in `get_inline_expression`, which the select-variant cycle
   doesn't), and refuses past `MAX_FTL_NESTING` = 64.
4. The `[patch]` is removed from all five manifests; the README section
   asking dependents for it is gone.

## Alternatives considered

- **Keep the fork**: correct here, unfixed in every dependent app.
- **Count brackets without parsing**: a `}` or `)` in a string literal
  would undercount; a `"` in variant text isn't a string, so skipping
  quoted spans could hide real nesting; placeables span lines starting at
  column 0, so the file can't be split per message. The hand-written CSS
  scan of bug_0183/0184 failed this way.
- **Parse on a thread with a huge stack**: the depth is still unbounded by
  input size.

## Consequences

- Upstream fluent-syntax, unpatched; dependents need nothing.
- `ftl_check::tests::the_port_finds_fluent_syntaxs_entries` compares the
  port's entries (messages, terms, junk spans) with fluent-syntax's on
  generated FTL (2 000 cases per run; seven mutations of the port's error
  paths tried, five caught, two equivalent).
  `the_limit_fits_a_two_megabyte_stack` parses and formats every cycle at
  the limit on a 2 MB thread with the real parser and resolver (192 select
  levels still fit in a debug build).
- A resource nesting deeper than 64 now fails the whole bundle (the fork
  made only that entry Junk).
- The port follows 0.11.1: on a `fluent` bump, re-run the tests and re-port
  what changed; drop a check once a released fluent-syntax fixes its bug.
