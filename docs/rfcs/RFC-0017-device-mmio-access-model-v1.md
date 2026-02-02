# RFC-0017: Device MMIO Access Model v1

- Status: Done
- Owners: @kernel-team @runtime
- Created: 2026-02-02
- Last Updated: 2026-02-02
- Links:
  - Tasks: `tasks/TASK-0010-device-mmio-access-model.md` (execution + proof)
  - Related RFCs: `docs/rfcs/RFC-0005-kernel-ipc-capability-model.md` (IPC/cap model)
  - Related RFCs: `docs/rfcs/RFC-0004-safe-loader-guards.md` (W^X enforcement)
  - Related RFCs: `docs/rfcs/RFC-0015-policy-authority-audit-baseline-v1.md` (policy authority)
  - Related RFCs: `docs/rfcs/RFC-0016-device-identity-keys-v1.md` (uses MMIO primitive)

## Consumers (not exhaustive, but canonical)

Direct consumers / gates (explicitly depend on this MMIO foundation):

- `TASK-0008B` (virtio-rng via `rngd`)
- `TASK-0003` / `TASK-0004` (virtio-net via `netstackd`)
- `TASK-0009` (persistence via virtio-blk + statefs)
- `TASK-0032` (blk-backed OS packagefs path, optional)
- `TASK-0247` (bring-up v1.1b, includes `virtioblkd`)
- `TASK-0246` (host image builder for virtio-blk, prerequisite for blk proofs)

Broader “future device MMIO” gates (tracks and later tasks):

- `TRACK-DRIVERS-ACCELERATORS`
- `TRACK-NETWORKING-DRIVERS`

## Status at a Glance

- **Phase 0 (Kernel primitive)**: ✅ Complete
- **Phase 1 (Init-controlled distribution)**: ✅ Complete
- **Phase 2 (Per-device windows)**: ✅ Complete (net/rng; blk follow-up when device present)

Definition:

- "Complete" means the **contract** is defined and the **proof gates** are green (tests/markers). It does not mean "never changes again".
- This RFC is **Done** once the contract is proven for the required virtio consumers (net/rng/blk) in QEMU as applicable.

## Scope boundaries (anti-drift)

This RFC is a **design seed / contract** for safe userspace MMIO access. Implementation planning and proofs live in `tasks/TASK-0010-device-mmio-access-model.md`.

- **This RFC owns**:
  - `DeviceMmio` capability kind semantics (bounded physical window)
  - `SYSCALL_MMIO_MAP` syscall contract (USER|RW only, never EXEC)
  - Capability distribution model (init-controlled, policy-gated, audited)
  - Per-device window semantics (least privilege)
  - Security invariants for device MMIO access

- **This RFC does NOT own**:
  - IRQ routing/delivery to userspace (polling-only in v1; follow-up RFC)
  - DMA buffer ownership/IOMMU isolation (follow-up; see `TRACK-DRIVERS-ACCELERATORS.md`)
  - Full device manager / dynamic enumeration framework (follow-up)
  - Device-specific driver implementations (virtio-net, virtio-blk, etc.)

### Relationship to tasks (single execution truth)

- `tasks/TASK-0010-device-mmio-access-model.md` defines **stop conditions** and **proof commands**.
- This RFC defines the contract; the task implements and proves it.

## Context

Our vision is "kernel minimal, drivers in userspace". On QEMU `virt`, virtio devices are MMIO-mapped.
Without a safe, capability-gated mechanism to expose device MMIO to specific userspace services,
true userspace virtio frontends (net/blk/rng) are impossible.

This RFC defines the minimal kernel/userspace contract for:

- Receiving a capability representing a specific device MMIO range
- Mapping it into userspace read/write (never executable)
- Using it to drive a virtio queue implementation in userspace

Track alignment: this is a foundational prerequisite for:

- `TRACK-DRIVERS-ACCELERATORS.md` (GPU/NPU/VPU/Audio/Camera/ISP userspace drivers)
- `TRACK-NETWORKING-DRIVERS.md` (virtio-net userspace frontend)

## Goals

- Provide a capability-gated mechanism for userspace to map device MMIO regions
- Enforce W^X at the hardware boundary (never executable device mappings)
- Define an explicit, auditable capability distribution model (init-controlled, policy-gated)
- Support per-device bounded windows (least privilege)
- Enable userspace virtio frontends on QEMU `virt`

## Non-Goals

- A full device manager and dynamic enumeration framework
- Exposing arbitrary physical memory to userspace
- Interrupt routing / delivery to userspace (polling-only is acceptable for v1)
- DMA buffer pinning, IOMMU/GPU-MMU isolation, or "safe DMA" primitives (follow-up)

## Constraints / invariants (hard requirements)

- **Security floor**:
  - Mapping MUST be capability-gated (no ambient MMIO)
  - Mappings MUST be **USER + RW**, never executable (W^X at hardware boundary)
  - Mapping range MUST be fixed and bounded to the device BAR/MMIO window
