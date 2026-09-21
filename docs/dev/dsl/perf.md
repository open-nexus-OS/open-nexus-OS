<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# DSL Performance

This doc captures how to measure and improve DSL performance:

- where the cost actually is (layout/paint/present, emit churn — not expression
  interpretation; an AOT tier was retired 2026-09-09, TASK-0079),
- snapshot perf gates,
- deterministic benchmarks (host-first, QEMU-gated).

At runtime, performance work should follow the retained UI pipeline contract:

- stable Scene-IR / retained-tree identity,
- deterministic text preparation and measurement,
- narrow invalidation (`paint-only`, `place-only`, `measure+place`, `text-prep+measure+place`),
- and bounded caches for large collections and responsive surfaces.

See also:

- `docs/dev/dsl/runtime.md` (the one execution tier and its scale contract, TASK-0077C)
- `docs/dev/ui/foundations/layout/layout-pipeline.md`

## Memory over a session — measured (TASK-0077C, 2026-09-21)

The runtime's memory contract is a flat base heap: after warm-up, a structural
interaction costs nothing durable. The numbers below are the measurement that
sized the contract; provenance is the boot log (`build/logs/visible--*/uart.log`,
`apphost: frame arena (…)` / `apphost: heap steady (…)`) and the host harness
(`tests/dsl_apps_conformance/tests/arena_invariant.rs`), both on the desktop shell
at 1280×800.

| Quantity | Value | Where it comes from |
|----------|-------|---------------------|
| One layout pass, total allocation | 226 560 B (119 824 B retained across the call) | host harness, `SYSTEM_BYTES` around `relayout_retained` |
| One structural emit, total allocation | ~100 KiB | host harness, scope byte counter |
| Live drift per interaction (before the arena) | 0 B live, 50 765 B churn | `just test-os visible`, 2026-09-20 measurement that fenced the task |
| Base heap growth per layout, no arena | 75 018 B | boot log, `heap-16m` era |
| … with the layout region scoped | 28 129 B | boot log, P2 |
| … with emit + layout scoped | 432 B | boot log, P2b |
| … with size-class free lists (P3) | 0 B after warm-up (`heap steady`, 105 880 → 105 880 B) | boot log, P3 (two boots) |
| Arena peak | 76 733 B of 1 MiB, `spill=0` | `apphost: frame arena` sample, constant across boots |
| Warm-up carves after the first frame | 5 (16 / 8 / 4 KiB at gen 1, 2 × 64 B at gen ≈ 10 / 22), 28.8 KB total | `alloc-carve` diagnostic, constant over five boots of different length |
| app-host image | 7.39 MB of a 14 MB budget | `scripts/check-image-budgets.sh` |

What the table proves and what it does not: the host harness proves the purity
invariant (nothing allocated in a scope outlives its generation) under 64 frames
× 3 apps with poisoning; the boot proves a flat base over the shell's own warm-up
(~14 layouts). A boot under hundreds of scripted structural interactions is
TASK-0145B P3b — the visible profile's injector holds the only QMP client.
