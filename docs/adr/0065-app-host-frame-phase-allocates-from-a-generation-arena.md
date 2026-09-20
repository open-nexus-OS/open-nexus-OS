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
  arena; outside it, from the base heap exactly as today. Services that never open a scope are
  bit-for-bit unaffected.
- The arena holds **two generations**. Opening generation *g+1* makes *g* the retained one and
  resets *g-1* **wholesale** — one pointer, no per-object free. Two, because app-host provably
  needs exactly two: it keeps the previous boxes and texts alive to diff against the new ones
  (`probe/interaction.rs` takes `old_boxes`/`old_texts` before `relayout_retained`).
- The scope wraps the **layout + text phase** (`relayout_retained`), which is the dominant cost
  and is free of store writes — it reads `view.scene()` and writes `self.layout` / `self.texts`
  and nothing else.
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
- **New failure mode, deliberately chosen:** a frame phase that allocates more than the arena
  holds fails at a *known* point with a named error, instead of silently consuming headroom
  until the service dies at an arbitrary later click. Exhaustion becomes an assertion rather
  than a freeze.
- **New invariant to keep honest:** an allocation made inside a scope must not outlive its
  generation. This is the one real hazard. It is contained by keeping the scope narrow (one
  phase, no store writes) and by the task's proof, and it is why the scope is a closed API and
  not a free-floating "arena mode" a caller can leave open.
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
