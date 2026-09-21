# ADR-0065: The app-host's frame-producing phase allocates from a generation arena, and the service allocator owns the scope

- Status: Proposed
- Date: 2026-09-20
- Links:
  - Tasks: `tasks/TASK-0077C-dsl-v0_2c-runtime-long-session-large-data-contract.md` (execution + proof)
  - Scope firewall (latency questions belong there, not here):
    `tasks/TASK-0145B-ui-interaction-latency-budget.md`
  - Allocator this narrows: `source/libs/nexus-service-entry/src/lib.rs` (the service bump)
  - Where the workaround lives today: `heap-16m` + the 50/75/90 % `heap-watermark` markers
  - Related ADRs: `docs/adr/0062-boot-stage-fence-and-readiness-barriers.md` (stage ordering this
    must not disturb)

## Context

Every OS service in this tree allocates from a **never-freeing bump**
(`nexus-service-entry`): `dealloc` is a no-op, the pointer only moves forward. That is a
deliberate, load-bearing choice — bump allocation is O(1), fragmentation-free and
deterministic, which is what a microkernel service floor needs. It is correct for a service
that allocates a bounded working set once.

The app-host is not such a service. It re-produces a frame on every structural interaction,
and each of those frames is thrown away. Measured 2026-09-20 with a counting host allocator
against the REAL compiled `desktop-shell`:

| | measured |
|---|---|
| layout, one call | **226 560 B allocated**, 119 824 B retained (136 boxes) |
| emit, non-structural dispatch | 59 B |
| emit, structural dispatch | ~100 KiB |
| **live drift over 100 dispatches** | **0 B** |

Two facts decide this ADR.

**First: it is all garbage.** Live drift is zero — the instant an allocator actually frees,
nothing accumulates. So on the bump, ~100 % of a frame's allocation leaks by construction. The
ceiling is arithmetic, not bad luck: 16 MiB ÷ ~200–330 KiB per interaction ≈ **50–85
interactions before the app-host freezes**. The heap history in the code reads as a series of
raises — 4 → 8 → 16 MiB — each buying a few dozen more clicks, and the comment at
`nexus-service-entry` says so in its own words: *"the honest fix (emit-generation arena)"*.

**Second: the cost is not where the task ledger assumed.** `TASK-0077C` D1 placed the arena in
`userspace/dsl/runtime/src/arena.rs`. But the dominant allocator is `nexus_layout` — **226 KiB
per layout against ~100 KiB for a structural emit** — and layout runs in app-host, not in the
DSL runtime. A runtime-local arena would recycle the smaller half. The scope has to sit where
the allocator does.

A free-list or general-purpose allocator was the obvious alternative and is rejected below: it
would import fragmentation and nondeterminism into **every** service to serve one consumer.

## Decision

**The app-host's frame-producing phase runs inside an allocator GENERATION, and the service
allocator owns that scope.**

- `nexus-service-entry` gains a second, fixed-capacity region — the generation arena — carved
  out beside the base heap, and a scope API. Inside the scope, allocation comes from the
  arena; outside it, from the base heap exactly as today. The region is **opt-in per service**
  (`frame-arena`): without it the array is zero-sized, so nothing else pays a byte.

  Measured rather than asserted (2026-09-20, `logd` release for `riscv64imac-unknown-none-elf`,
  before vs after): **`.text` 43 734 B identical, `.bss` 393 264 B identical**. The ELF file
  differs by −2 160 B in non-loaded debug sections only, a side effect of splitting the UART
  writers into their own module. An earlier draft of this ADR claimed "bit-for-bit unaffected";
  that was not measured and is not true of the file — it is true of the code and the memory,
  which is what the claim needed to be.
- The arena holds **two generations**. Opening generation *g+1* makes *g* the retained one and
  resets *g-1* **wholesale** — one pointer, no per-object free. Two, because app-host provably
  needs exactly two: it keeps the previous boxes and texts alive to diff against the new ones
  (`probe/interaction.rs` takes `old_boxes`/`old_texts` before `relayout_retained`).
- **Two regions, each its own pair of generations** (amended 2026-09-21, TASK-0077C P2b): the
  layout + text phase (`relayout_retained`) opens `Region::Layout`; the DSL runtime's emission
  (`View::emit`) opens `Region::Emit` through a `FrameScope` the app-host installs once after
  mount. They are separate on purpose. Their retained outputs have independent lifetimes — a
  scene is read by its own layout and by the next hit-test, boxes by the next frame's row diff
  — so each resets on its own schedule, and a paint-only frame (emit without layout) cannot
  reset the boxes it still paints from. Sharing one pair would have forced the reduce and the
  emit apart at the runtime's API; separate pairs make that unnecessary.
- **Emission is pure with respect to durable state, and that is a contract, not a hope.**
  Inside the emit scope the runtime builds the scene and resolves handlers and motion intents
  against it, and every retained output is a FRESH value each frame. The live-instance sweep
  over the stores and the text-focus record (which clones a path and a payload) run after the
  guard has dropped, on the ordinary heap. A retained buffer kept across frames with `clear()`
  is forbidden: it is written in generation g+1 and reset underneath its owner in g+2.
- **Durable state never enters a generation.** Stores, the mounted program, the IR, the view's
  own retained scene: base heap. The rule is not "what is big" but "what outlives one frame".
- A generation is never reset while anything still reads it. Since generations are reset only
  at the start of the next-but-one frame phase, the invariant is checkable by construction and
  is what the proof in the task asserts.
- The `heap-16m` feature and the 50/75/90 % `heap-watermark` markers are **deleted** once the
  arena is proven, and the image budget returns to its pre-workaround ceiling. A workaround
  kept "just in case" beside its fix is the dual structure this project removes on sight.

