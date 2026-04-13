---
title: TASK-0054B UI v1a extension: kernel/UI perf floor (zero-copy bulk path + coarse QoS/affinity + SMP hardening carry-ins)
status: Draft
owner: @kernel-team @runtime @ui
created: 2026-03-29
depends-on: []
follow-up-tasks:
  - TASK-0288
  - TASK-0290
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - UI v1a host renderer baseline: tasks/TASK-0054-ui-v1a-cpu-renderer-host-snapshots.md
  - VMO plumbing baseline: tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md
  - UI performance philosophy: docs/dev/ui/foundations/quality/performance-philosophy.md
  - SMP v2 controls baseline: tasks/TASK-0042-smp-v2-affinity-qos-budgets-kernel-abi.md
  - SMP/parallelism policy: tasks/TASK-0277-kernel-smp-parallelism-policy-v1-deterministic.md
  - Per-CPU ownership wrapper: tasks/TASK-0283-kernel-percpu-ownership-wrapper-v1.md
  - Zero-copy app platform: tasks/TRACK-ZEROCOPY-APP-PLATFORM.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

`TASK-0054` deliberately keeps the first renderer slice host-first and kernel-free. That is still the right
baseline, but if we want later blur, glass, transitions, and rich windowd scenes to feel as fluid as possible
in QEMU, we should establish a small **kernel/UI performance floor** before the visible compositor path grows.

This task is intentionally **not** a scheduler rewrite. It pulls forward only the minimum, drift-free pieces that
improve later UI latency and data movement:

- a stronger UI/media interpretation of `TASK-0031` (VMO/filebuffer bulk path is the default for large surfaces),
- a small trusted-service slice of `TASK-0042` (coarse affinity/shares/burst hints, not a new scheduler family),
- and hardening posture from `TASK-0277` / `TASK-0283` so SMP hot paths stay bounded and auditable.

## Goal

Deliver a kernel/UI perf floor that keeps the existing architecture but reduces avoidable latency/copy cost for
future `windowd`, input, audio, and media paths:

1. **Zero-copy UI/media bulk stance**:
   - surface/image/media bulk payloads are explicitly documented as VMO/filebuffer-first consumers,
   - large UI payload paths avoid “copy into IPC payload” designs by default.
2. **Coarse trusted-service scheduling controls**:
   - allow a minimal trusted-service profile for `windowd` / input / audio-facing services,
   - use coarse affinity/shares (and, if justified, a minimal burst hint) without changing the scheduler family.
3. **SMP hot-path hardening carry-in**:
   - make `TASK-0277` rules normative for the early UI floor,
   - adopt `PerCpu<T>` or equivalent ownership hardening where it reduces cross-CPU scheduler/IPI drift.
4. **Proof scenes and measurement hooks**:
   - establish deterministic microbench / selftest evidence that later UI tasks can rely on.
5. **Hot-path budget floor**:
   - define early metrics for UI-shaped actions:
     - service hops per user action,
     - cross-core hops per user action,
     - queue transitions / queue residence time by QoS,
     - wakeups per interaction,
     - and control-plane-vs-data-plane bytes for large UI/media payloads.

## Non-Goals

- Replacing the scheduler with a fair/deadline scheduler.
- Full MCS / scheduling-context design.
- GPU acceleration or display drivers.
- Reworking the local IPC contract (handled in `TASK-0054C`).
- A full MM redesign (handled in `TASK-0054D`).

## Constraints / invariants (hard requirements)

- Preserve the “small kernel, policy in userspace” architecture.
- No second scheduler authority path; extend the existing QoS + SMP hardening path only.
- Bulk data for UI/media stays on the VMO/filebuffer data plane, not in oversized control-plane payloads.
- Scheduler/SMP/IRQ hot paths remain bounded:
  - no heap growth,
  - explicit lock ordering,
  - no unbounded per-frame kernel work.
- The floor must reduce hidden hot-path complexity:
  - avoid accidental cross-core ping-pong,
  - avoid unnecessary wakeups,
  - and keep global synchronization points minimal and explainable.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Security considerations

