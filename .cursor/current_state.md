# Cursor Current State (SSOT)

## Current architecture state

- **last_decision (2026-04-21)**: Next execution focus is `TASK-0031` with contract seed `RFC-0040`.
  - `TASK-0029` and `RFC-0039` remain closed (`Done`) and are no longer active execution scope.
  - New seed RFC `RFC-0040` exists and is linked from `TASK-0031`.
  - RFC-0040 now carries a **normative production-grade requirement** tied to `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md` with closure routed through `TASK-0290`.
  - `TASK-0031` remains the plumbing/honesty floor (host-first + OS-gated), while kernel-enforced seal/rights closure remains in `TASK-0290`.

- **prev_decision (2026-04-22)**: `TASK-0029` closure remediation completed and status synchronized (`TASK-0029` + `RFC-0039` marked done).

## Active focus (execution)

- **active_task**: `tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md` — `In Progress`
- **contract_seed**: `docs/rfcs/RFC-0040-zero-copy-vmos-v1-plumbing-host-first-os-gated.md` — `In Progress`
- **production_closure_task**: `tasks/TASK-0290-kernel-zero-copy-closure-v1b-vmo-seals-reuse-truth.md` — `Draft`
- **tier_target**: production-grade trajectory per `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md` (Gate A + Gate C relevant zero-copy obligations)

## Active constraints (TASK-0031 / RFC-0040)

- Kernel remains untouched in `TASK-0031` scope.
- No fake-success markers: `ok/ready` only after real cross-process VMO behavior.
- Bounded resources: explicit caps for VMO length, live handle counts, and retries.
- Rust discipline is explicit where safety-relevant: `newtype`, ownership/lifetime, `#[must_use]`, justified `Send`/`Sync`.
- Behavior-first proof rule applies: tests must prove Soll-Verhalten, not code-shape trivia.
- Production-grade claims are forbidden until `TASK-0290` closure proofs are green.

## Contract links (active)

- `tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md`
- `docs/rfcs/RFC-0040-zero-copy-vmos-v1-plumbing-host-first-os-gated.md`
- `tasks/TASK-0290-kernel-zero-copy-closure-v1b-vmo-seals-reuse-truth.md`
- `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`
- `docs/rfcs/RFC-0005-kernel-ipc-capability-model.md`
- `docs/standards/RUST_STANDARDS.md`
- `docs/standards/SECURITY_STANDARDS.md`

## Carry-over note

- `TASK-0023B` external CI replay artifact closure remains an independent environmental follow-up and does not block `TASK-0031` kickoff.
