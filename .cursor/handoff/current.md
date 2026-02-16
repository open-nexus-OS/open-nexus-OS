# Current Handoff: TASK-0013B IPC liveness hardening — in review

**Date**: 2026-02-16  
**Status**: `TASK-0013B` is `In Review`. Migration slices are implemented and proof sync is complete with one documented runtime-timeout caveat on SMP2 at 90s.
**Contract seed**: `docs/rfcs/RFC-0025-ipc-liveness-hardening-bounded-retry-contract-v1.md`

---

## What is stable now

- `TASK-0014` remains closed (`Done`) and its proof package stays green.
- New hardening contract seed (`RFC-0025`) exists and is linked to new follow-up task `TASK-0013B`.
- Drift-free status/index scaffolding is updated for new RFC/task workstream.

## Runtime progress in this slice

- `RFC-0025` and `TASK-0013B` scaffolding are complete and drift-free indexes are synced.
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
  - 🟨 `SMP=2 ... RUN_TIMEOUT=90s` times out close to `SELFTEST: end` on this host load profile
  - ✅ `SMP=2 ... RUN_TIMEOUT=180s` green with full marker ladder.

## New architectural decision (this slice)

- Cross-service IPC retry behavior will be centralized on shared bounded helpers in `nexus-ipc` instead of service-local ad-hoc loops.
- Liveness hardening remains authority-preserving: no alternate SMP/scheduler authority is introduced.

## Active focus (next)

- Review and approve timeout-floor policy for SMP2 (keep 90s target vs ratify 180s on current host profile).
- Collect review feedback and decide closeout criteria.

## Closure note

- `TASK-0014` remains explicitly closed as `Done`; current work is follow-up hardening (`TASK-0013B` / `RFC-0025`).

## Guardrails

- No fake success markers (`ready`/`ok` only after proven behavior).
- No unbounded cardinality, payload, span-table, or rate growth.
- Identity/policy from kernel-authenticated `sender_service_id` only.
- Proof floor remains modern virtio-mmio with bounded deterministic timeouts.
