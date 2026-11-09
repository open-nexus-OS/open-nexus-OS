<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# DSL Performance

How a DSL app spends time and memory, which limits bound it, and how to measure it. The
approach is host-first (deterministic, allocation-counting tests) with QEMU/board boot
markers as the proof. The runtime model is in [runtime.md](runtime.md); the layout side is
in `docs/dev/ui/foundations/layout/layout-pipeline.md`.

## How a change reaches the screen

1. A dispatched event runs its reducer arm; the store reports which fields changed.
2. The runtime matches those fields against the dependency list the last emit recorded
   (`userspace/dsl/runtime/src/emit/deps.rs`). Each dependency carries a damage class:
   `None`, `Paint` or `Layout`. The strongest class among the changed fields is what
   `View::dispatch` returns.
3. `None` does nothing; otherwise the scene is re-emitted. `Paint` keeps the retained layout
   geometry and repaints; `Layout` re-lays out ([ir.md](ir.md), *Field classification*).

What follows for app authors:

- Put a value in a **paint** position (`.bg`, `.fg`, `.opacity`, a motion value) rather than a
  layout one (`.width`, `.padding`, text content) when it changes often — the geometry then
  survives. Which modifier is which: [modifiers.md](modifiers.md).
- Give collection items a stable `.key(expr)`: identity, per-item state and the row diff
  follow the key ([state.md](state.md)).
- Motion belongs in `.animate` / `.transition` / `.effect`: the host owns the clock and
  interpolates, and the program is not re-run per frame.
- `.scroll` makes a container a scroll layer: its content is laid out and painted once and
  the compositor moves it (see [modifiers.md](modifiers.md)).

## Limits

| Limit | Value | Enforced by |
|---|---|---|
| View nodes per program | 4096 (`maxViewNodes`) | loader, `BudgetExceeded` |
| Children per view node | 64 (`maxChildren`) | loader, `BudgetExceeded` |
| Structural nesting in source | 64 levels | parser, `NX0105` |
| Payload container (IR + locale packs) | 512 KiB, asserted at 90 % | `dsl_apps_conformance::budget_probe` |
| Frame arena | 4 MiB, two regions × two generations = 1 MiB per generation | `nexus_service_entry::FRAME_ARENA_BYTES` |
| Service heap | 64 MiB region, mapped lazily | `nexus-service-entry` |
| app-host image | 14 MiB | `scripts/check-image-budgets.sh` |
| `@persist` snapshot | 2048 bytes | app-host |
| Query page | `limit` 1–1000 | checker (`NX0410`), [db-queries.md](db-queries.md) |

The other `Budgets` fields (`maxExprNodes`, `maxListLen`, `maxStrLen`, `maxEffectSteps`,
`maxLocals`) are written into every program ([ir.md](ir.md)); the loader does not check them.

## Memory over a session

The runtime's contract is a flat base heap: after warm-up, a structural interaction costs
nothing durable. Per-frame allocations (the emitted scene, layout boxes, text runs) go to the
frame arena and are dropped by resetting a generation; durable state (store values, queues)
lives on a size-class heap that reuses freed blocks. The rules that make this safe are in
[runtime.md](runtime.md).

A frame that outgrows its arena generation spills to the heap and the leak is back. Watch
for it in the boot log of a visible run:

```text
apphost: frame arena (layouts=… gen=… base=… peak=… of=… spill=… … abandoned=…)
apphost: heap steady (layouts=… base=… peak=…)
```

`spill=0` is required; `abandoned=` counts frames that failed and were discarded;
`heap steady` is printed only when the base heap did not move between two consecutive samples.
Decode logs with `docs/testing/run-logs.md`.

## Measuring

| What | Where |
|---|---|
| Bytes one frame allocates per arena region, per real app, at the board's 1920×1080 and the lanes' 1280×800 | `tests/dsl_apps_conformance/tests/frame_arena_budget.rs` — the recorded costs are budgets; growth past them is a red test |
| The purity invariant: nothing allocated in a frame phase outlives its generation, under 64 frames of each real app with a poisoning allocator | `tests/dsl_apps_conformance/tests/arena_invariant.rs` |
| A long session never exhausts the program reader | `tests/dsl_apps_conformance/tests/long_session.rs` |
| Node count, payload size and box count of the biggest app | `tests/dsl_apps_conformance/tests/budget_probe.rs` |
| Hit-testing and partial repaint across the real shell layout | `tests/dsl_apps_conformance/tests/hover_sweep.rs` |
| A host preview of any app or state | `nx-dsl render` ([cli.md](cli.md)) |

```bash
env RUSTFLAGS='--check-cfg=cfg(nexus_env,values("host","os")) --cfg nexus_env="host"' \
  cargo +nightly-2025-01-15 test -q -p dsl_apps_conformance --test frame_arena_budget
```

Measure first, then change: a budget in a test is raised in the same change that adds the
cost and says why, never to silence a failure.
