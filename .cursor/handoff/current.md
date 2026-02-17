# Current Handoff: TASK-0013B IPC liveness hardening — extension in review

**Date**: 2026-02-16  
**Status**: `TASK-0013B` is now `In Review` with the RFC-0026 extension slices implemented and proofed.
**Contract seed**: `docs/rfcs/RFC-0026-ipc-performance-optimization-contract-v1.md` (extends RFC-0025 baseline)

---

## What is stable now

- `TASK-0014` remains closed (`Done`) and its proof package stays green.
- New hardening contract seed (`RFC-0025`) exists and is linked to new follow-up task `TASK-0013B`.
- Drift-free status/index scaffolding is updated for new RFC/task workstream.

## Runtime progress in this slice

- `RFC-0025` and `TASK-0013B` scaffolding are complete and drift-free indexes are synced.
- `RFC-0026` created as a new contract seed (README/template compliant) for minimal-invasive performance optimization on top of the RFC-0025 baseline.
- Review package artifact created: `.cursor/handoff/task-0013b-rfc0026-review-package.md`.
- Shared retry contract landed in `userspace/nexus-ipc`:
  - `NonceMismatchBudget` newtype
  - `RouteRetryOutcome` `#[must_use]` bounded outcome
  - `route_with_nonce_budgeted(...)` deterministic helper.
- Service migrations landed:
  - wave-1: `timed`, `metricsd`, `rngd`
  - wave-2: `execd`, `keystored`, `statefsd`, `policyd`, `updated`
- Kernel-aligned hardening landed:
  - scheduler regression test for `set_task_qos` queue-full rollback invariant.
- Proof snapshot:
  - ✅ `cargo test -p nexus-ipc -- --nocapture`
  - ✅ `cargo test -p timed -- --nocapture`
  - ✅ `cargo test --workspace`
  - ✅ `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`
  - ✅ `SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
  - ✅ `SMP=2 ... RUN_TIMEOUT=90s` green (sequential run discipline)
  - ✅ `SMP=2 ... RUN_TIMEOUT=180s` green with full marker ladder.
  - discipline note: parallel QEMU smoke execution is invalid evidence (shared image/log contention); proofs are recorded from sequential runs only.

## New architectural decision (this slice)

- Cross-service IPC retry behavior will be centralized on shared bounded helpers in `nexus-ipc` instead of service-local ad-hoc loops.
- Liveness hardening remains authority-preserving: no alternate SMP/scheduler authority is introduced.

## Active focus (next)

- Hold `TASK-0013B` in `In Review` until explicit close decision.
- Preserve sequential QEMU proof discipline for any re-runs and incremental follow-up optimization work.

## Closure note

- `TASK-0014` remains explicitly closed as `Done`; current work is follow-up hardening (`TASK-0013B` / `RFC-0025`).

## Guardrails

- No fake success markers (`ready`/`ok` only after proven behavior).
- No unbounded cardinality, payload, span-table, or rate growth.
- Identity/policy from kernel-authenticated `sender_service_id` only.
- Proof floor remains modern virtio-mmio with bounded deterministic timeouts.