This task touches kernel scheduling and bulk buffer handling, so it is security-relevant.

### Threat model

- **QoS/affinity abuse**: unauthorized tasks trying to obtain UI/perf-biased scheduling treatment.
- **Information leakage**: affinity or boost controls exposing system topology or trusted-service policy.
- **Bulk-path drift**: large payloads silently falling back to unsafe or expensive copy paths.
- **Cross-CPU ownership bugs**: accidental shared mutable scheduler state causing corruption or denial of service.

### Security invariants (MUST hold)

- Only trusted/bootstrap-authorized paths may assign elevated UI/perf scheduling hints to other tasks.
- User tasks must not gain ambient topology control or privileged CPU placement.
- `TASK-0031` rights/bounds rules continue to govern all VMO/filebuffer bulk paths.
- Per-CPU state remains single-owner unless an explicit, audited synchronization rule exists.

### DON'T DO

- DON'T introduce a new “desktop boost” scheduler class outside the existing QoS/affinity/shares model.
- DON'T route large surfaces through IPC payload copies just because it is easier in bring-up.
- DON'T weaken `TASK-0012B` bounded queue / resched evidence rules.
- DON'T make affinity/shares a general-purpose information leak about CPU layout.

## Production-grade gate note

This task establishes the early **UI/kernel performance floor**, but production-grade consumer claims
still depend on two later closeout steps:

- `TASK-0288` for deterministic runtime/SMP/timer/IPI stress closure,
- `TASK-0290` for kernel-enforced zero-copy rights/sealing and reuse truth.

Use this task to justify the early hot-path floor; use those follow-up tasks to justify a production-grade kernel/UI boundary.

## Stop conditions (Definition of Done)

### Proof (Host) — required

- Host/kernel tests prove:
  - trusted scheduling profiles clamp/validate as documented,
  - per-CPU ownership wrapper adoption does not regress existing scheduler invariants,
  - UI/media bulk-path helpers choose VMO/filebuffer for large buffers.
  - deterministic counters exist for wakeups, QoS queue residence, and bulk-path copy fallback on fixed fixtures.

### Proof (OS/QEMU) — gated

Deterministic markers / evidence:

- `execd: qos set (svc=windowd ... )` or equivalent trusted-service proof
- `execd: affinity set (svc=windowd ... )` or equivalent trusted-service proof
- `SELFTEST: vmo ui bulk path ok`
- `SELFTEST: ui kernel perf floor ok`

Notes:

- Markers should remain additive and honest; this task must not invent fake UI readiness.

## Touched paths (allowlist)

- `source/kernel/neuron/src/sched/`
- `source/kernel/neuron/src/core/`
- `source/kernel/neuron/src/task/`
- `source/kernel/neuron/src/syscall/`
- `source/libs/nexus-abi/`
- `source/services/execd/`
- `source/apps/selftest-client/`
- `docs/architecture/01-neuron-kernel.md`
- `docs/architecture/16-rust-concurrency-model.md`
- `docs/testing/index.md`
- `docs/storage/vmo.md`

## Plan (small PRs)

1. Pull forward the minimal trusted-service slice from `TASK-0042`.
2. Make `TASK-0277` rules explicit for early UI perf-sensitive work.
3. Adopt `PerCpu<T>` / ownership hardening in the smallest high-value scheduler/IPI sites.
4. Strengthen `TASK-0031` consumer wording and proofs for UI/media bulk buffers.
5. Add deterministic host/QEMU evidence that later `windowd` tasks can cite.

## Phase plan

### Phase A — Early hot-path observability

- define the minimal metric surface for UI-shaped actions,
- ensure kernel/SMP/QoS carry-ins can report bounded, deterministic counters without becoming a second tracing stack.

### Phase B — Trusted-service floor

- apply the hot-path budget floor to `windowd` / input / audio-adjacent trusted services,
- prove that reduced copy cost is not offset by hidden wakeup, queue, or cross-core regressions.
