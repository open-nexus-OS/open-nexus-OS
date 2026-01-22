<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# RFC-0013: Boot gates v1 — readiness contract + spawn failure reasons + resource/leak sentinel

- Status: Complete
- Owners: @runtime, @kernel-team, @tools-team
- Created: 2026-01-16
- Last Updated: 2026-01-16
- Links:
  - Tasks: `tasks/TASK-0269-boot-gates-v1-readiness-spawn-resource.md` (execution + proof)
  - Related RFCs:
    - `docs/rfcs/RFC-0003-unified-logging.md` (deterministic markers, no fake success)
    - `docs/rfcs/RFC-0005-kernel-ipc-capability-model.md` (quotas, leak-free failure paths, IPC selftests)
    - `docs/rfcs/RFC-0009-no-std-dependency-hygiene-v1.md` (OS build hygiene gates)
    - `docs/rfcs/RFC-0011-logd-journal-crash-v1.md` (logd readiness marker contract)
  - Testing methodology: `docs/testing/index.md`
  - QEMU marker contract: `scripts/qemu-test.sh`

## Status at a Glance

- **Phase 0 (Readiness gate contract + harness coupling)**: ✅
- **Phase 1 (Spawn failure reason codes + selftests)**: ✅
- **Phase 2 (Resource/leak sentinel gate)**: ✅

Definition:

- “Complete” means the **contract** is defined and the **proof gates** are green (tests/markers). It does not mean “never changes again”.

## Scope boundaries (anti-drift)

This RFC is a **design seed / contract**. Implementation planning and proofs live in tasks.

- **This RFC owns**:
  - The **readiness contract** for OS/QEMU bring-up: what `init: up <svc>` means vs what `<svc>: ready` means, and which signals are authoritative for gates.
  - The v1 **spawn failure reason taxonomy** (stable classification) and requirements for surfacing them deterministically.
  - The v1 **resource/leak sentinel** contract: a deterministic stress mix + acceptance invariants that catch leaks/quota regressions early.
- **This RFC does NOT own**:
  - The `logd` wire contract or crash pipeline (owned by RFC-0011).
  - General OOM handling, global memory accounting, or a production “OOM killer” (see `tasks/TASK-0228-oomd-v1-deterministic-watchdog-cooperative-memstat-samgr-kill.md`).
  - Scheduler policy changes (fairness/QoS), unless strictly required to satisfy deterministic syscall semantics (owned by RFC-0005 / kernel tasks).
  - Persistent boot/power-cycle state and recovery (persistence roadmap tasks).

### Relationship to tasks (single execution truth)

- Tasks (`tasks/TASK-*.md`) define **stop conditions** and **proof commands**.
- This RFC defines contracts; `TASK-0269` implements proof gates and updates the canonical harness/doc references.

## Context

We already gate OS/QEMU bring-up with a deterministic marker contract (`scripts/qemu-test.sh`). However, a recurring failure class is:

- `init` reports `init: up <svc>` (control-plane handshake), but the service never emits `<svc>: ready` (data-plane readiness), and/or
- boot proceeds, then later `spawn`/resource operations fail (`abi:spawn-failed`) with insufficient reason detail to debug quickly.

As we add more services and more capability wiring, this class of issue becomes more frequent unless we **codify** the semantics and provide **early, deterministic** alarm mechanisms.

Note: allocator exhaustion can still occur during bring-up until the cooperative OOM watchdog
(`tasks/TASK-0228-oomd-v1-deterministic-watchdog-cooperative-memstat-samgr-kill.md`) lands.
This RFC provides the diagnostics and gates; TASK-0228 provides the mitigation.

## Goals

- Provide a **clear, repo-wide readiness contract** that distinguishes:
  - “spawned + ctrl channel ready” (`init: up <svc>`) vs
  - “service is actually ready to serve its contract” (`<svc>: ready` or an equivalent ready proof).
- Make `spawn` failures **actionable** by requiring a stable reason taxonomy and deterministic surfacing (UART markers + logd audit events where available).
- Add a deterministic **resource/leak sentinel** gate that detects:
  - capability slot leaks,
  - endpoint leaks / quota regressions,
  - resource exhaustion regressions (spawn path) under a bounded, deterministic stress mix.

## Non-Goals

- Introducing new kernel syscalls purely for diagnostics (unless required to satisfy the contract with bounded overhead).
- Replacing the existing QEMU harness; we extend/align it.
- Turning QEMU into the default developer loop (host-first stays primary; QEMU is authoritative smoke).

## Constraints / invariants (hard requirements)

