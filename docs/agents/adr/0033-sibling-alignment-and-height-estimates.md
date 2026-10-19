# 33. Sibling alignment and height estimates for content changes

| Field | Content |
|---|---|
| `ADR` | `0033` |
| `Title` | Sibling alignment and height estimates for content changes |
| `Date` | 2026-10-19 10:07 +0400 |
| `Author` | Claude Opus 5.5 (Anthropic), via Claude Code |
| `Commit` | `dbd5a06 Improve tests and fix bugs` + uncommitted bug_0168 fix |
| `Status` | Accepted |
| `Related` | ADR 0021 (the skeleton; amended), ADR 0032 (the windowed build; amended), bug_0168, bug_0170, bug_0165, review_0006 |

## Context

ADR 0021's skeleton pairs the fresh DOM with the previous build's, so
unchanged subtrees reuse their specs. Two rules made a single inserted or
removed element expensive (bug_0168):

- **The pairing aligned siblings by position only.** At the top level
  (`pair_trees_into`) it zipped the old and fresh children, so after an
  insertion every later child paired with its old neighbour. Each such
  pair is unclean: it re-collects, and in a windowed UI `Ctx::cull`
  refuses to cull it (bug_0161), so it builds. Inside a container
  (`pair_node`), a changed child count didn't pair the container at all.
  Its whole subtree lost reuse and, in a windowed UI, its heights
  (bug_0166).
- **An unknown height invalidated the window's cursor** (ADR 0032
  decision 3 and the cursor rules). A newly inserted element has no
  height until its first layout, so everything after it in its frame
  built.

Measured with `one_inserted_row_builds_only_the_band`: a windowed list
of 1000 rows with 20 built showed 1000 or 1001 built in the frame after
one row was added or removed, back to 20 the next frame.

## Decision

1. **Siblings align by their common head and tail** (`build.rs`
   `align_siblings`). Equal counts still pair by position. A changed
   count pairs children from both ends while their source is identical
   (`same_source`: a tag's `raw()` span covers its subtree), then the
   rest of the middle by position. It applies at the top level and in
   `pair_node`. A container whose count changed is now paired, but not
   clean (`all: same_count`), so the clean set still gates reuse. Pairing
   claims nothing about cleanness; `pair_node` decides it per pair, as
   before. Positions that the cascade reads stay correct for two reasons:
   - A changed count is `SiblingChange::Shape`, which un-cleans every
     child when the sheet is sibling-dependent (review_0005 I05).
   - An `li` the alignment moved to another ordinal changed its marker
     number, so it leaves the clean set (`shifted`).
2. **A new element's height is estimated** (`WindowCtx::estimate`,
   `estimate_for`). Its step is the mean of its measured siblings'
   heights in the same frame. The cursor places the rows after it by that
   estimate for one frame; the measurement then records the real height
   and rebuilds. Still invalidated:
   - a frame with no measured child (the first build, a cleared model);
   - a child the measurement marked untrackable (`positioned`: out of
     flow, or an `auto` margin it can't resolve).

   The unknown element itself is always built: `culled` needs a height.

## Alternatives considered

- **Keyed alignment by `id`** (React-style). Exact for keyed lists, but
  the skeleton pairs DOM nodes, not elements. `update_children` already
  keys entities by unique `id`, and head/tail alignment covers the
  common insertion and removal without requiring authors to write ids.
  A minimal edit script (LCS) over each changed list costs O(n·m) per
  list. Head/tail is O(n), and a single edit is its best case.
- **Keep invalidating at an unknown height.** It is exact, but it is the
  cost bug_0168 measured on every insertion.
- **Estimate from the old element at the same position.** It doesn't
  exist for an insertion. The siblings' mean is defined whenever any
  sibling is measured, and it is exact for uniform rows.

## Consequences

- One inserted or removed row now builds only the band, at the top,
  middle and end of a root list and of a nested one
  (`one_inserted_row_builds_only_the_band`, which also checks placement
  and coverage against an unwindowed twin in every frame).
- Non-windowed UIs reuse more: the rows after an insertion pair with
  their own old copies (clean) instead of their neighbours. Updates stay
  equal to a fresh build. `one_insertion_or_removal_matches_a_fresh_build`
  (`tests/properties.rs`) checks a positional sheet and a plain one, top
  level, `section` and `ol`; it fails with either uncleaning rule removed.
  An id-less row reused onto another row's entity goes through
  `update_spec`, as ADR 0021's implementation note 3 already handles.
- An insertion whose height differs from its siblings' mean shifts the
  rows below it by the difference for one frame. This is bug_0170's
  known limit, now with a third cause; the band's margin absorbs shifts
  up to its size.
- bug_0165's frame-invalidation path is now reached only by untrackable
  children; its regression test uses an `auto` margin.
- Cost: the head and tail scans compare source bytes, O(list size) per
  changed list, and run only when a count changed. [INFERENCE] This is
  negligible next to the collection it saves; not benchmarked
  separately.