## Consequences

- App-host memory becomes **flat over an unbounded number of interactions**, which is the
  property a long-lived UI process needs and the one it does not have today.
- The heap floor drops from 16 MiB to a base heap plus a bounded arena. Sizing is measured, not
  guessed: ~2 × (retained boxes + texts + scene) plus churn headroom, asserted by a budget
  probe in the task — an early estimate of 256 KiB was made against the emit-only figure and
  was wrong; the layout numbers above are why the probe exists.
- **Exhaustion is loud, not fatal (revised during P1).** A frame that outgrows its generation
  SPILLS the remainder to the base heap and sets a sticky `spilled` flag the budget probe
  reports and the gate asserts stays false. The service keeps exactly the behaviour it has
  today — a base-heap allocation — instead of dying mid-frame, and the condition is visible
  and gateable rather than silent. An earlier draft made exhaustion a fatal allocation failure;
  killing a UI on a large frame is not an improvement on leaking slowly, and "silent" was the
  property worth ending, not "survivable".
- **The invariant is ENFORCED, not trusted — twice.** On the host,
  `tests/dsl_apps_conformance/tests/arena_invariant.rs` replaces the global allocator with a
  two-generation arena that POISONS a reset generation and every block freed inside one, then
  drives the real apps (desktop-shell, stash, greeter text focus) for 64 frames each. On the
  first attempt at scoping emission it aborted in `drop_in_place<HandlerEntry>` with
  "`unchecked_mul` cannot overflow" — a poisoned `Vec` capacity, from `self.handlers.clear()`
  reusing a buffer across frames; that, not a store write, was the `alloc-fail
  size=0x20000004d` in the boot log. On the OS, `frame-arena-poison` fills the bytes a reset
  generation gave up with `0xDE` (one memset of the USED bytes, ~48-78 KiB), so a stale
  reader fails at once instead of reading the frame from two frames ago. App-host keeps it on.
- **New invariant to keep honest:** an allocation made inside a scope must not outlive its
  generation. `alloc_zeroed` is the sharp edge and is handled: the base bump can skip zeroing
  because it only ever hands out untouched `.bss`, but the arena REUSES memory, so it zeroes
  explicitly — without that it would hand back the frame from two frames ago. This is the one real hazard. It is contained by keeping the scope narrow (one
  phase, no store writes) and by the task's proof, and it is why the scope is a closed API and
  not a free-floating "arena mode" a caller can leave open.
- **What the arena does and does not buy, measured (2026-09-21, visible boot, poison on):**
  the frame is flat — arena peak 76 733 B across both regions, spill 0, no allocation fault —
  and the base heap still advances **432 B per layout** (28 129 with layout alone scoped,
  75 018 with no arena). The residue is the REDUCE path: per-dispatch transients and, above
  all, durable state overwritten on a heap that never frees. That is outside this ADR's
  subject by design — durable state must never enter a generation — and it bounds a session
  at ~39 000 interactions. The follow-on (TASK-0077C P3) is a per-service, opt-in, size-class
  free list for app-host's base heap. This ADR's rejection of "a free-list allocator" is about
  a GENERAL one on every service's floor; it does not speak against that.
- **Durable state frees, by size class (amended 2026-09-21, TASK-0077C P3).** The residue
  above is answered where it lives: `nexus-service-entry` gains opt-in size-class free lists
  (`small-object-free-list`) — eight classes from 16 to 2048 bytes, 16-aligned blocks, LIFO
  reuse through an intrusive link in the freed block, no splitting, no coalescing, no search.
  `dealloc` checks the arena FIRST (a generation's blocks are reclaimed by its reset and must
  never be parked, or the list would hand out memory the next reset overwrites), then returns
  a small durable block to its class; `alloc` consults the class before the bump and carves
  new small blocks class-rounded so any later request of the class can take them;
  `alloc_zeroed` zeroes a reused block, as it does an arena one. Larger or over-aligned
  requests stay on the bump. Every operation is O(1) and a function of the request sequence
  alone — the property this ADR's rejection of a GENERAL free list was protecting — and a
  session's working set becomes bounded by its peak per class. With `frame-arena-poison` a
  freed block is filled with `0xDE` after its link, so a use-after-free of durable state is as
  loud as a use-after-reset. The rule is proven on the host in `freelist.rs`; the wiring is
  proven by the boot marker `apphost: heap steady`, which this ADR's first two rounds could not
  produce and did not declare.
- Every other service keeps the bump exactly as it is. No service floor changes.
- `nexus-service-entry` is an approval zone; this ADR is the record that the change is a
  narrowing of one allocator's behaviour under an explicit scope, not a new allocator.

## Alternatives considered

- **A free-list / general-purpose allocator for services.** Rejected. It would make `dealloc`
  meaningful everywhere and solve this by accident — at the cost of fragmentation and
  nondeterministic allocation latency in every service, including the ones on the boot path.
  One consumer's churn does not justify changing the floor every service stands on.
- **Keep raising the heap.** Rejected, and on the record as already tried: 4 → 8 → 16 MiB, each
  buying a few dozen clicks. The ceiling is linear in heap size and the churn is unbounded, so
  no finite raise is a fix.
- **An arena inside the DSL runtime** (`TASK-0077C` D1 as originally written). Rejected by
  measurement: it would recycle the ~100 KiB emit and leave the ~226 KiB layout leaking.
- **Make the DSL runtime and layout engine allocator-generic** (`allocator_api`). Rejected: it
  is unstable, it would infect every collection type in the UI stack, and it buys nothing the
  scope does not — the phase boundary is already exact.
- **Reference-count or GC the scene.** Rejected: it trades a bounded, deterministic reset for
  per-object bookkeeping in the hot path, against a project invariant.
