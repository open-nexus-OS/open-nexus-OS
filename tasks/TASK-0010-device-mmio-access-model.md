---
title: TASK-0010 Device access model v1: safe userspace MMIO for virtio devices (enables virtio-net/virtio-blk)
status: In Review
owner: @kernel-team @runtime
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - RFC: docs/rfcs/RFC-0017-device-mmio-access-model-v1.md
  - Related RFC: docs/rfcs/RFC-0005-kernel-ipc-capability-model.md
  - Used-by (already done): tasks/TASK-0008B-device-identity-keys-v1-virtio-rng-rngd-keystored-keygen.md
  - Used-by (already done): tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md
  - Used-by (already done): tasks/TASK-0004-networking-dhcp-icmp-dsoftbus-dual-node.md
  - Unblocks: tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Unblocks: tasks/TASK-0247-bringup-rv-virt-v1_1b-os-smp-hsm-ipi-virtioblkd-packagefs-selftests.md
  - Unblocks: tasks/TASK-0249-bringup-rv-virt-v1_2b-os-virtionetd-netstackd-fetchd-echod-selftests.md
  - Unblocks: tasks/TRACK-DRIVERS-ACCELERATORS.md (GPU/NPU/VPU/Audio/Camera/ISP userspace drivers)
  - Unblocks: tasks/TRACK-NETWORKING-DRIVERS.md (virtio-net userspace frontend)
follow-up-tasks:
  - TASK-0009: Persistence v1 (virtio-blk + statefs) — blocked until this task is complete enough for virtio-blk
  - TASK-0247: RISC-V bring-up v1.1b (virtioblkd + pkgfs from disk) — needs virtio-blk MMIO caps
  - TASK-0249: RISC-V bring-up v1.2b (virtionetd/netstackd) — needs virtio-net MMIO caps
  - TASK-0032: packagefs v2 blk-backed OS path (optional) — gated on virtio-blk MMIO caps
  - TASK-0280: DriverKit v1 core contracts — depends on MMIO caps being real and auditable
  - TASK-0284: DMA buffer ownership prototype — builds on the same device-class boundary (MMIO now, DMA later)
  - TASK-0251: Display v1.0b OS (fbdevd) — may require MMIO caps for real devices
  - TASK-0253: Input v1.0b OS (hidrawd/touchd) — may require MMIO caps for real devices
  - TASK-0255: Audio v0.9b OS (audiod + codec/i2s) — may require MMIO caps for real devices
  - TASK-0257: Battery v0.9b OS — may require MMIO caps for fuel-gauge/charger
  - TASK-0259: Sensor bus v0.9b OS (i2cd/spid) — may require MMIO caps for bus controllers
---

## Context

Our vision is “kernel minimal, drivers in userspace”. On QEMU `virt`, virtio devices are MMIO.
Historically userspace could map VMOs (`as_map`) but could not map device MMIO ranges.
Today the kernel provides a **DeviceMmio capability + mapping syscall**, so userspace can map MMIO
**only when explicitly granted a bounded MMIO capability**.

That makes true userspace virtio frontends (net/blk) **impossible** unless the kernel provides a safe,
capability-gated way to expose device MMIO to a specific userspace driver/service.

Track alignment: this is a foundational prerequisite for both `tasks/TRACK-DRIVERS-ACCELERATORS.md` and
`tasks/TRACK-NETWORKING-DRIVERS.md` (userspace device-class services require a safe MMIO/IRQ/DMA contract).

## Goal

Provide the minimal kernel/userspace contract to allow a userspace service to:

- receive a capability representing a specific device MMIO range (virtio-net/virtio-blk),
- map it into its address space read/write (never executable),
- and use it to drive a virtio queue implementation in userspace.

## Non-Goals

- A full device manager and dynamic enumeration framework.
- Exposing arbitrary physical memory to userspace.
- Interrupt routing / delivery to userspace (polling-only is acceptable for v1).
- DMA buffer pinning, IOMMU/GPU-MMU isolation, or “safe DMA” primitives (follow-ups; see Drivers/Accelerators track).

## Constraints / invariants (hard requirements)

- **Security floor**:
  - mapping must be capability-gated (no ambient MMIO),
  - mappings must be **USER + RW**, never executable,
  - mapping range must be fixed and bounded to the device BAR/MMIO window.
- **Kernel minimal**: provide a tiny primitive; policy and driver logic remain in userspace.
- **Determinism**: mapping errors deterministic; no “success” logs without real capability.
- **No selftest-only escape hatch**: device MMIO capabilities must be distributable to the **real owner service**
  (e.g. `rngd`, `virtionetd`/`netstackd`, `virtioblkd`/`statefsd`), not only to selftests.
