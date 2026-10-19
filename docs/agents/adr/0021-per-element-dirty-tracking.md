# 21. Per-element dirty tracking: dependency maps at load, slot-local updates per frame

| Field | Content |
|---|---|
| ADR | 0021 |
| Title | Per-element dirty tracking: dependency maps at load, slot-local updates per frame |
| Date | 2026-10-12 11:24 +0400 |
| Author | GLM (opencode-zen/glm-5.3-flash), via omp — drafted for the project owner |
| Commit | `f05290d` Separate Tera statements in Markdown templates at load |
| Status | Accepted — fully implemented (A+B, C-1, C-2; see the implementation notes); amended by 0033 (sibling alignment) |
| Related | 0017 (its fingerprint variant was rejected on measurement — this is the successor design), 0020 (the load-time conversion this builds on), 0009 (identical-render skip, in-place updates), 0018/0019 (measurement); the "Low-Hanging / Dirty-Flag Rebuilds" items in the session record |

## Context

What still scales with the document is the *changed* path. 0009 already
skips an identical render entirely (no parse, no cascade, no diff; the
idle frame costs 194.8 µs at 600 sections, criterion) and writes kept
elements' components only where they differ (`build.rs` `put`). But a
context change that alters the rendered HTML re-runs the whole pipeline,
no matter how little changed:

1. **Render** — Tera renders the whole template (`html.rs`
   `render_templates`);
2. **Parse** — `tl` parses the whole output;
3. **Collect + spec** — every element is walked (`build.rs`
   `collect_items`), its style resolved, and a `NodeSpec` built;
4. **Diff** — `update_children` matches old and new specs and writes
   what differs.

Current criterion numbers (warm-up 0.5 s, measurement 1 s, this
machine, `f05290d`): `frame/changing/600` 2.02 ms, `frame/localized/600`
2.55 ms, `frame/markdown/600` 8.25 ms, `frame/rebuilding/600` 9.87 ms
against an idle floor of 194.8 µs. Stage instrumentation (timed spans
inside `build_html_ui`, temporary, this session) put the markdown/600
correct-DOM build at ~4.2 ms total: collect ≈ 3.4 ms, spec construction
≈ 0.42 ms, diff ≈ 0.38 ms — the collect walk dominates the library-side
cost, and much of it is fixed per-element work (the cascade style-cache
key clones `tag`/`id`/`classes` Strings per element per build; measured
earlier this session at ~3 allocations per element per build, ~22 k
allocations/frame at 7203 tags). The aggravating case is unchanged since
0017: per-frame context UIs (`calc`, `live`) pay stages 1–4 in full
every frame.

The fine-grained-reactivity frameworks (SolidJS, Leptos) solved this
class by compiling templates to real DOM nodes and updating only the
bindings whose inputs changed — no whole-document re-derivation. The
question: can bevy_markup get the same shape for its template-driven
UI, and can it afford it?

**ADR 0017 is the caution.** Its fingerprint design — per-frame hashing
of every item's subtree inputs, reusing cached `NodeSpec`s on a match —
was implemented and **rejected on measurement** (changing 600: +28%,
hashing alone +0.46 ms against −0.08 ms of reuse). Its stated reasons:
the cascade was already nearly free (per-element style cache), the diff
was already incremental, and per-frame O(subtree) hashing plus a deep
`Item` clone per hit cost more than the skip saved. Any successor must
answer those numbers, not ignore them.

## Decision (proposed)

Replace 0017's *per-frame fingerprinting* with *load-time dependency
extraction*: the mapping from context variables to template slots is
computed once when the template loads (ADR 0020 already parses the
converted template at load); per frame, only slots whose variables
changed are re-derived. The per-frame skip cost drops from "hash every
element's subtree" to "look up which slots read a changed key".

1. **Dependency extraction at load.** After the loader's conversion
   (`md::to_html_template`) and Tera compilation, walk the converted
   source and record, per tl node position in the converted output:
   - **value slots** — a text run's Tera interpolations
     (`md.rs` `tera_runs` already isolates them per node) and templated
     attributes (`style="calc(… {{ sidebar }} …)"`, `data-l10n-args`),
     each mapped to the set of context keys its expression reads;
   - **structural slots** — `{% for %}`/`{% if %}` substreams, mapped to
     the keys their collection/condition reads. A structural slot's
     body is a keyed sub-document: if its inputs changed, the existing
     `update_children` reconcile runs over just that subtree.
   Expressions' variable sets come from Tera's parsed AST. Tera 2.4
   (`src/parsing/ast.rs`) does not export it publicly (checked this
   session) — a `[patch.crates-io]` fork export, the same mechanism the
   fluent-syntax fork uses (UPSTREAM U1/U2), or a weaker self-written
   reference scanner that misses filter/function internals. A missed
   variable is a correctness bug (below), so the fork export is the
   default choice; the scanner is the fallback if the fork is refused
   upstream.
