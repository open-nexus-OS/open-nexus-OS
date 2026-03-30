---
title: TASK-0054C UI v1a extension: kernel IPC fastpath v1 (short control messages + VMO-first bulk discipline)
status: Draft
owner: @kernel-team @runtime
created: 2026-03-29
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Kernel IPC contract: docs/rfcs/RFC-0005-kernel-ipc-capability-model.md
  - IPC runtime architecture: docs/adr/0003-ipc-runtime-architecture.md
  - VMO plumbing baseline: tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md
  - UI perf floor baseline: tasks/TASK-0054B-ui-v1a-kernel-ui-perf-floor-zero-copy-qos-hardening.md
  - Present/input consumer baseline: tasks/TASK-0056-ui-v2a-present-scheduler-double-buffer-input-routing.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

The current IPC architecture is directionally right for the system:

- endpoint capabilities,
- small typed control messages,
- large payloads out-of-band via VMO/filebuffer,
- channel-bound identity via `sender_service_id`.

That architecture should remain. However, once `windowd`, launcher, settings, overlays, and animation-heavy UI
start emitting lots of small control messages, the system needs a **fast path** for those short request/reply flows.

This task focuses on the **existing IPC ABI**, not a redesign. The goal is to make the hot path smaller and more
predictable while pushing bulk bytes harder onto the VMO/data plane.

## Goal

Deliver a kernel IPC fastpath suitable for UI/control-plane traffic:

1. **Short-message fastpath**:
   - optimize the common case of small control messages,
   - keep queueing/wake bookkeeping bounded and cheap.
2. **Reply/wake hot-path tightening**:
   - reduce avoidable wake/unblock overhead in request/reply flows,
   - keep timeout/deadline semantics intact.
3. **VMO-first bulk discipline**:
   - document and enforce the rule that large payloads should move via VMO/filebuffer,
   - avoid “temporary” oversized inline message paths for surfaces/media/documents.
4. **Evidence and benchmarks**:
   - provide deterministic host microbenches and bounded QEMU selftests for ping/reply latency.

## Non-Goals

- New distributed IPC design.
- Replacing endpoint capabilities with a different primitive.
- General-purpose lock-free router experiments.
- MM mapping optimization beyond what is necessary for IPC data-plane handoff (`TASK-0054D` owns MM perf).

## Constraints / invariants (hard requirements)

- Preserve RFC-0005 contract semantics and identity binding rules.
- Do not break `sender_service_id`, endpoint close-on-exit, waiter wake, or CAP_MOVE correctness.
- Large bulk payloads remain VMO/filebuffer-first.
- Hot-path work must be bounded:
  - no unbounded queue scans,
  - no unbounded logging,
  - no opportunistic heap growth in the fast path.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Security considerations

IPC is a trust boundary and must remain fail-closed while being optimized.

### Threat model

- **Identity drift**: optimization accidentally bypasses `sender_service_id` authority.
- **Capability mishandling**: fastpath changes breaking CAP_MOVE or close semantics.
- **Queue exhaustion / DoS**: optimization reintroducing unbounded buffering or unfair wake behavior.
- **Oversized inline payload drift**: services bypassing VMO/filebuffer discipline and bloating kernel copies.

### Security invariants (MUST hold)

- `sender_service_id` remains authoritative for security-sensitive consumers.
- CAP_MOVE remains rights-bounded and rollback-safe.
- Endpoint close / owner-exit wake semantics remain deterministic.
- Large payloads do not silently expand the control-plane attack surface.

### DON'T DO

- DON'T add a “fast path” that skips identity binding or endpoint rights checks.
- DON'T create a parallel IPC ABI just for UI traffic.
- DON'T add a giant inline-payload exception for convenience.
- DON'T optimize by weakening existing timeout or waiter semantics.

## Stop conditions (Definition of Done)

### Proof (Host) — required

- Microbench / contract tests prove improvements for:
  - ping-pong request/reply,
  - wake/unblock common case,
  - large-payload path selecting VMO/filebuffer instead of inline copy.
- Compatibility tests remain green for existing RFC-0005 ABI/layout guarantees.

### Proof (OS/QEMU) — gated

Deterministic markers / evidence:

- `SELFTEST: ipc fastpath ping ok`
- `SELFTEST: ipc fastpath reply ok`
- `SELFTEST: ipc bulk-vmo path ok`

Optional additive perf evidence:

- bounded latency counters exported by the selftest harness or `perfd`

## Touched paths (allowlist)

- `source/kernel/neuron/src/ipc/`
- `source/kernel/neuron/src/syscall/`
- `source/kernel/neuron/src/task/`
- `source/libs/nexus-abi/`
- `userspace/nexus-ipc/`
- `source/apps/selftest-client/`
- `docs/rfcs/RFC-0005-kernel-ipc-capability-model.md` (only if contract wording needs explicit fastpath notes)
- `docs/testing/index.md`

## Plan (small PRs)

1. Identify and tighten the small-message request/reply hot path.
2. Preserve RFC-0005 behavior while reducing avoidable wake/queue overhead.
3. Make VMO/filebuffer-first bulk discipline explicit in UI/media-facing paths.
4. Add microbench + QEMU evidence without inventing new fake-success markers.