- **Kernel minimal**: kernel provides enforce-only primitive; policy and driver logic remain in userspace
- **Determinism**: mapping errors deterministic; no "success" logs without real capability
- **No selftest-only escape hatch**: device MMIO capabilities MUST be distributable to real owner services (e.g., `rngd`, `virtionetd`, `virtioblkd`), not only to selftests
- **Bounded resources**: per-device windows preferred over shared windows
- **Stubs policy**: any stub must be explicitly labeled as bring-up-only and removed before v1 "foundation complete"

## Proposed design

### Contract / interface (normative)

#### Capability model

The kernel provides `CapabilityKind::DeviceMmio { base, len }` with `Rights::MAP`:

```rust
/// Device MMIO window (physical base + length).
/// Mapped into userspace only via SYSCALL_MMIO_MAP.
pub enum CapabilityKind {
    // ...
    DeviceMmio { base: usize, len: usize },
}
```

#### Syscall: `SYSCALL_MMIO_MAP` (ID = 27)

Maps a page from a `DeviceMmio` capability window into the caller's address space.

**Arguments:**
- `a0`: capability slot index (must reference a `DeviceMmio` capability with `Rights::MAP`)
- `a1`: virtual address (page-aligned)
- `a2`: offset within the capability window (page-aligned)

**Returns:**
- `0` on success
- Negative errno on failure:
  - `EPERM (1)`: insufficient rights or wrong capability kind
  - `EINVAL (22)`: offset out of bounds or invalid arguments

**Security invariants (enforced by kernel):**
- MUST map only within the bounded window `[base, base+len)`
- MUST create mappings as **USER|RW, never executable** (PageFlags: `VALID | USER | READ | WRITE`)
- MUST fail deterministically on:
  - Invalid offset (beyond device window)
  - Missing capability
  - Wrong capability kind
  - Executable mapping attempt (not possible via this syscall)

#### Userspace wrapper

```rust
/// Maps a page from a DeviceMmio capability into the caller's address space.
///
/// Security invariants (enforced by kernel):
/// - mapping is USER + RW
/// - mapping is never executable
/// - mapping is bounded to the capability window
pub fn mmio_map(handle: Handle, va: usize, offset: usize) -> SysResult<()>;
```

### Distribution model (normative)

#### Init is the distributor

- Init (e.g., init-lite) is responsible for placing device capabilities into designated services at spawn time (or via explicit handoff IPC during early boot)
- The kernel MUST NOT grant device MMIO caps based on service names/strings once v1 is "foundation complete"

#### Policy is the authority

- Whether a given service is allowed to receive a device MMIO cap is a deny-by-default decision by `policyd`, bound to `sender_service_id` (no payload identity)
- Init uses delegated policy checks (same pattern used elsewhere for privileged routing) and logs allow/deny via `logd`

#### Auditability

- Capability distribution events MUST be auditable (allow/deny + target service + device kind/window), without leaking secrets

### Per-device windows (least privilege)

Prefer **per-device** bounded windows over one broad "virtio-mmio region" cap:

| Device      | Window        | Owner Service |
|-------------|---------------|---------------|
| virtio-rng  | 1× 4KiB slot (discovered at boot) | `rngd` |
| virtio-net  | 1× 4KiB slot (discovered at boot) | `netstackd` / `virtionetd` |
| virtio-blk  | 1× 4KiB slot (discovered at boot) | `virtioblkd` / block authority |

If early bring-up uses a shared window, it MUST be explicitly labeled as bring-up-only and removed before calling v1 "foundation complete".

### Deterministic slots (early boot ergonomics)

Early boot MAY use deterministic capability slot assignments (for reproducible bring-up), but:

- The assignment MUST be owned by init configuration, not hard-coded kernel name checks
- It MUST be consistent with the IPC/capability model (CAP_MOVE hygiene, no leaks)

### Phases / milestones (contract-level)

- **Phase 0 (Kernel primitive)**: ✅ Complete
  - `DeviceMmio` capability kind exists
  - `SYSCALL_MMIO_MAP` enforces USER|RW and bounds
  - QEMU marker `SELFTEST: mmio map ok` passes

- **Phase 1 (Init-controlled distribution)**: ✅ Complete
  - Kernel name-check distribution replaced with init-controlled distribution
  - Policy-gated decisions via `policyd` + audit via `logd`
  - Kernel negative tests for bounds/no-cap/exec-attempt

- **Phase 2 (Per-device windows)**: ✅ Complete (net/rng)
  - Init probes the virtio-mmio bus and grants per-device windows for net/rng
  - QEMU proof that a designated owner service maps its specific device window (`rngd: mmio window mapped ok`)

## Consumer prerequisites (explicit, to avoid “fake success”)

This RFC defines the **v1 foundation contract**. Some follow-up tasks require **device-specific presence + proofs** beyond the net/rng coverage that is currently exercised in QEMU:

