---
title: TASK-0054C Kernel IPC performance contract + `call` fastpath (short control messages, direct reply handoff, VMO-first bulk)
status: Draft (end-state rewrite 2026-09-09; kernel approval zone — architecture-review + explicit user approval before code)
owner: @kernel-team @runtime
created: 2026-03-29
depends-on: []
follow-up-tasks: []
links:
  - Vision: docs/architecture/vision.md
  - Playbook: CLAUDE.md
  - Kernel IPC contract: docs/rfcs/RFC-0005-kernel-ipc-capability-model.md
  - IPC runtime architecture: docs/adr/0003-ipc-runtime-architecture.md
  - VMO plumbing baseline: tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md
  - UI perf floor baseline: tasks/TASK-0054B-ui-v1a-kernel-ui-perf-floor-zero-copy-qos-hardening.md
  - Present/input consumer baseline: tasks/TASK-0056-ui-v2a-present-scheduler-double-buffer-input-routing.md
  - Testing contract: scripts/qemu-test.sh
---

## End-state rewrite 2026-09-09 (binding; supersedes older sections where they differ)

**Ground truth 2026-09-09:** zero fastpath code (`source/kernel/neuron/src/ipc/` = `mod.rs`
718, `trace.rs` 279, `endpoint.rs` 151, `header.rs` 92 LOC), no IPC microbench anywhere, no
design pass recorded. The world it must be designed for landed since the draft: phased
syscalls (`core/trap/phased.rs`), the lock-free syscall class and cpu0 right-of-way (ADR-0049,
BKL wait 90.8 → ~6 ms), routing v2 with parked replies (TASK-0324 P3). Every service today
does request/reply as `send_with_cap_move_wait` + `recv` pairs, and app-host carries a stale-
reply drain (`source/services/app-host/src/svc_call.rs`) because the pair can desynchronize.

### Goal (end system)

A written IPC performance contract with deterministic gates, and the fastpath sized to it:
ONE `call` syscall for short control messages (≤ 64 B inline, zero heap) with direct reply
handoff — inside the phased/lock-free/right-of-way world, with RFC-0005 semantics untouched.

### Non-goals

New IPC primitive or ABI redesign; lock-free router experiments; MM changes; any change to
`sender_service_id`, CAP_MOVE rights/rollback, endpoint close/EOF (RFC-0079) semantics.

### Invariants

- RFC-0005 contract semantics preserved; identity stamped by the kernel on every path.
- `IPC_SHORT_MAX = 64` bytes inline; `IPC_PAYLOAD_MAX = 8192` hard cap → `E2BIG` above (no
  "temporary" oversized inline exception, ever).
- Hot path bounded: no unbounded queue scans, no logging, no heap growth for ≤ 64 B.
- Budgets are measured under `smp1` + icount (deterministic virtual time), printed with their
  numbers, and asserted — never a bare `ok`.

### Decisions

- **D1 Contract first.** RFC-0096 "IPC performance contract + `call` fastpath" (amends RFC-0005
  by reference) + ADR-0063. Budgets `IPC_CALL_RT_BUDGET_NS` (64 B ping-pong round trip),
  `IPC_CALL_WAKE_HOPS = 0` (the reply switches to the caller without a runqueue hop when the
  caller is on the serving hart or cpu0 right-of-way applies), `IPC_CALL_ALLOCS = 0` for ≤ 64 B —
  calibrated in P1 on the current path, then asserted.
