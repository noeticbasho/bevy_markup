# 32. Windowed build, revised: one unit system, nested culling, scroll reuse and slack

| Field | Content |
|---|---|
| `ADR` | `0032` |
| `Title` | Windowed build, revised: one unit system, nested culling, scroll reuse and slack |
| `Date` | 2026-10-16 10:25 +0400 |
| `Author` | Claude Opus 5.5 (Anthropic), via Claude Code |
| `Commit` | `bd99546 INDEX: bug_0151's row` + uncommitted review_0006 fixes |
| `Status` | Accepted; amended by 0033 |
| `Related` | ADR 0031 (amended by this record), ADR 0021 (the skeleton), ADR 0029 (restyle accounting), review_0006 (J01–J09 and the follow-up), bug_0151–bug_0157, bug_0160–bug_0177 |

## Context

ADR 0031 introduced the windowed build. review_0006 found that the
implementation diverged from it, and from correctness, in ways that need
design decisions rather than point fixes:

- the band drifted above the viewport (bug_0151: coalesced spacers
  dropped the row gaps);
- nothing inside a container was ever culled (bug_0152);
- the units were mixed: a physical border box was compared with a logical
  content width, so any padded or bordered root, or HiDPI display,
  rebuilt every frame and culled nothing (bug_0153);
- nothing culled before the first scroll (bug_0154);
- scrolling re-collected the whole band every frame (bug_0156), and
  rebuilt on every sub-pixel step (bug_0157).

ADR 0031 also promises things the code doesn't do: focus pulling an
off-window element into the band (§3) and windowing being off in
headless harnesses (§5). Its measured win (77.9 → 17.2 ms, testbench
`--no-render`) was taken where the root is never laid out (0 × 0), and
with the bench's scripted key injection free to change the size.

## Decision

1. **One unit system.** The band, the width anchor and the height model
   are all logical pixels of the root's **content box**
   (`logical_content_width`). The visible region is the padding box,
   starting `padding-top` above the content (`visible_region`). Only
   `ComputedNode`'s measured size is converted; CSS margins are already
   logical.
2. **Measure every level into the root's model.**
   `measure_window_heights` walks up to the nearest `HtmlUi` and skips
   UIs without `HtmlWindowed`. Containers' children get heights, so the
   nested cursor frames cull. Keys (`WindowKey`) exist only on windowed
   UIs' entities: an item is keyed when it is collected with a window.
   `update_spec` keeps the key in step, so entities spawned by a
   window-less first build gain theirs.
3. **The measurement drives one rebuild.** `WindowModel::changed` (a new
   height, or one moved by more than 0.5 px) triggers one build. A root
   that isn't laid out (zero viewport) windows nothing, so it builds
   everything rather than culling it all.
4. **Scroll builds pair with the skeleton.** They reuse what stays in the
   band, under three rules:
   - A cached subtree reuses only if it was built keyed, holds no
     spacer, and lies wholly inside the current band. It carries the
     culling of the band it was built under, so a straddling container
     re-collects and its children decide one by one.
   - An unchanged document keeps the model's keys (the pairing holds
     element pairs only; remapping would drop anonymous blocks).
   - Scroll builds don't touch the strike state (a long jump reuses
     nothing and says nothing about content pairing, as ADR 0029 decided
     for restyles).
5. **Rebuild by slack, not by offset.** Each cull decision records how
   far the offset may move before it flips. A culled element flips past
   its distance; a materialized one at it, so its slack is shortened by
   an epsilon. A subtree reused whole counts by its own extent. The
   model keeps the anchor and the slack. The UI rebuilds when the offset
   leaves the slack, when the viewport's width **or height** changes,
   or when a height changes.
6. **The spacer keeps its column's gap** (`Item::Spacer { gap }`):
   coalescing k culled siblings adds the k − 1 gaps between them.
7. **ADR 0031's unimplemented promises are dropped from the contract:**
   - Focus stays inside the band; navigating to an off-band element is
     not supported. This is what `HtmlWindowed`'s doc already says.
   - Windowing applies wherever the root is laid out. Headless harnesses
     without layout build everything (decision 3).

## Alternatives considered

- **Hysteresis instead of exact slack** (rebuild every N px). Simpler,
  but it either rebuilds needlessly (N small) or shows unbuilt rows
  (N larger than the margin). The slack is exact and costs a min per
  cull decision.
- **Never reuse in windowed builds** (the pre-review behavior). Correct
  but slow: a scroll frame re-collects the whole band. Measured: a
  one-row scroll reused 0 of 74 specs; now most.
- **Clear and re-measure the model on any doubt** (what the mixed units
  did by accident). It defeats culling entirely.
- **Keep keys on every UI and filter in the measurement.** That makes
  non-windowed UIs pay a walk per laid-out change per frame.

## Consequences

- Measured with real layout (testbench, `--offscreen`, release, 2000-row
  big-document, worst frame after warm-up): unwindowed 85.9 ms idle and
  81.5 ms scrolling; windowed 23.0 ms idle (165 elements, 56 of 58
  reused) and 21.5 ms scrolling (77 of 101 reused). ADR 0031's
  `--no-render` figure didn't measure windowing; these replace it.
- The testbench's `--size` now pins the size: `--exit-after`'s injected
  keys stepped it, so scripted runs measured some other size.
- Tests compare positions with an unwindowed build of the same document
  (review_0006's `band_errors`), and the testbench gains a `band` check
  (the built rows cover the viewport). Both fail with bug_0151 put back;
  counts of built rows never could.
- `note_reused`'s slack guards bounded over-building only. It binds when
  no neighbour sits between the band's edge and a reused subtree (the
  first child of a container whose padding holds the edge); its test
  builds exactly that.
- The windowing property (`tests/windowing_model.rs`, review_0006's
  follow-up) found twelve more defects (bug_0160–bug_0171), and the mutation run
  over the windowing diff six more (bug_0172–bug_0177). Three of the
  fixes are decisions:
  - **A windowed UI always pairs.** ADR 0021's adaptive gate turns the
    pairing off after low-reuse builds. A windowed build reuses at a
    band's rate, and its pairing also carries the height model across
    content changes, so with the gate the model cleared (bug_0167).
  - **The band follows the offset the root shows:** `ScrollPosition`
    clamped to the last layout's scroll range, as Bevy's layout clamps it
    (bug_0164).
  - **Walked-through elements stay in the walk.** The first
    implementation unwrapped `html`/`body` so the cursor started at the
    content, and lost their styles for every UI (bug_0172). Instead a
    walked-through element advances its column's cursor by its
    children's heights (bug_0175).
- Content changes have two known costs (`HtmlWindowed`'s docs):
  - One inserted or removed row builds the rest of its list for a
    frame: the skeleton aligns siblings by position (bug_0168, open).
  - Rows above the band that change height shift the view for one
    frame, beyond the margin (bug_0170, a known limit).
- Open: focus navigation beyond the band stays unsupported.
