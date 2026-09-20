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
| **layout, one call** | **226 560 B allocated**, 119 824 B retained (136 boxes) |
| emit, structural dispatch | ~100 KiB (non-structural: **59 B**) |
| retained emit output, one generation | 68 270 B |
| live drift over 100 dispatches | **0 B** |

⭐ **The second round of measurement moved the arena.** The first pass measured `view.dispatch`
alone and gave 50 765 B — but app-host also LAYOUTS and re-measures text on every structural
interaction (`probe/scroll.rs::relayout_retained`), and layout allocates **226 KiB per call**,
roughly twice what it retains. Layout, not emit, is the cost — and `nexus_layout` runs in
app-host, not in the DSL runtime. See D1.

Consequences, now arithmetic rather than impression:

- **The ceiling is ~50-85 interactions.** 16 MiB ÷ ~200-330 KiB. Which finally reconciles the
  code comment's story: 8 MiB ÷ ~200 KiB ≈ 40 clicks, and the comment says 8 MiB "froze after a
  few dozen clicks". The heap history — 4 → 8 → 16 MiB — is a series of raises each buying a few
  dozen more, and no finite raise is a fix because the churn is unbounded.
- **Drift 0 is the load-bearing finding.** The moment an allocator actually frees, nothing
  accumulates: every byte of that 50 KiB is garbage, so on the never-freeing bump it leaks in
  full. An arena does not need to be clever — the precondition is proven, not assumed.
- **Arena sizing is ~2 × (retained boxes + texts + scene) + churn headroom** — measured by the
  P1 budget probe, not guessed. An earlier estimate of 256 KiB in this ledger was made against
  the emit-only figure and is WRONG; it is left visible here because it is exactly why the probe
  exists.
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

- **D1 Generation arena, in the SERVICE ALLOCATOR — not in the DSL runtime** (ADR-0065,
  revised 2026-09-20 by measurement). `nexus-service-entry` gains a second fixed-capacity
  region beside the base heap plus a SCOPE: inside the scope allocation comes from the arena,
  outside it from the base heap exactly as today, so a service that never opens a scope is
  bit-for-bit unaffected. Two generations, because app-host provably needs exactly two — it
  holds `old_boxes`/`old_texts` to diff against the new ones (`probe/interaction.rs`). Opening
  *g+1* resets *g-1* wholesale: one pointer, no per-object free.

  app-host opens the scope around the **layout + text phase** (`probe/scroll.rs::
  relayout_retained`) — the dominant allocator, and verified free of store writes: it reads
  `view.scene()` and writes `self.layout` / `self.texts`, nothing else. **Durable state never
  enters a generation**; the rule is not "what is big" but "what outlives one frame".

  *Why not the original placement:* `userspace/dsl/runtime/src/arena.rs` would have recycled
  the ~100 KiB emit and left the ~226 KiB layout leaking, because the layout engine runs in
  app-host. *Why not a free-list allocator:* it would import fragmentation and nondeterministic
  allocation latency into every service — including the boot path — to serve one consumer.

  **The one real hazard, named:** an allocation made inside a scope must not outlive its
  generation. Contained by keeping the scope to one phase with no store writes, by making it a
  closed API rather than a mode a caller can leave open, and by P1's proof.

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

### architecture-review verdict (P0, 2026-09-20)

- **Scope** — in: `source/libs/nexus-service-entry` (second region + a closed generation-scope
  API; approval zone), `source/services/app-host` (opens the scope around `relayout_retained`,
  heap feature back to the honest floor), `scripts/check-image-budgets.sh`, the proof-manifest
  marker, `tests/dsl_conformance`. Explicitly OUT: the DSL runtime (measurement says the cost
  is not there), any change to `dealloc` semantics for unscoped services, subtree re-emit
  (→ TASK-0145B P3), and every latency/boot question (→ TASK-0145B / TASK-0269B).
- **Invariant** — nothing allocated inside a generation outlives it, and durable state never
  enters one. Negative proof: `test_reject_*` that a store write inside a scope is not
  reachable (the scope is a closed API around a phase that provably performs none), plus a
  host test that a generation's memory is REUSED after reset — a reset that quietly keeps
  growing would pass a "hwm flat" test on a big enough heap.
- **Contract** — ADR-0065, narrowing `nexus-service-entry`'s allocator under an explicit
  scope. No RFC: no new syscall, wire format or service API; the ABI is untouched.