- **virtio-blk consumers** (e.g. `TASK-0009` Persistence v1) require:
  - a **virtio-blk device present in QEMU**, and
  - a **block owner service** receiving the **blk** `DeviceMmio` cap (policy-gated + audited), and
  - a deterministic QEMU proof marker (e.g. `virtioblkd: mmio window mapped ok` or `blk: virtio-blk up`).

Those consumers are **unblocked at the contract level** (kernel primitive + distribution model are Done), but may still be **blocked at the device/service level** until the blk owner is implemented and proven in QEMU.

### Implemented consumer proofs (repo reality)

- `rngd: mmio window mapped ok`
- `virtioblkd: mmio window mapped ok`

## Security considerations

### Threat model

- **Arbitrary memory access**: Malicious userspace service requests MMIO mapping outside device window
- **DMA attacks**: Compromised driver uses DMA to read/write arbitrary physical memory (out of scope for v1)
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
- DON'T keep kernel name/string checks as the long-term distribution mechanism

### Mitigations

- `CapabilityKind::DeviceMmio { base, len }` bounds physical window precisely
- `SYSCALL_MMIO_MAP` enforces **USER|RW, never EXEC** at syscall level
- Fixed, build-time device list for QEMU `virt` (no dynamic enumeration in bring-up)
- Capability distribution targets only the designated driver service
- W^X enforcement at page table level (no execute permission on device pages)

## Failure model (normative)

### Error conditions

| Condition | Error | Behavior |
|-----------|-------|----------|
| Missing capability | `EPERM` | Mapping denied |
| Wrong capability kind (not DeviceMmio) | `EPERM` | Mapping denied |
| Offset outside window | `EPERM` | Mapping denied |
| Insufficient rights (no MAP) | `EPERM` | Mapping denied |
| Invalid arguments | `EINVAL` | Mapping denied |

### Retry safety

- All failures are deterministic and safe to retry (no state mutation on failure)
- No silent fallback: if mapping fails, userspace must handle the error explicitly

## Proof / validation strategy (required)

### Proof (Host - kernel negative tests)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p neuron -- mmio_reject --nocapture
```

Required tests:
- `test_reject_mmio_outside_window` — mapping beyond device bounds → denied
- `test_reject_mmio_exec` — executable mapping attempt → denied (N/A via this syscall; implicit)
- `test_reject_mmio_no_cap` — mapping without capability → denied

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os
```

### Deterministic markers

- `SELFTEST: mmio map ok` — legitimate mapping works (already required)
- `rngd: mmio window mapped ok` — designated owner service mapped its window (present and verified)
- `virtioblkd: mmio window mapped ok` — virtio-blk consumer proof (device present, cap distributed, mapping works)
- `SELFTEST: mmio policy deny ok` — policy deny-by-default is enforced for non-matching MMIO capabilities (e.g., netstackd denied device.mmio.blk)

## What is still missing (to reach “foundation complete”)

This section is intentionally redundant with `tasks/TASK-0010-device-mmio-access-model.md` stop conditions.
If any item here is unchecked, **this RFC must remain In Progress**.

- [x] **Remove kernel name/string distribution as the foundation mechanism**:
  - MMIO caps are distributed by init (configuration + handoff), not granted via kernel name checks.
- [x] **Policy-gated distribution (deny-by-default) via `policyd`**:
  - Decisions bound to kernel identity (`sender_service_id`), not payload strings.
- [x] **Audit trail via `logd`**:
  - Allow/deny records include target service + device/window, without secrets.
- [x] **Per-device bounded windows (least privilege)**:
  - Init probes virtio-mmio slots and grants net/rng windows; blk follows when device/service is present.
- [x] **Kernel negative tests fully match the task’s required cases**:
  - Includes the “exec attempt” rejection proof requirement from TASK-0010.

## Alternatives considered

1. **Ambient device access (no capabilities)**: Rejected — violates security model; no isolation between services
2. **Kernel device manager**: Rejected — kernel should remain minimal; device enumeration belongs in userspace
3. **Kernel name-check distribution (long-term)**: Rejected — bring-up convenience only; v1 requires init-controlled distribution

## Open questions

- Q1: Should per-device windows use fixed slot assignments or dynamic allocation? **Resolution**: Fixed assignments for bring-up determinism; init-owned configuration.
- Q2: How should IRQ routing be handled in follow-up? **Deferred**: Out of scope for v1; separate RFC.

---

## Implementation Checklist

**This section tracks implementation progress. Update as phases complete.**

- [x] **Phase 0**: Kernel primitive — proof: `SELFTEST: mmio map ok`
- [x] **Phase 1**: Init-controlled distribution — proof: init distributes caps; policyd gates; logd audits; no kernel name-check foundation
- [x] **Phase 2**: Per-device windows — proof: per-device windows (net/rng) + owner-service mapping proof
- [x] Task linked: `tasks/TASK-0010-device-mmio-access-model.md`
- [x] QEMU markers appear in `scripts/qemu-test.sh` and pass (includes `rngd: mmio window mapped ok`)
- [x] Security-relevant negative tests exist and match TASK-0010 required cases (incl. exec-attempt proof requirement)