- **Determinism**: gates must not depend on wall-clock jitter; they must be reproducible.
- **No fake success**: readiness markers must only be emitted after real readiness, never as placeholders.
- **Bounded resources**: sentinel tests must have explicit bounds (iterations, queues, allocation sizes).
- **Security floor**:
  - Failure reasons must not leak secrets (keys, capabilities).
  - Identity in audit records must use kernel-provided metadata (`sender_service_id`), never payload strings (align with RFC-0005).
- **Stubs policy**: stubs must be explicit, non-authoritative, and must not claim readiness/ok.

## Proposed design

### Contract / interface (normative)

#### A) Readiness gate contract (normative)

##### Terminology

- **Spawned**: a process/task exists (PID allocated) and its address space/stack is mapped.
- **Up / control-plane ready**: `init-lite` has completed the per-service bootstrap control channel handshake. This is announced as:
  - `init: up <svc>`
- **Ready / data-plane ready**: the service has finished its own initialization and has made its primary IPC endpoint(s) live. This is announced as:
  - `<svc>: ready`

##### Rules

- **Rule A1**: `init: up <svc>` MUST NOT be treated as “service ready” by tests or docs. It is only the control-plane handshake.
- **Rule A2**: A service MUST emit `<svc>: ready` only after it can correctly serve its v1 contract (wire contract or stub contract).
- **Rule A3**: For every service listed in the canonical OS/QEMU bring-up set (see `scripts/run-qemu-rv64.sh DEFAULT_SERVICE_LIST`), tests MUST define whether:
  - it is required to emit `<svc>: ready`, and
  - which downstream selftest markers depend on it.
- **Rule A4**: If a service emits `init: up <svc>` but never reaches `<svc>: ready` within the bounded QEMU run, the harness MUST fail with an explicit, stable reason (not just a generic timeout).

##### Notes

- This RFC intentionally does not mandate a specific transport for “ready” beyond the marker contract; in v1 we use UART markers as the authoritative gate signal for QEMU (aligning with current harness behavior and RFC-0003).
- **Low-effort gate (v1 compatibility)**: for services without a ready RPC yet, the harness may gate readiness by querying `logd` for the deterministic ready marker (e.g. `dsoftbusd: ready`). This is a stopgap until the service exposes a readiness RPC.
- **Follow-up placement**: readiness RPCs for dsoftbusd are tracked in later DSoftBus tasks (e.g. `tasks/TASK-0158-dsoftbus-v1b-os-consent-policy-registry-share-demo-cli-selftests.md`, `tasks/TASK-0212-dsoftbus-v1_1d-os-busdir-ui-selftests.md`), since TASK-0004 is already complete.

#### B) Spawn failure reason taxonomy (normative)

Current failures can collapse into generic `abi:spawn-failed`, which slows debugging and hides regressions.

##### Rule B1

The kernel spawn path MUST classify failures into a stable taxonomy that is:

- deterministic,
- bounded (small enum),
- and mappable to an on-wire error for userspace (`nexus-abi`) without ambiguity.

##### SpawnFailReason v1 (required set)

The v1 taxonomy MUST include (at minimum):

- `OutOfMemory` — page table / mapping / stack allocation failed due to memory pressure.
- `CapTableFull` — capability slot allocation failed.
- `EndpointQuota` — endpoint creation/allocation denied due to quota.
- `MapFailed` — address space mapping failed (invalid range / collision / permissions).
- `InvalidPayload` — invalid or unsupported ELF/payload metadata.
- `DeniedByPolicy` — spawn denied by policy (if policy gating applies at the spawn boundary; see RFC-0005).

##### Rule B2

The spawn reason MUST be surfaced deterministically as:

- a stable UART marker (kernel selftest marker or init marker) that includes the reason token, and
- a structured `logd` event when logd is available (without secrets).
- a userspace-accessible reason code via a bounded syscall (`spawn_last_error`) for debugging and tests.

#### C) Resource/leak sentinel gate (normative)

We need a deterministic, bounded test that catches “resource leaks” and “quota regressions” early.

**Rule C1**: The sentinel MUST be deterministic and bounded:

- bounded number of iterations,
- bounded allocation sizes,
- bounded endpoint depth and cap-table slot usage.

**Rule C2**: The sentinel MUST cover at least:

- **cap lifecycle churn**: clone/transfer/close sequences (align with RFC-0005 “no slot leaks across failure paths”),
- **endpoint pressure**: allocate up to quota, observe deterministic error (`EndpointQuota`), release, repeat,
- **spawn pressure**: repeated spawn/exit cycles until failure, verifying the failure reason is stable and no leaks remain after cleanup.

