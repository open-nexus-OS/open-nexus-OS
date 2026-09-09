---
title: TASK-0050B Recovery bringup console (recovery-sh) — resolved by decision: not part of the end system
status: Done (2026-09-09 — resolved by decision: no recovery console in the consumer end state; nx recovery + bootctld targets are the solution)
owner: @reliability
created: 2026-08-18
depends-on:
  - TASK-0050 # boot targets (the graph this console would run in)
  - TASK-0051 # ops surface (the ONLY operations it may call)
follow-up-tasks: []
links:
  - Reliability contract: docs/rfcs/RFC-0087-reliability-failure-model-v1.md
  - Authority registry: tasks/TRACK-AUTHORITY-NAMING.md
---

## Closure 2026-09-09 (resolved by decision — Done, nothing to build)

**Goal of this ledger:** give the "recovery shell" idea one home so it cannot re-enter other
ledgers as scope drift.

**Resolution:** the consumer-grade end system has no interactive recovery console. The
capability the console would have fronted is fully shipped and policy-gated:

- TASK-0050 Done 2026-08-24 — real SBI reset + boot targets (`source/services/bootctld/src/
  machine.rs` `BootTarget`, `set_boot_target`/`set_next_boot`), three-boot recovery cycle proven
  in one uart.
- TASK-0051 Done — recovery operations surface (statefsd fsck op, bootctld slot/target ops,
  `nx diagnose` as the ONE evidence bundle; `tools/nx/src/commands/diagnose.rs`).
- TASK-0053 Done — `.nxra` signed recovery actions (`bootctld/src/nxra_gate.rs`,
  `nx recovery token make/show`, RFC-0088).

Ground truth 2026-09-09: zero console code anywhere (`recovery-sh`, `line editor`, `recovery
console` → 0 hits under `source/ userspace/ tools/ scripts/`). Real-hardware bring-up without a
host channel, should it ever be needed, gets a fresh ledger with a named board — this one is
closed. The scope sketch below stays as the drift guard: any PR adding console code is drift.


## Scope when (if) activated

- A bounded UART line editor running only in the `recovery` target's graph.
- **Built-ins are thin verbs over the TASK-0051 ops surface** — the console owns
  ZERO operation logic, no second fsck/slot/diag semantics, no second command
  language beyond verb names mirroring `nx recovery` verbs.
- Mutating verbs require `.nxra` tokens exactly like the IPC surface (TASK-0053)
  — the console is a client, not a bypass.
- No arbitrary execution, no escape hatch; built-ins only.

## Activation gate

Activate only when a concrete bringup scenario without host tooling exists
(named board / channel). Until then: any PR adding console code is drift.