2. **Per-key change detection per frame.** Before rendering, diff the
   context's values against the previous frame's per key (`tera::Context`
   iteration; keys are few, values small). A key set with no changes
   takes 0009's identical-render path unchanged. Changed keys mark their
   slots dirty via the load-time map — a bitmap walk over the slot list,
   O(slots), no hashing of element subtrees.
3. **Slot-local update, skeleton reuse.** The build keeps the previous
   frame's item/spec skeleton. Non-dirty items are reused verbatim —
   no cascade, no spec construction, and `update_children` writes
   nothing for them (it already compares and skips via `put`). Dirty
   value slots are re-collected and updated in place
   (`update_spec`'s existing path; the restyle machinery proves a
   single element can be rewritten without touching siblings). Dirty
   structural slots re-run the keyed reconcile over their subtree.
   Stages 1–2 stay whole-document: the Tera render and `tl` parse
   measured 43 µs + 46 µs at markdown/600 this session — not worth
   instrumenting (0017's Conclusion, unchanged).
4. **Phased, with a measurement gate.**
   - **A** — per-key change detection + "nothing changed → skip" fast
     path (extends the existing identical-render check). Small; no new
     semantics.
   - **B** — the dependency map + slot-local collect (1–3 above).
     Measured against 0018/0019 suites before any further work; 0017's
     kill-switch discipline applies (the map off → the exact current
     path).
   - **C** — full inversion (dirty slots drive direct entity updates,
     the spec tree becomes authoritative) is **gated**: 0017's outcome
     showed the clone-heavy reuse path loses, and clone-free reuse means
     `Rc`/ownership threading through the whole build path — a large
     redesign. C is attempted only if B's measured win makes it worth
     the redesign, with numbers in the record.
5. **Freshness contract, made testable.** A dirty-path build must equal
   a fresh build for the same inputs — the metamorphic property
   `content_update_matches_a_fresh_build` (0009) extended to
   random *key-change sequences* over random documents: after each key
   change, the built world must equal a fresh build. This is the net for
   the new bug class: a missed dependency edge renders stale UI until
   the next full rebuild (locale/stylesheet/font changes already force
   full rebuilds and are unaffected).

The public API does not change. No new components; the map and skeleton
live beside `RebuildState` on the `HtmlUi` entity. `HtmlUiBuilt` /
`HtmlUiRestyled` fire as today; kept elements keep entities and attached
state exactly as 0009 promised.

## Alternatives considered

- **0017's per-frame fingerprints** — implemented, measured, rejected
  (+28% on the changing path at 600 elements; hashing +0.46 ms, reuse
  −0.08 ms). This design differs where that one failed: the dependency
  work moves to load (one-time), so the per-frame skip is a bitmap
  lookup rather than O(subtree) hashing; and the target scale is
  different — 0017 measured 600-element plain documents (changing =
  1.93 ms, fixed costs dominant), while markdown/600 is ~4200 entities
  at 8.25 ms with collect+specs+diff ≈ 4.2 ms to skip. Whether the win
  survives the bitmap and skeleton overhead is exactly what the B-phase
  gate measures `[INFERENCE]`.
- **Static/dynamic template split** (Million.js, arXiv 2202.08409):
  partition the converted template into static chunks reused verbatim
  and dynamic holes re-derived per frame. This is the dependency map
  with all-empty variable sets — a special case of Decision 1. It is
  the fallback if the full map is rejected by measurement: less
  machinery, no Tera AST fork needed (a static/dynamic classification
  comes from the same `tera_runs` scan without expression parsing), and
  it still removes the static fraction from stages 3–4.
- **Render instrumentation / sentinel diffing** (0017's Alternative 1)
  — still rejected, same reasons: opaque Tera renders, structural
  changes remount subtrees anyway.
- **Incremental parse** (0017's Alternative 2) — still rejected:
  fragment parsing cannot know it isn't cutting a tag in half.
- **Do nothing** — the fallback if B fails its gate. 0017's lesson
  stands: the current changed path is already within ~10× of idle for
  plain documents, and the machinery is proven.

## Consequences

- **Expected gain** `[INFERENCE]`, with the fan-out honesty the design
  depends on: the savings scale with how few slots read a changed key.
  A low-fan-out change (one text field, one chip) drops the changed
  path toward idle + the dirty slots' cost; a high-fan-out change (a
  variable in every element's `style`, as the `calc` example's widths)
  saves almost nothing — every element is dirty. The markdown workload
  (mostly static prose, few dynamic values) is the best case; the calc
  example is the worst case. The B gate must report both.
- **Correctness surface**: the stale-UI bug class (0017 named it; this
  design widens it to template structure). The mitigation is the
  Decision 5 property, the existing stateful machine (extended with
  key-change ops), and full rebuilds on locale/stylesheet/font changes
  as dirty-path escape hatches.
- **The Tera AST fork** adds a vendored dependency with upstream-tracking
  obligations (UPSTREAM.md), like the fluent-syntax fork.
- **Memory**: the dependency map and the cached skeleton are held per UI
  between builds (0017's consequence, same shape). Despawn takes both.
- **Open after B**: per-element `var()` invalidation (0017's open item —
  the sheet-level `Frame::vars_changed` hash is the conservative
  version this design inherits); `data-l10n-args` changes as slots;
  includes/extends' contribution to the map (the loader merges
  references, so extraction runs on the merged set).

## Implementation note (2026-10-12, same day): phases A+B implemented

A+B are in (`build.rs`), with a refinement over the ADR's letter: **no
dependency map and no Tera AST fork** — the dependency question dissolved.
Cleanliness is decided by *content equality*: a fresh build pairs its DOM
against the previous build's (`Skeleton` holds the previous document, the
item tree and a handle→item-path table); `pair_trees_into` walks both DOMs
in lockstep (tag + attributes + own text, recursively) and marks whole
subtrees clean. A clean subtree's cached item is cloned with every DOM
handle remapped to the fresh document (`remap_item`), skipping the cascade
and the run construction for it. Content equality is sound by
construction — what renders the same builds the same item — and needs no
expression parsing. (The skeleton's document is a re-parse of the source,
~35 µs at markdown/600; its items are the build's own, moved.)

Gates: the reuse runs only on `Build` frames whose change came from the
render, never when `:hover`/`:active` state, `var()` values, fonts or
inline images changed this frame (a reused item's styles would be stale);
restyles refresh the skeleton's items in place (the document is
unchanged). Translated elements never reuse (their items come from
per-build fragments); a localization change does not block reuse — the
first draft gated on `!localized.is_changed()`, but the localize system
rewrites `LocalizedText` whenever the render changes, which disabled reuse
on every localized frame; the gate is redundant with the walk's
translation exclusion and was removed. An adaptive gate turns the pairing
walk off after a useless pass (an all-dirty workload, e.g. `rebuilding`)
and retries every 16th build (backing off ×2 per useless retry, capped
at 256); when skipped, the skeleton's specs and document are replaced
alongside the fresh entities (a gate-skipped build updates the
entities, so the old skeleton would be stale) —
no per-frame data move.

`item_spec`/`block_spec` take `&Item` now (the skeleton keeps the build's
items by move and clones only reused subtrees). `Skeleton` is
`#[doc(hidden)] pub` for the property harness.

Measured (criterion, warm-up 0.5 s, measurement 1 s, against the
pre-skeleton state on this machine):

| Benchmark (600) | Before | After | Δ |
|---|---|---|---|
| `idle` | 194.8 µs | 190.0 µs | −2% (noise) |
| `changing` | 2.02 ms | 1.92 ms | −5% |
| `localized` | 2.55 ms | 2.64 ms | **+4%** (100% fan-out — every element translated, nothing to reuse; the adaptive gate's residual) |
| `markdown` | 8.20 ms | 7.56 ms | **−8%** |
| `rebuilding` | 9.87 ms | 9.3–9.4 ms | ~noise (all-dirty frames; the gate disables the walk) |

The freshness net is
`skeleton_reuse_matches_a_fresh_build` (`tests/properties.rs`): two
long-lived UIs run the same random context-op sequence, the comparator
drops its skeleton before every change (every build there is fresh), and
the dumps must match after every op. The full suite passes, including the
stateful machine's model checks over context/locale/theme/pointer ops.

Phase C stays gated as written: reuse at the item level still runs
`item_spec` on the cloned items (~half the per-block cost), so the
theoretical floor needs the spec-level cache with clone-free ownership —
attempt only if a workload's profile justifies it with numbers.

## Implementation note 2 (2026-10-12, later): C-1 implemented — shared specs

C-1 is in: the skeleton caches the previous build's **spec tree**
(`Arc<NodeSpec>`), not its items. A clean subtree is handed to the diff by
refcount — no item clone, no `item_spec` — and the reused spec keeps its
place in the next frame's skeleton, so the trees are structurally shared
across frames. Two pieces made borrowing sound:

- **`put_ref`** — the compare-then-clone write: the queued commands capture
  the spec by `Arc` and every component write compares against the current
  value *before* cloning, so a visited-but-unchanged element copies
  nothing (`update_spec`/`spawn_spec`/`spawn_spans` take references now;
  fresh specs are wrapped in `Arc` at construction, which the skeleton
  then adopts by move).
- **The up-front handle remap** (`remap_spec_handles`): the cached specs'
  `DomNode` handles point into the previous document; the pairing's
  reverse map rewrites them while the `Arc`s are exclusively held, so
  reused subtrees (and the entities they spawn) never carry stale
  handles, and the `:hover` state walk stays keyed by current handles.

Nested reuse works: a dirty container's clean children reuse their specs
inside the container's fresh collection (`Item::Cached` flows through
`item_spec`'s children construction).

Measured same-session, skeleton on/off (the only trustworthy comparison —
cross-session criterion drift measured ±10–27% on the pure
`render`/`parse` stages during this work):

| Benchmark (600) | Off (= A+B) | On (C-1) | Δ |
|---|---|---|---|
| `changing` | 2.36 ms | 1.42–1.54 ms | **−35…−40%** |
| `markdown` | 10.28 ms | 7.79–7.95 ms | **−23…−24%** |
| `localized` | 2.94 ms | 2.95 ms | 0 |
| `rebuilding` | 9.89 ms | 10.06 ms | +1.7% |
| `idle` | 219 µs | 218 µs | 0 |

The dirty path pays ~nothing: an all-dirty workload's writes were going to
change anyway, so `put_ref`'s lazy clone costs what the old eager move
plus `set_if_neq` cost. The remaining gap to the theoretical floor is the
diff's *visit* itself (it still walks clean subtrees to confirm they
match) — that is C-2, gated on the same numbers discipline.

C-2's soundness argument, recorded while it is cheap: a clean subtree's
cached spec is only ever *read* by the diff, and every write it would do
compares equal (the gates invalidate the whole reuse on any style-side
change; the `ImageNode` re-insert on visits is covered by the
`images_changed` gate). Skipping the visit entirely for cached specs is
therefore equivalent — but it makes the gate list load-bearing
correctness, which is why it stays a separate, measured step.

## Implementation note 3 (2026-10-12, latest): C-2 implemented — the diff skips clean subtrees

C-2 is in: the spec tree carries its provenance (`SpecNode::Fresh` /
`Cached`), and `update_children` **skips `update_spec` entirely for a
cached, identity-matched child** — the entities already match its specs
(every write since they were built compared equal under `put_ref`, and
the gates drop the whole reuse on any style-side change, which is what
makes the skip load-bearing correctness rather than an optimization).
Two things keep it sound:

- **The one component that legitimately changes across frames is the
  `DomNode` handle** (an earlier sibling insertion shifts tl's indices
  without touching any subtree's content). A cached child therefore gets
  exactly one queued write — `put(DomNode(remapped_handle))` — a compare
  that usually writes nothing.
- **The skip only fires when the cached spec lands on the entity it was
  built for** (`DomNode == pairing[spec.handle]`, threaded through the
  diff as `update_children`/`update_spec` parameters). The freshness
  property `content_update_matches_a_fresh_build` caught the first
  version skipping that check: an id-less list reordered between frames
  positionally pairs a cached spec with a foreign entity, and skipping
  the visit left the foreign entity carrying the old subtree's content
  while the fresh build shows the new order. The fix: a cached spec on a
  foreign entity goes through `update_spec` like a fresh one — sound
  because a cached spec's content is provably what a fresh build
  produces for its subtree (the A+B invariant), so the write is correct
  either way; the skip is simply lost on that element for that frame.
- **The `:hover`/`:active` state walk maps entity handles through the
  pairing's reverse map** before keying the states (the walk runs before
  the collect and the skip means an entity's handle can lag the fresh
  document by the frames it was reused). On pure restyles the source is
  unchanged, the mapping is the identity, and the walk pays nothing.
- The retry cadence backs off with consecutive useless passes (16 → 64 →
  256 builds), so an all-dirty workload (`rebuilding`) pays almost
  nothing while one that becomes reusable is still found; and the
  pairing walk checks the translation exclusion *before* the attribute
  comparison, so an all-translated document exits at the top.

Measured same-session, skeleton on/off (600 elements, with the guard):

| Benchmark | Off | On (C-2) | Δ |
|---|---|---|---|
| `changing` | 2.09 ms | **1.17 ms** | **−44%** |
| `markdown` | 8.75 ms | **6.27 ms** | **−28%** |
| `localized` | 2.68 ms | 2.60 ms | −3% |
| `rebuilding` | 9.25 ms | 9.44 ms | +2% |
| `idle` | 192 µs | 193 µs | 0 |

(The pre-guard skip measured changing 1.13 / markdown 5.87 — the guard
costs ~4–6% on the reuse-heavy workloads and is not optional.) markdown
is near the measured floor for this pipeline (Bevy's schedule/layout/
text measure ~4 ms, the localize walk ~0.4 ms, render + parse ~0.1 ms —
none of it rebuild machinery). The phase plan of this ADR is complete.

Also since the C-1 note: `spec_id`/`same_identity` take the provenance
wrapper, and the state walk's translation-fragment detection uses the
mapped handle.
