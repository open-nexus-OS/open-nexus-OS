---
title: TASK-0013B IPC liveness hardening v1: bounded retry/correlation contract across services
status: In Review
owner: @runtime
created: 2026-02-16
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Seed contract: docs/rfcs/RFC-0025-ipc-liveness-hardening-bounded-retry-contract-v1.md
  - Performance extension contract: docs/rfcs/RFC-0026-ipc-performance-optimization-contract-v1.md
  - Previous baseline: tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md
  - Correlation baseline: docs/rfcs/RFC-0019-ipc-request-reply-correlation-v1.md
  - SMP hardening baseline: tasks/TASK-0012B-kernel-smp-v1b-scheduler-smp-hardening.md
  - Follow-up runtime hardening: tasks/TASK-0247-bringup-rv-virt-v1_1b-os-smp-hsm-ipi-virtioblkd-packagefs-selftests.md
  - Testing contract: scripts/qemu-test.sh
  - Review package: .cursor/handoff/task-0013b-rfc0026-review-package.md
---

## Context

`TASK-0013` closed QoS/timed v1 behavior, but cross-service IPC retry loops are still partially service-local and inconsistent in boundedness semantics.

This follow-up hardens liveness and overload behavior by converging routing/reply retry loops on one deterministic contract.

The task is now extended (re-opened) to reduce SMP=2 end-to-end timeout pressure with minimal-invasive, drift-free performance work on top of the shipped liveness baseline.

## Goal

Prove a deterministic bounded IPC retry/correlation contract across selected services, with:

- bounded deadlines and attempt budgets,
- bounded nonce mismatch handling,
- explicit timeout/reject behavior,
- no fake success markers.

Extension goal (RFC-0026):

- improve control-plane efficiency (routing/client setup/reply channel reuse) and keep data-plane zero-copy aligned with existing architecture, without ABI drift or authority model changes.

## Non-Goals

- Redesigning scheduler authority or SMP architecture.
- Changing service payload contracts unrelated to retry/liveness.
- Cross-node retry policy.

## Constraints / invariants (hard requirements)

- Deterministic markers; bounded retries; no unbounded drain/yield loops.
- No fake success markers (`ready`/`ok` only after real behavior).
- Preserve ownership/newtype/Send-Sync/must_use boundaries:
  - explicit retry/deadline boundary types where practical,
  - `#[must_use]` retry outcomes handled explicitly.
- No new `unsafe impl Send/Sync` without written safety argument + tests.
- Keep `TASK-0012B` scheduler/SMP authority model intact.

## Security considerations

### Threat model

- Queue contention causes retry spin and starvation.
- Nonce mismatch traffic causes correlated reply desync.
- Inconsistent timeout behavior hides liveness regressions.

### Security invariants

- Retry loops are bounded by deadline and/or explicit attempt budget.
- Correlation mismatch handling is bounded.
- Policy/correlation decode failures remain fail-closed where applicable.

### DON'T DO

- Don't add infinite retry loops.
- Don't silently fall back from timeout into hidden success.
- Don't trust payload identity for security decisions.

## Stop conditions (Definition of Done)

- Shared bounded retry contract exists in `userspace/nexus-ipc` and is host-tested.
- Services in touched list migrate high-risk retry loops to shared helpers.
- Kernel-aligned overload/liveness checks remain deterministic and bounded in proofs.
- QEMU/host proof commands pass.
- RFC-0026 extension slices are implemented with deterministic proof evidence and no regression of RFC-0025 guarantees.

### Proof (Host)

- `cargo test -p nexus-ipc -- --nocapture`
- `cargo test -p timed -- --nocapture`
- `cargo test --workspace`

### Proof (OS / QEMU)

- `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`
- `SMP=2 REQUIRE_SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
- `SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`

Latest evidence (2026-02-16):

- ✅ `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`
- ✅ `SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
- ✅ `SMP=2 REQUIRE_SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
- ✅ `SMP=2 REQUIRE_SMP=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=180s ./scripts/qemu-test.sh` green with full marker ladder (no functional regression observed).
- Note: one parallel-run attempt produced invalid harness evidence (`uart.log` contention / missing init marker). Sequential proof discipline is required and now enforced for all recorded results.

## Touched paths (allowlist)

- `userspace/nexus-ipc/`
- `source/services/timed/`
- `source/services/metricsd/`
- `source/services/rngd/`
- `source/services/execd/`
- `source/services/keystored/`
- `source/services/statefsd/`
- `source/services/policyd/`
- `source/services/updated/`
- `source/kernel/neuron/src/sched/`
- `source/kernel/neuron/src/syscall/`
- `source/kernel/neuron/src/selftest/`
- `scripts/qemu-test.sh`
- `docs/rfcs/`
- `tasks/`

## Plan (small PR slices)

1. Shared bounded retry contract helpers (`nexus-ipc`) + tests.
2. Migrate high-risk service routing/reply loops (`timed`, `metricsd`, `rngd`).
3. Migrate remaining service hotspots (`execd`, `keystored`, `statefsd`, `policyd`, `updated`).
4. Kernel-aligned overload/liveness test hardening (no authority drift).
5. Proof + marker validation + doc/status sync.
6. RFC-0026 phase-0 baseline and hotspot evidence for control-plane overhead.
7. RFC-0026 phase-1 minimal-invasive control-plane optimizations (client/reply reuse, bounded route lookup policy).
8. RFC-0026 phase-2 data-plane alignment: enforce bulk-path split (zero-copy/VMO where applicable) without changing IPC authority model.
9. Re-run full proof matrix and close review package.

## Execution status (2026-02-16)

- [x] Phase 0 shared helper contract
- [x] Phase 1 high-risk service migration
- [x] Phase 2 remaining hotspot migration
- [x] Phase 3 kernel-aligned hardening
- [x] Phase 4 proof + closure sync (with explicit SMP=2@90s runtime-timeout note captured above)
- [x] Phase 5 RFC-0026 baseline + hotspot evidence
- [x] Phase 6 RFC-0026 control-plane optimization slice
- [x] Phase 7 RFC-0026 data-plane alignment slice
- [x] Phase 8 proof + review sync for reopened scope