- **No name-check distribution as the v1 foundation**: temporary bring-up wiring may exist, but v1 “foundation complete”
  requires that MMIO capabilities are distributed by init configuration and gated by `policyd` (auditable), not by
  kernel string/name checks.

## Normative v1 contract (MMIO capability + distribution)

This section defines what follow-up tasks (networking, persistence, driverkit) may **rely on** once v1 is “foundation complete”.

### Capability model

- The kernel provides `CapabilityKind::DeviceMmio { base, len }` with `Rights::MAP`.
- The kernel provides `SYSCALL_MMIO_MAP` / userspace `nexus_abi::mmio_map(handle, va, offset)`:
  - MUST map only within the bounded window \([base, base+len)\) using page granularity.
  - MUST create mappings as **USER|RW, never executable**.
  - MUST fail deterministically on invalid offsets, missing caps, or out-of-window mapping attempts.

### Distribution model (who gets MMIO caps)

- **Init is the distributor**:
  - Init (e.g. init-lite) is responsible for placing device capabilities into designated services at spawn time
    (or via an explicit handoff IPC during early boot).
  - The kernel MUST NOT grant device MMIO caps based on service names/strings once v1 is “foundation complete”.
- **Policy is the authority**:
  - Whether a given service is allowed to receive a device MMIO cap is a deny-by-default decision by `policyd`,
    bound to `sender_service_id` (no payload identity).
  - Init uses delegated policy checks (the same pattern used elsewhere for privileged routing) and logs allow/deny via `logd`.
- **Auditability**:
  - Capability distribution events MUST be auditable (allow/deny + target service + device kind/window), without leaking secrets.

### Least privilege (per-device windows)

- Prefer **per-device** bounded windows over one broad “virtio-mmio region” cap:
  - virtio-rng owner gets only the rng window,
  - virtio-net owner gets only the net window,
  - virtio-blk owner gets only the blk window.
- If early bring-up uses a shared window, it MUST be explicitly labeled as bring-up only and removed before calling v1 “foundation complete”.

### Deterministic slots (early boot ergonomics)

- Early boot MAY use deterministic capability slot assignments (for reproducible bring-up), but:
  - the assignment MUST be owned by init configuration, not hard-coded kernel name checks,
  - and it MUST be consistent with the IPC/capability model (CAP_MOVE hygiene, no leaks).

## Red flags / decision points

- **RED (blocking / must decide now)**:
  - This task **requires kernel work**. If "kernel untouched" were absolute, then userspace virtio drivers
    must be deferred or replaced with a different backend (e.g., host-provided VMO block service) and the
    vision "userspace drivers" is not achievable on QEMU `virt`.
  - **Status (today)**: the minimal kernel primitive exists (DeviceMmio cap + map syscall) and is proven in QEMU.
    The remaining “foundation complete” work is to remove bring-up hard-wiring and make MMIO capability distribution
    init-controlled + policy-gated + auditable (see normative contract above).
  - Boundary (v1): kernel work here must remain a **minimal enforce-only primitive** (cap kind + map syscall).
    Policy decisions and capability distribution are explicitly userspace responsibilities (init + `policyd` + audit).
- **YELLOW (risky / likely drift / needs follow-up)**:
  - Device enumeration: we can start with a fixed, build-time wired device list for QEMU `virt`, but must
    document how it evolves.
    v1 rule: enumeration may live in init/DT parsing (trusted) but must not be exposed as an ambient kernel “device list”
    to untrusted services.

## Security considerations

### Threat model

- **Arbitrary memory access**: Malicious userspace service requests MMIO mapping outside device window
- **DMA attacks**: Compromised driver uses DMA to read/write arbitrary physical memory
- **Privilege escalation via MMIO**: Driver exploits MMIO access to compromise kernel
- **Device spoofing**: Attacker tricks kernel into providing MMIO cap for wrong device
- **Execute from MMIO**: Attacker attempts to execute code from mapped MMIO region

### Security invariants (MUST hold)

- MMIO mappings MUST be capability-gated (no ambient access)
- MMIO mappings MUST be bounded to the exact device BAR/window (no overmap)
- MMIO mappings MUST be **USER|RW only, NEVER executable** (W^X at hardware boundary)
- Only designated driver services may receive device MMIO capabilities
- Device capability distribution MUST be explicit and auditable
- DMA buffers (future) MUST be allocated from restricted memory regions

### DON'T DO

- DON'T allow any executable mappings of device MMIO regions
- DON'T grant MMIO capabilities to arbitrary services
- DON'T allow MMIO mappings outside the device's designated physical window
- DON'T expose device enumeration to untrusted services
- DON'T skip capability checks for "trusted" driver services