- **D2 One trap for request+reply.** `SYSCALL_IPC_CALL_V1` (next free number, verified at P0):
  send the request with the moved reply cap and block on the reply endpoint in ONE trap. The
  reply is an ordinary `ipc_send_v1`; when the receiver is found in `BlockReason::IpcCall`, the
  payload is copied straight into the caller's registered buffer and the caller is made
  runnable / switched to (direct handoff). Lock class = BKL (router + scheduler); the ≤ 64 B
  copy needs no phased split; declared in the `LockClass` table with the safety argument
  (ADR-0049's "add a match arm" seam).
- **D3 Kernel-internal payload shape.** `Message.payload: Vec<u8>` becomes
  `Payload::{Inline([u8; 64], u8), Heap(Vec<u8>)}` — ABI unchanged, allocation-free short path.
- **D4 One request/reply API in userspace, old pairing deleted.**
  `nexus_ipc::KernelClient::call()` on the new syscall is the ONLY request/reply API. The
  `send_with_cap_move_wait` + `recv` pairing helpers and app-host's stale-reply drain
  (`svc_call.rs:38-68`) are DELETED in the same package — the desync class becomes impossible by
  construction. Gate: `tests/sdk_surface` asserts the pair helpers are gone; nexus-ipc
  `test_reject_oversized_inline` (E2BIG) + `test_reject_call_without_reply_cap`.
- **D5 Bench gate, TASK-0318 pattern.** Host op-count / alloc-count tests in
  `source/kernel/neuron/src/ipc/` (router ops per call, zero allocs for short messages) run in
  the `just test-all` kernel stage; QEMU prints the numbers in the kernel gate line.

### Packages (each names its blast radius + lanes to re-prove)

- **P0** RFC-0096 + ADR-0063 + `architecture-review` pass + explicit user approval (kernel +
  `source/libs` zones). Blast: paper.
- **P1** Measurement only: `SELFTEST: ipc bench (rt=<n>us hops=<h>)` on the current path (no
  assert) to calibrate D1. Blast: `ipc_kernel` phase; lanes headless + smp1.
- **P2** `Payload::Inline` + hard cap + E2BIG. Blast: every lane (all IPC) — full `test-all`.
- **P3** `SYSCALL_IPC_CALL_V1` + direct handoff + `KSELFTEST: ipc call budget ok (rt=<n>us
  hops=0 alloc=0)`. Blast: all lanes incl. smp/bkl budgets; TASK-0324 P3 parked-reply rules
  must hold (a `call` before the callee's `@ready` parks like any request).
- **P4** `KernelClient::call()` + migration of every consumer + deletion of the pair helpers
  and the app-host drain. Blast: all services, app-host; lanes visible (DSL shell), ota-*,
  ingress/egress, reset.
- **P5** Docs (`docs/testing/README.md` bench lane, RFC-0005 fastpath note, budgets with
  provenance).

### Definition of Done

Host: op-count/alloc gates; RFC-0005 compatibility tests green; `test_reject_*` above.
QEMU (registered in `proof-manifest/markers/ipc_kernel.toml`, `scripts/qemu-test.sh`
headless + smp1 lists, `tools/nx/chains/markers.txt`):
`KSELFTEST: ipc call budget ok (rt=<n>us hops=0 alloc=0)`,
`SELFTEST: ipc fastpath ping ok (rt=<n>us)`, `SELFTEST: ipc fastpath reply ok`,
`SELFTEST: ipc bulk-vmo path ok` (inline > 8 KiB → E2BIG, VMO path succeeds).
Docs: `docs/testing/README.md`, RFC-0096, ADR-0063.

### Touched paths

`source/kernel/neuron/src/{ipc/mod.rs,syscall/mod.rs,syscall/api/ipc_msg.rs,syscall/api/
ipc_call.rs (new),core/trap/{phased.rs,budgets.rs},task/mod.rs}`,
`source/libs/nexus-abi/src/syscall/ipc.rs`, `userspace/nexus-ipc/src/{os_lite.rs,connection.rs}`,
`source/services/app-host/src/svc_call.rs`, every request/reply consumer,
`source/apps/selftest-client/src/os_lite/{phases,probes}/ipc_kernel*`, `docs/rfcs/RFC-0096-*.md`,
`docs/adr/0063-*.md`.

### Dependencies (active work only)

TASK-0324 P3 (routing v2: parked replies until `@ready`) and P5 (stage fence / wake
determinism findings) land first — the `call` syscall must honour both.

## Rebase (2026-08-14) — historical, superseded by the end-state rewrite above

### Verified baseline

- **Zero fastpath code exists.** `source/kernel/neuron/src/ipc/` is 4 files
  (`mod.rs` 718, `trace.rs` 279, `endpoint.rs` 151, `header.rs` 92 LOC) with no
  short-message fastpath, and there is no IPC microbench harness anywhere in
  the repo. The goal below **stands**: short control-message fastpath +
  reply/wake tightening + VMO-first bulk rule + deterministic microbenches.

### Stale baseline — design against the phased/lockfree world

The ledger's queueing/wake framing predates TASK-0288's addendum, which landed:

- **phased syscalls** (`source/kernel/neuron/src/core/trap/phased.rs`),
- a **lock-free syscall class** and **cpu0 right-of-way**,
- BKL wait 90.8 ms → ~6 ms — ADR-0049 Accepted and implemented
  (`docs/adr/0049-bkl-lockclass-and-softrt-cpu-placement.md`).

Do NOT re-derive wake/queue costs from the pre-0288 BKL picture. Before any
code, the fastpath design must answer explicitly: which trap phase the
fastpath executes in, whether its syscalls can qualify for the lock-free
class, and how reply/wake interacts with cpu0 right-of-way.

### Process gate

`source/kernel/**` and `source/libs/**` are approval zones: run the
`architecture-review` skill on the design and obtain explicit user approval
before editing kernel files. Status stays Draft until that design pass exists.

## Context — historical, superseded by the end-state rewrite above

The current IPC architecture is directionally right for the system:

- endpoint capabilities,
- small typed control messages,
- large payloads out-of-band via VMO/filebuffer,
- channel-bound identity via `sender_service_id`.

That architecture should remain. However, once `windowd`, launcher, settings, overlays, and animation-heavy UI
start emitting lots of small control messages, the system needs a **fast path** for those short request/reply flows.

This task focuses on the **existing IPC ABI**, not a redesign. The goal is to make the hot path smaller and more
predictable while pushing bulk bytes harder onto the VMO/data plane.

## Goal — historical, superseded by the end-state rewrite above

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
- `docs/testing/README.md`

## Plan (small PRs)

1. Identify and tighten the small-message request/reply hot path.
2. Preserve RFC-0005 behavior while reducing avoidable wake/queue overhead.
3. Make VMO/filebuffer-first bulk discipline explicit in UI/media-facing paths.
4. Add microbench + QEMU evidence without inventing new fake-success markers.
