---
title: TASK-0077C DSL v0.2c runtime long-session & large-data contract: emit-generation arena + subtree re-emit (pro primitives retired)
status: Draft (scope fixed 2026-09-20 after measuring: MEMORY only, P0-P3. Latency moved to TASK-0145B, boot to TASK-0269B)
owner: @ui @runtime
created: 2026-01-26
updated: 2026-07-06
depends-on:
  - tasks/TASK-0077-dsl-v0_2a-state-nav-i18n-core.md
follow-up-tasks: []
links:
  - Track: tasks/TRACK-DSL-V1-DEVX.md (blessed-primitive set + "no scripting creep")
  - NativeWidget contract: docs/dev/dsl/syntax.md
  - Existing virtualization to promote: userspace/ui/widgets/virtual-list (nexus-virtual-list,
    production, windowd-consumed) — promote the BEST impl, don't rebuild
  - Widget home: userspace/ui/widgets/* (NOTE: `userspace/ui/kit/` does not exist — stale ref removed)
  - Zero-copy data plane gate: tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md
  - QuerySpec paging feeding these surfaces: tasks/TASK-0078B
  - Scope firewall — latency questions found here belong THERE:
    tasks/TASK-0145B-ui-interaction-latency-budget.md
  - Scope firewall — boot time belongs THERE: tasks/TASK-0269B-boot-to-first-frame-budget.md
---

## End-state rewrite 2026-09-09 (binding; supersedes the pro-primitives scope below)

**Re-evaluation (user, 2026-09-09) — ground truth verified in code:**

- **Data virtualization is solved, and more elegantly than a VirtualList widget:** QuerySpec
  keyset paging (`limit` mandatory 1..=1000, opaque tokens, offsets banned —
  `docs/dev/dsl/db-queries.md` "Keyset paging contract", `runtime/src/effects.rs`) + the
  scroll container's `EndReached` trigger → `LoadMore` (`userspace/apps/chat/ui/pages/
  ChatPage.nx:111`, `chat.store.nx:75-84`) + the store-window builtin `tail(list, n)`
  (`chat.store.nx:51`, IR `ListOpKind::tail`) keep the resident list bounded regardless of the
  source size. Paint is visibility-indexed (cost = screen content), scroll is a paint-only
  offset (no re-layout per notch). Lint NX0404 `UnboundedFor` exists. stash sorts in its store
  (`sortBy`/`sortDir`, app-host sorts) — no Table widget is needed.
- **Therefore the VirtualList widget, the `nexus-virtual-list` promotion into the DSL, a
  Table/Grid widget crate, Timeline and NativeWidget hosting are RETIRED from this ledger**
  (Timeline/NativeWidget stay demand-gated with their future consumer apps, > 0080).
- **The one real scale ceiling lies underneath:** the app-host heap is a never-freeing bump
  allocator (`source/libs/nexus-service-entry/src/lib.rs:101-110`), and every structural
  interaction re-emits scene + layout + texts onto it (~100–300 KiB per click;
  `userspace/dsl/runtime/src/view.rs:11` — "subtree-scoped re-emit … recorded follow-up").
  8 MiB froze after a few dozen clicks; the 16 MiB `heap-16m` feature is headroom, not a fix
  (TASK-0311:188-201: "the honest fix (emit-generation arena) … backlog"). Every long-lived app
  freezes after a bounded number of interactions — independent of lists. Ownership of that fix
  moves HERE (TASK-0311 updated).

### Measured 2026-09-20 (replaces the numbers quoted from a code comment)

A counting host allocator over the REAL compiled `desktop-shell`, 100 structural dispatches
(panel open + absorber), gives the three numbers this task rests on:

| | measured |
|---|---|
| retained emit output, one generation | **68 270 B** |
| allocated per structural interaction | **50 765 B** |
| live drift over 100 dispatches | **0 B** |

Consequences, now arithmetic rather than impression:

- **The ceiling is ~330 interactions.** 16 MiB ÷ 50 KiB. The app-host carries 16 MiB to survive
  roughly three hundred clicks, against a working set of 68 KiB — a factor of 240.
- **Drift 0 is the load-bearing finding.** The moment an allocator actually frees, nothing
  accumulates: every byte of that 50 KiB is garbage, so on the never-freeing bump it leaks in
  full. An arena does not need to be clever — the precondition is proven, not assumed.
- **The arena sizes itself: ~256 KiB.** (50 KiB churn + 68 KiB retained) × 2 generations,
  plus margin — not the megabytes the current heap implies.
- The code comment in `nexus-service-entry` says "~100-300KiB/click"; the shell's measured
  cycle is 50 KiB. Neither number is gospel; the probe in P1 is what the budget will cite.

### Goal (end system)

An app's memory is flat over an unbounded number of interactions and pages: per-generation
emit output lives in a reusable arena, re-emit cost is proportional to the changed subtree,
and the language's store-window rule (`tail()` + QuerySpec `limit`) is the documented
virtualization contract.

### Non-goals

VirtualList / Table / Grid widget crates; Timeline and NativeWidget hosting (consumer-app
tasks); a general free-list allocator for OS services (rejected: nondeterministic
fragmentation, service floor stays bump — see D1 rationale); kernel changes.

### Invariants

- Heap high-water mark flat after warm-up: N dispatches (N ≥ 10 000 on the host, ≥ 500 in
  QEMU) do not grow `hwm` beyond generation 2.
- Stores (durable state) live on the base heap; ONLY emit output (scene, layout boxes, text
  runs, handler tables) lives in the generation arena.
- Determinism unchanged: same IR + inputs ⇒ identical frames; goldens byte-identical before
  and after.
- Zero-alloc steady scroll unchanged. ⚠️ **This is a CLAIM, not a gate** (verified
  2026-09-20: `zero_alloc` exists in the tree only for blur). It is carried here as an
  invariant nobody can check; TASK-0145B P4 either gates it or deletes the sentence.

### Decisions

- **D1 Emit-generation arena.** `userspace/dsl/runtime/src/arena.rs`: two fixed-capacity
  generation buffers (`GEN_ARENA_BYTES`, sized from the largest app scene + margin, asserted by
  a budget probe); `View::dispatch` emits generation g+1 into the idle buffer, swaps, and the
  previous buffer is reset wholesale (no per-object free). app-host hands the arena its backing
  range once (`nexus-service-entry` exposes an arena carve-out, base heap unchanged). Rationale
  vs a free-list allocator: bump + generation reset is O(1), fragmentation-free and
  deterministic — the property every OS service relies on; a free-list would import the
  fragmentation problem into every service for one consumer.
- **D2 Dual structure deleted.** Once D1 is proven, `heap-16m` for app-host returns to the
  honest floor (`heap-4m` + arena) and the 50/75/90 % watermark-workaround markers are removed;
  gate = `just contract-image-budgets` (24 → 14 MB ceiling restored) + the heap-steady test.
  No "keep 16 MiB just in case".
- **D3 RETIRED from this ledger 2026-09-20 — subtree-scoped re-emit moved to TASK-0145B P3.**
  Its justification here was memory, and that justification does not survive D1: once emit
  output lives in a generation arena, churn is FREE. What subtree re-emit actually buys is less
  work per interaction — latency — which is the other task's subject. Keeping it here would be
  precisely the scope creep this ledger is being fenced against.
- **D4 Store-window rule as language contract.** `docs/dev/dsl/patterns.md` "Large data & long
  sessions": `tail()` + QuerySpec `limit` are the virtualization; NX0404 verified to cover a
  `List` over a store list that grows without `tail()`/`limit` — extended only if a gap is
  proven by a fixture.
- **D5 Proof home.** `tests/dsl_conformance` (heap/emit counters via the host runtime) + the
  app-host visible lane; no new crate.

### Packages

- **P0** ADR "emit-generation arena in the DSL runtime" (docs/adr, next free number) +
  `architecture-review`. Blast: paper.
- **P1** Arena + View integration + heap-steady host test (10 000 dispatches over the chat +
  settings scenes). Blast: `dsl_conformance`, `dsl_goldens`, `dsl_apps_conformance`.
- **P2** app-host wiring + QEMU marker `apphost: heap steady (gen=<n> hwm=<bytes>)` (printed
  with numbers after 500 scripted interactions in the visible lane). Blast: visible, smp1.
- **P3** D2 deletion (heap floor back, watermark markers out, image budget restored).
  Blast: `contract-image-budgets`, every app-host lane.
- **P4 / P5 RETIRED.** P4 (subtree re-emit) → TASK-0145B P3, see D3. P5 (docs) is not a
  package: the docs sweep belongs to *Done*, as it has in every package of TASK-0077B.

**Four packages, and the fence is part of the task:** a latency, boot, jank or frame-pacing
question found while building this one goes into TASK-0145B or TASK-0269B as a line, never into
a package here.

### Definition of Done

Host: heap-steady test (hwm flat), goldens unchanged, and the NX0404 question answered by a
FIXTURE rather than an opinion — a `List` over a store list that grows without `tail()`/`limit`
either trips a lint or it does not. If it does not, that is one documented sentence in
`state.md`, not a new ledger; `tail()` appears in exactly one file in the whole corpus today
(`chat.store.nx:51`), so the "store-window rule" is a convention with one user, and inventing
enforcement before a fixture proves the gap would be the same creep in another direction.
(The emit-counter fixture moved out with D3.) QEMU (`proof-manifest/markers/ui.toml` visible profile + the visible
lane's display-truth block + `markers.txt` only if an app-host chain contract exists):
`apphost: heap steady (gen=<n> hwm=<bytes>)`; the `heap-watermark` markers are gone from the
contract. Docs: `patterns.md` large-data chapter, `runtime.md`, `perf.md`, ADR.

### Touched paths

`userspace/dsl/runtime/src/{arena.rs (new),view.rs,emit.rs}`, `source/libs/nexus-service-entry/
src/lib.rs` (arena carve-out; approval zone), `source/services/app-host/{Cargo.toml,src/probe/
mount.rs}`, `scripts/check-image-budgets.sh`, `tests/dsl_conformance/`, `docs/dev/dsl/
{patterns,runtime,perf}.md`, `docs/adr/`.

### Dependencies

TASK-0077B (IR v1.3, keyed state — instance keys are the subtree identity D3 splices on).
None on TASK-0324.

## Context (updated 2026-07-06) — historical, superseded by the end-state rewrite above

Hard apps (office/BI, audio workstation, video editing) need a few "pro surfaces" that
must not turn the DSL into an unbounded scripting language. The answer is (a) a small
set of virtualized/timeline **contracts** rendered by first-party widgets, and (b) one
**blessed NativeWidget path** for heavy canvases — same determinism/boundedness rules
on every tier.

**Sequencing (masterplan):** the **virtualized `List` core is pulled forward into the
Phase-4 wave** (TASK-0077 consumer contract) because the master-detail demo (0078) and
the launcher grid (0080B) need it. The rest of this task — table/grid, timelines,
NativeWidget hosting — is **demand-gated**: it lands when the first real consumer app
task (0092 PDF / 0098 rich text / 0100B mixer / office wave) pulls it, per the track's
"app-driven capability expansion" rule. Do not build speculative primitives.

**IST corrections:** virtualization already exists in production
(`nexus-virtual-list`, used by windowd) — the DSL windowed-ForEach must promote/wrap
it, not re-implement scroll physics. `userspace/ui/kit/` never existed; widgets live in
`userspace/ui/widgets/*`.

## Goal — historical, superseded by the end-state rewrite above

1. **Virtualized list (pulled into Phase 4)**: windowed keyed ForEach backed by
   `nexus-virtual-list` physics; stable keys, bounded live instances, deterministic
   window mapping; QuerySpec page-token hook (backpressure: at most one page fetch in
   flight).
2. **Table/grid contract** (demand-gated): column model as data, bounded rows/columns
   per viewport, deterministic ordering + selection semantics, paging tokens.
3. **Timeline contract** (demand-gated): tracks/clips/keyframes as data; deterministic
   zoom/scroll mapping; bounded rendering; selection semantics shared across consumers.
4. **NativeWidget hosting** (demand-gated): registry of capability-gated handles
   (registered at build, no dynamic loading); the runtime hosts the widget as a leaf
   with the standard invalidation contract; strict rules — deterministic given inputs,
   bounded CPU/memory per frame, no direct IO (svc.* via effects only), a11y required;
   host golden strategy (scripted input replay → frame-sequence goldens).

## Non-Goals

- Any scripting/expression growth in the DSL for these surfaces (data + contracts
  only). Building actual office/DAW/video apps. Kernel changes.

## Constraints / invariants (hard requirements)

- Virtualization mandatory for large collections (lint: unbounded non-virtualized
  collection over budget = error).
- Deterministic input replay for pro widgets (bounded event streams, no wall-clock).
- Explicit cache budgets/eviction for every pro surface; zero-alloc steady scroll.
- Promote-best rule: wrap `nexus-virtual-list`; no parallel scroll/window
  implementation.
- No `unwrap/expect`; no godfiles.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/ui_pro_primitives_host/`:

- windowed list: deterministic window mapping fixtures (scroll position → live
  instance set), reorder/insert stability, page-fetch backpressure fixture
  (Phase-4 portion);
- table/grid + timeline: deterministic goldens for fixtures + scripted interaction
  frame sequences (when demand-gated portion lands);
- NativeWidget: budget-enforcing harness proves bounded frame cost; determinism via
  input-replay goldens (when it lands).

### Docs — required (reference grade)

- `docs/dev/dsl/patterns.md`: large-data chapter (virtualization + paging);
- NativeWidget blessed-path guidance in `docs/dev/dsl/syntax.md` +
  per-contract docs under `docs/dev/ui/` as portions land.

## Touched paths (allowlist)

- `userspace/ui/widgets/*` (table/grid/timeline widget crates when pulled)
- `userspace/dsl/runtime/` (windowed ForEach, NativeWidget host leaf)
- `tests/ui_pro_primitives_host/` (new)
- `docs/dev/dsl/{syntax,patterns}.md`, `docs/dev/ui/`

## Plan (small PRs)

1. windowed List core (rides with Phase 4 / TASK-0077)
2. [demand-gated] table/grid contract + widget + goldens
3. [demand-gated] timeline contract + widget + goldens
4. [demand-gated] NativeWidget registry + hosting + replay harness + docs