### Attack surface impact

- **Critical**: MMIO access is a kernel-userland trust boundary
- **Highest privilege**: Misconfigured MMIO access could compromise kernel
- **Requires security review**: Any changes to MMIO mapping must be reviewed

### Mitigations

- `CapabilityKind::DeviceMmio { base, len }` bounds physical window precisely
- `SYSCALL_MMIO_MAP` enforces **USER|RW, never EXEC** at syscall level
- Fixed, build-time device list for QEMU `virt` (no dynamic enumeration in bring-up)
- Capability distribution targets only the designated driver service (`netstackd`, `virtionetd`)
- W^X enforcement at page table level (no execute permission on device pages)

## Security proof

### Audit tests (negative cases)

- Command(s):
  - `cargo test -p neuron -- mmio_reject --nocapture` (to be added by this task; name is a suggested filter)
- Required tests:
  - `test_reject_mmio_outside_window` — mapping beyond device bounds → denied
  - `test_reject_mmio_exec` — executable mapping attempt → denied
  - `test_reject_mmio_no_cap` — mapping without capability → denied

### Hardening markers (QEMU)

- `SELFTEST: mmio map ok` — legitimate mapping works
- `kernel: mmio denied (outside window)` — bounds enforcement works
- `kernel: mmio denied (exec attempt)` — W^X enforced

## Contract sources (single source of truth)

- Loader/mapping invariants: `docs/rfcs/RFC-0004-safe-loader-guards.md` (W^X and mapping safety expectations)
- IPC/cap model: `docs/rfcs/RFC-0005-kernel-ipc-capability-model.md`

## Stop conditions (Definition of Done)

- Kernel tests exist for capability + mapping invariants (negative cases above).
- QEMU selftest marker proving a userspace driver can map its MMIO window and read a known virtio register:
  - `SELFTEST: mmio map ok`
- QEMU proof that the MMIO capability reaches a **designated owner service** without relying on selftest-only paths
  (init distributes the cap; policy gates/audit the decision).

## Current state

**Done** (2026-02-02):

- Kernel cap kind exists: `CapabilityKind::DeviceMmio { base, len }` (bounded physical window).
- Kernel syscall exists: `SYSCALL_MMIO_MAP` enforcing **USER|RW** and **never EXEC** for device mappings.
- Userspace wrappers exist in `nexus-abi`:
  - `mmio_map(handle, va, offset)` (maps a page within the cap window at `va`)
  - `cap_query(cap, out)` (diagnostics: base/len + kind tag)
- OS selftest exercises the path end-to-end (maps and reads a known virtio-mmio register) and emits:
  - `SELFTEST: mmio map ok`
- Canonical QEMU harness requires the marker (no silent “green”).
- Init-controlled distribution (policy-gated via `policyd`, audited via `logd`) replaces kernel name checks.
- Per-device windows: init probes virtio-mmio slots and grants **net** and **rng** windows.
- Kernel negative tests added: `test_reject_mmio_no_cap`, `test_reject_mmio_wrong_cap_kind`,
  `test_reject_mmio_outside_window`, `test_reject_mmio_insufficient_rights`, `test_reject_mmio_exec`.
- Designated owner service proof: `rngd: mmio window mapped ok` marker verified in QEMU.
- RFC-0017 seed exists and is kept in sync with the proofs.

Virtio consumers proven in QEMU:

- `rngd: mmio window mapped ok`
- `virtioblkd: mmio window mapped ok`

Policy enforcement proven in QEMU:

- `SELFTEST: mmio policy deny ok` (deny-by-default for non-matching capabilities)

## Touched paths (allowlist)

- `source/kernel/neuron/` (minimal new capability kind and mapping syscall support)
- `source/libs/nexus-abi/` (userspace wrapper for the new capability/map primitive)
- `source/apps/selftest-client/` (MMIO map proof marker)
- `scripts/qemu-test.sh` (canonical marker harness)
- `docs/` (document the device access model + security invariants)

## Plan (small PRs)

1. Define a new capability kind for device MMIO windows (base, len, allowed flags).
2. Expose a syscall/wrapper to map a device MMIO cap into the caller’s AS (RW|USER, never X).
3. Wire a fixed virtio device list for QEMU `virt` for bring-up (cap distribution to the relevant service).
4. Add a selftest that maps and reads the virtio magic/version register deterministically.

## Acceptance criteria (behavioral)

- Userspace can map only the granted device MMIO window, not arbitrary addresses.
- Mapping is non-executable and respects W^X enforcement at the boundary.
- QEMU marker proves the mapping works.

## Evidence (to paste into PR)

- `cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh` (marker present: `SELFTEST: mmio map ok`)