**Rule C3**: The sentinel MUST expose a single authoritative marker when it passes, e.g.:

- `KSELFTEST: resource sentinel ok`

and MUST emit a stable failure marker on first failure, including the reason token.

### Phases / milestones (contract-level)

- **Phase 0 (Readiness gate contract + harness coupling)**:
  - Codify A1–A4 in docs/tests.
  - Update the canonical test description (`docs/testing/index.md`) and ensure `scripts/qemu-test.sh` failure output is unambiguous for missing ready markers.
- **Phase 1 (Spawn failure reason codes + selftests)**:
  - Implement SpawnFailReason v1 in the kernel spawn path and surface it via `nexus-abi` + markers.
  - Add kernel selftests (or a deterministic trigger fixture) that proves each reason is reachable and correctly reported.
- **Phase 2 (Resource/leak sentinel gate)**:
  - Add the deterministic sentinel mix and gate it in QEMU runs (opt-in initially, then default once stable).

## Security considerations

- **Threat model**:
  - Diagnostic leaks: failure reasons expose sensitive info (keys/caps) via logs/markers.
  - Confused-deputy: tests accept “up” as “ready” and hide a security-relevant partial initialization.
  - DoS regressions: resource leaks allow a service to exhaust system resources over time.
- **Mitigations**:
  - Failure reasons are coarse-grained tokens; never include secrets, raw capability values, or key material.
  - Identity for audit logs is bound to `sender_service_id` (RFC-0005).
  - Sentinel uses bounded loops and explicit caps.
- **Open risks**:
  - Adding reason taxonomy requires careful ABI review to avoid leaking unstable internals while still being actionable.

## Failure model (normative)

- Missing readiness MUST fail deterministically and be attributable to a specific service and readiness stage (up vs ready).
- Spawn failures MUST provide a stable reason token from the SpawnFailReason v1 set (no silent collapse to “unknown” in gates).
- Sentinels MUST not “retry until it works”; first failure is authoritative and must be reported.

## Proof / validation strategy (required)

Canonical proofs are owned by `TASK-0269` and must be kept current there.

### Proof (Host)

- Readiness contract is documented and consistent with the test harness description:

```bash
cd /home/jenning/open-nexus-OS && sed -n '1,220p' docs/testing/index.md
```

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os
```

### Deterministic markers (if applicable)

- `init: up <svc>` (control-plane handshake)
- `<svc>: ready` (service readiness)
- `KSELFTEST: resource sentinel ok` (once Phase 2 lands)
- `KSELFTEST: spawn reason <token> ok` (Phase 1 selftests; exact marker names are task-defined but must be stable)

## Alternatives considered

- **A1: Treat `init: up` as readiness**: rejected (hides partial init and creates flaky bring-up).
- **A2: Only rely on logd for readiness**: rejected for v1 (logd itself is a bring-up dependency; UART markers remain the authoritative QEMU contract).
- **A3: Add ad-hoc debug prints everywhere**: rejected (drift + alloc risk; violates RFC-0003 discipline).

## Open questions

- **Q1 (owner: @kernel-team)**: RESOLVED — SpawnFailReason v1 is a new ABI enum surfaced via `spawn_last_error`.
- **Q2 (owner: @tools-team)**: RESOLVED — resource/leak sentinel runs in the default QEMU smoke (marker contract).

## RFC Quality Guidelines (for authors)

When writing this RFC, ensure:

- Scope boundaries are explicit; cross-RFC ownership is linked.
- Determinism + bounded resources are specified in Constraints section.
- Security invariants are stated (threat model, mitigations, DON'T DO).
- Proof strategy is concrete (not "we will test this later").
- If claiming stability: define ABI/on-wire format + versioning strategy.
- Stubs (if any) are explicitly labeled and non-authoritative.

---

## Implementation Checklist

**This section tracks implementation progress. Update as phases complete.**

- [x] **Phase 0**: Readiness gate contract documented — `docs/testing/index.md` updated.
- [x] **Phase 1**: Spawn failure reason codes — `KSELFTEST: spawn reasons ok` marker in QEMU.
- [x] **Phase 2**: Resource/leak sentinel — `KSELFTEST: resource sentinel ok` marker in QEMU.
- [x] Task linked: `tasks/TASK-0269-boot-gates-v1-readiness-spawn-resource.md`
- [x] QEMU markers pass: `init: ready`, `<svc>: ready`, `KSELFTEST: spawn reasons ok`, `KSELFTEST: resource sentinel ok`
