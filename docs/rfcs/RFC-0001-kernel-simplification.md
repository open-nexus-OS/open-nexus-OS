# RFC-0001: Kernel Simplification (Logic-Preserving)

- Status: Draft
- Owners: @kernel-team, @runtime
- Created: 2025-10-24
- Last Updated: 2026-02-09
- Links:
  - Tasks: `tasks/TASK-0011-kernel-simplification-phase-a.md` (execution + proof)
  - Kernel overview: `docs/architecture/01-neuron-kernel.md`

## Status at a Glance

- Phase A (text-only headers + docs): ⬜
- Phase B (physical reorg: moves + wiring only): ⬜

Definition:

- "Complete" means the contract is defined and the proof gates are green. It does not mean "never changes again".

## Scope boundaries (anti-drift)

This RFC is a design seed / contract. Implementation planning and proofs live in tasks.

- This RFC owns:
  - The responsibility taxonomy and naming guidance for kernel subsystems.
  - The required kernel module header fields and what they mean.
  - The decision that physical reorganization (if done) must be logic-preserving and proof-gated.
- This RFC does NOT own:
  - The execution checklist, file-by-file move plan, touched paths allowlist, or proof commands.
  - Ownership/type-system refactors (separate tasks).
  - SMP implementation work (separate tasks).
  - Any syscall ABI changes.

### Relationship to tasks (single execution truth)

- Tasks define stop conditions and proof commands.
- This RFC links to the task(s) that implement and prove each phase.

## Context

The NEURON kernel is functionally progressing, but navigation and comprehension costs increase over time:

- It becomes harder to find the right subsystem when debugging.
- Invariants are present in code but not consistently documented at module boundaries.
- Tests exist but often lack a clear statement of scope and scenarios.
- A delayed directory reorganization becomes progressively more painful (merge churn, stale links).

The goal is to improve clarity without changing runtime behavior.

## Goals

- Faster orientation (clear responsibilities, fewer tokens to grok intent).
- Explicit invariants, dependencies, and test scope per module.
- Logic-preserving work only (no behavior changes).
- If a physical reorganization is performed, it results in a stable, responsibility-aligned directory tree.

## Non-Goals

- Performance tuning, algorithmic changes, or broad API redesigns.
- SMP bring-up (separate task).
- Immediate subcrate split (can be a later follow-up).

## Constraints / invariants (hard requirements)

- Determinism: marker/proof strings required by `scripts/qemu-test.sh` must not change.
- No fake success: do not change readiness/ok markers semantics to "paper over" issues.
- ABI stability: syscall numbers, error semantics, and stable layouts must remain unchanged.
- Logic-preserving: no semantic code changes in Phase A and Phase B.

## Proposed design

### Contract / interface (normative)

This RFC defines a kernel organization contract:

1. A responsibility taxonomy for kernel code:
   - `arch`, `hal`, `core`, `mm` (memory), `cap` (capabilities), `ipc` (comm), `sched`, `task` (process), `syscall`, `diag` (cross-cutting debug/determinism), `selftest`.
2. Standard kernel module headers:
   - CONTEXT, OWNERS, PUBLIC API, DEPENDS_ON, INVARIANTS, ADR
   - If required by repo standards: STATUS, API_STABILITY, TEST_COVERAGE
3. A physical reorganization policy:
   - If performed, it must be mechanical (moves + module wiring only), logic-preserving, and proof-gated.

The exact file move map and proof commands are defined in tasks.

### Phases / milestones (contract-level)

- Phase A (text-only): normalize headers, docs cross-links, and test documentation.
  - Execution: `tasks/TASK-0011-kernel-simplification-phase-a.md` (Phase A)
- Phase B (physical reorg): reorganize directories to match the taxonomy; moves + wiring only.
  - Execution: `tasks/TASK-0011-kernel-simplification-phase-a.md` (Phase B)

## Security considerations

- Threat model:
  - Documentation drift causes incorrect assumptions about invariants and enforcement points.
  - Large refactors can accidentally weaken critical enforcement if not proof-gated.
- Mitigations:
  - Keep all phases logic-preserving and proof-gated with existing QEMU marker contracts.
  - Document invariants at module boundaries so reviews focus on the right properties.
- Open risks:
  - Physical reorganization creates merge churn; mitigate with small, mechanical PRs.

## Failure model (normative)

- If a phase causes behavior changes, marker changes, or ABI drift, it is a failure and must be reverted or split into a separate task with an explicit contract change.
- No silent fallback: do not "fix" failing proofs by changing marker text or loosening gates.

## Proof / validation strategy (required)

Canonical proofs are owned by tasks, but must remain stable for this RFC:

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh
```

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test --workspace
```

## Alternatives considered

- Doc-only forever (no physical reorg):
  - Rejected because navigation costs remain higher than necessary and path drift accumulates.
- Large, one-shot refactor including ownership/type changes:
  - Rejected because it increases risk; phases keep diffs reviewable and proof-gated.

## Open questions

- Should `types.rs` remain at the crate root as a ubiquitous dependency, or move under `core/`?
- Do we want a small "prelude" module for common kernel types, or keep explicit imports?

---

## Implementation Checklist

- [ ] Phase A complete via `tasks/TASK-0011-kernel-simplification-phase-a.md` (Phase A) — proof: `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
- [ ] Phase B complete via `tasks/TASK-0011-kernel-simplification-phase-a.md` (Phase B) — proof: `cargo test --workspace` and QEMU marker contract
- [ ] Tasks are linked with stop conditions and proof commands.
