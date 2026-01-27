# TASK-0010 Handoff: Device access model v1 (safe userspace MMIO foundation)

**Date**: 2026-01-27  
**Status**: In Progress (kernel mapping primitive proven; “foundation complete” distribution still pending)  
**Scope note**: This task is the **foundation gate** for userspace drivers on QEMU `virt` (virtio-* MMIO devices). It must remain enforce-only in kernel and keep policy/distribution in userspace.

---

## Executive Summary

You are implementing **TASK-0010: Device access model v1**:

- Provide the minimal kernel/userspace contract for **capability-gated MMIO**:
  - `CapabilityKind::DeviceMmio { base, len }`
  - `SYSCALL_MMIO_MAP` / `nexus_abi::mmio_map(...)`
- Enforce **USER|RW only, never executable**, and strict bounds.
- Make the **distribution model** drift-proof:
  - **init** distributes device caps to **designated owner services**
  - `policyd` is the **deny-by-default authority** for “who may receive which device cap”
  - distribution events are **audited via logd**
  - **no kernel name/string checks** as the v1 foundation (bring-up wiring must be removed before calling this “done”)

This task unlocks (and must stay aligned with):

- `TASK-0009` Persistence v1 (virtio-blk + statefs)
- Networking bring-up paths (virtio-net owner services)
- Driver/accelerator roadmap (`TRACK-DRIVERS-ACCELERATORS`, `TRACK-NETWORKING-DRIVERS`)

---

## Must-Read Files (in order)

### 1) Repo standards / rules

1. `docs/agents/PLAYBOOK.md`
2. `docs/agents/VISION.md`
3. `docs/standards/SECURITY_STANDARDS.md`
4. `docs/standards/BUILD_STANDARDS.md`
5. `docs/standards/RUST_STANDARDS.md`

### 2) Task + dependent contracts

- Primary: `tasks/TASK-0010-device-mmio-access-model.md`
- Consumers/gates:
  - `tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md` (explicitly gated on 0010; requires init+policy distribution)
  - `tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md` (done, but still a reference consumer)
  - `tasks/TRACK-DRIVERS-ACCELERATORS.md`
  - `tasks/TRACK-NETWORKING-DRIVERS.md`

### 3) Testing contract

- `docs/testing/index.md`
- `scripts/qemu-test.sh` (marker ladder)

---

## Decisions locked (anti-drift)

- **Kernel boundary**:
  - Kernel provides only the enforceable primitive (cap kind + mapping syscall) and W^X at the boundary.
  - Kernel does **not** become a policy authority and does **not** “enumerate devices for apps”.
- **Authority model**:
  - `policyd` is the single authority for allow/deny.
  - Identity binding uses kernel `sender_service_id`.
- **Least privilege**:
  - Prefer per-device bounded windows (rng vs net vs blk) rather than a single broad “virtio-mmio region” cap.

---

## Current state (repo reality)

Implemented and proven:

- `DeviceMmio` capability exists in kernel.
- `SYSCALL_MMIO_MAP` exists and is wrapped by `nexus_abi::mmio_map`.
- QEMU harness requires `SELFTEST: mmio map ok` and it is emitted by `selftest-client`.

Still incomplete for “foundation complete”:

- MMIO cap distribution still has bring-up hard-wiring (kernel name-check in spawn path) for a fixed slot/window.
- Kernel negative tests listed in the task are not yet present.
- Per-device window distribution (rng/net/blk split) must replace broad/shared windows.

---

## What “Done” means (stop conditions, summarized)

### Kernel tests (required)

Add negative tests proving:

- mapping beyond cap window is rejected
- executable mapping attempt is rejected
- mapping without a cap is rejected

### QEMU proofs (required)

Canonical:

- `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`

Markers:

- `SELFTEST: mmio map ok` (already present)
- plus at least one proof that a **designated owner service** (not `selftest-client`) receives the right device window via init distribution and can map it successfully.

---

## Guardrails / “Don’t accidentally do this”

- Do **not** keep kernel name/string checks as the long-term distribution mechanism.
- Do **not** grant broad “all virtio-mmio slots” windows to multiple services once v1 is “foundation complete”.
- Do **not** add IRQ/DMA scope into v1 silently; that belongs to follow-ups (tracked by the driver tracks).
- Keep all proofs deterministic (no flaky timing, no “log grep is truth”).

---

## Suggested implementation slices (PR order)

1. **Document the normative v1 contract** (already written into the task):
   - init distributes caps; `policyd` authorizes; audits to `logd`; per-device windows.
2. **Replace bring-up name-check distribution**:
   - move cap distribution to init configuration and policy-gated delegated checks.
3. **Add kernel negative tests** for `sys_mmio_map` bounds/W^X/no-cap.
4. **Add QEMU proof** that a real owner service maps its specific device window (net/blk/rng), not only `selftest-client`.

---

## Protected zones reminder

This task touches protected zones:

- `source/kernel/**`
- `source/libs/**` (e.g., `nexus-abi`)
- `scripts/**`

Get explicit approval before modifying protected files (unless already granted in the current session).