### Packages

- **P0** ✅ ADR-0065 + `architecture-review` verdict + this revision. Blast: paper.
- **P1** ✅ **2026-09-20** The scope in `nexus-service-entry`. `src/generation.rs` is the RULE
  with no allocator, no statics and no cfg gate, so it is proven on the host (7 tests); the
  OS-only `GlobalAlloc` glue asks it where the next byte goes. `frame-arena` is opt-in, so a
  service that does not enable it has a zero-sized region. `FrameGeneration` is a Drop guard,
  not an open/close pair — a scope left open by an early return is the one hazard this design
  has, and a guard removes it. `arena_stats()` is the probe's read side for P2.

  **The unscoped-service claim, measured instead of asserted** (`logd`, release, riscv64):
  `.text` 43 734 B and `.bss` 393 264 B **identical** before and after; the ELF differs by
  −2 160 B in non-loaded debug sections only. ADR-0065 said "bit-for-bit unaffected" — that was
  never measured and is false of the FILE; it is true of the code and the memory, and the ADR
  now says so.

  ⭐ **Two corrections the build forced, both recorded in the ADR:**
  1. **`alloc_zeroed` would have handed back the frame from two frames ago.** The base bump can
     skip zeroing because it only ever returns untouched `.bss`; the arena reuses memory. It
     zeroes explicitly now. This is the exact class of bug the "nothing outlives its
     generation" invariant exists for, and it was one line from shipping.
  2. **Exhaustion is loud, not fatal.** The first design made an oversized frame an allocation
     failure, i.e. the service dies mid-frame. A frame that outgrows its generation now SPILLS
     to the base heap and sets a sticky flag the gate asserts stays false — today's behaviour,
     made visible. "Silent" was the property worth ending; "survivable" was not.

  Also: `lib.rs` hit the 600-LOC ratchet, so the UART writers moved to `src/debug_write.rs` —
  a real seam (every caller runs where allocation is impossible: the allocator, the panic
  handler, the alloc-error handler), not a dumping ground.
- **P2** ✅ **2026-09-20** app-host opens the scope inside `relayout_retained` — inside the
  function, not at its ten call sites, so it cannot be forgotten — and reports the numbers.
  `apphost: frame arena (layouts=… base=… peak=… of=… spill=…)` states what IS;
  `apphost: heap steady (…)` is printed ONLY when the base heap did not move between two
  samples, so it is a detector, not a decoration.

  ⭐ **Measured A/B over a real visible boot** (13 layouts apart, same profile):

  | | base heap per layout | ceiling at 16 MiB |
  |---|---|---|
  | without the arena | **75 018 B** | ~223 frames |
  | with the arena | **28 129 B** | ~596 frames |

  The arena takes **46 889 B per frame** and holds a **flat peak of 47 880 B** from the second
  layout on, `spill=0` — and the two numbers cross-validate, which is why this is a measurement
  rather than a hope.

  ⭐⭐ **And it is NOT flat, so P2 does not claim it is.** 28 129 B per frame still leaks
  outside the scope: the runtime's EMIT (a new scene + handler table per interaction) and the
  paint path run in `view.pointer_scrolled`/`dispatch`, not in `relayout_retained`. The ceiling
  moves 223 → 596 frames — a 2.7× improvement and a real one, but a long-lived app still dies,
  just later. `heap steady` correctly never printed in any boot.

  Scoping emit needs the runtime to separate REDUCE from EMIT, because a reduce writes stores
  and stores must never enter a generation. That is a package, not a tweak → **P2b**.

  Sizing note: the arena is 1 MiB against a measured boot-scene peak of 47 880 B. That is
  deliberate headroom, not sloppiness — the peak comes from boot scenes only, and a richer page
  (Control Center open, a long list) allocates more; `spill` is the signal if one ever exceeds
  it. Tightening belongs after more scenes are exercised.

- **P2b (NEW, from P2's measurement)** Scope the EMIT phase. Requires separating reduce from
  emit in `dsl/runtime` so the store writes stay on the base heap while the scene and handler
  table go to the generation. Gate: `apphost: heap steady` actually prints — which is the
  marker P2 built and deliberately did not declare, because declaring a marker the system
  cannot yet produce is the fake-green this tree removes on sight.
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
