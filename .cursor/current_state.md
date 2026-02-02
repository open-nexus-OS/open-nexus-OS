# Cursor Current State (SSOT)

<!--
CONTEXT
This file is the single source of truth for the *current* system state.
It is intentionally compact and overwritten after each completed task.

Rules:
- Prefer structured bullets over prose.
- Include "why" (decision rationale), not implementation narration.
- Reference tasks/RFCs/ADRs with relative paths.
-->

## Current architecture state
- **last_decision**: `docs/rfcs/RFC-0017-device-mmio-access-model-v1.md` (Status: Done)
- **rationale**:
  - Userspace drivers need safe device access without kernel trust assumptions
  - Capability-gated MMIO prevents arbitrary physical memory access
  - Init-controlled distribution enables policy enforcement + audit trail
  - Per-device windows enforce least privilege (no broad MMIO grants)
- **active_constraints**:
  - No fake success markers (only emit `ok` after real behavior proven)
  - OS-lite feature gating (`--no-default-features --features os-lite`)
  - W^X for MMIO (device mappings are USER|RW, never EXEC)
  - Policy decisions bound to kernel `sender_service_id` (not payload strings)
  - All security decisions audited via logd (no secrets in logs)
  - Kernel remains minimal (device enumeration, policy logic in userspace)

## Active invariants (must hold)
- **security**
  - Secrets never logged (device keys, credentials, tokens)
  - Identity from kernel IPC (`sender_service_id`), never payload strings
  - Bounded input sizes; validate before parse; no `unwrap/expect` on untrusted data
  - Policy enforcement via `policyd` (deny-by-default + audit)
  - MMIO mappings are USER|RW and NEVER executable (W^X enforced at page table)
  - Device capabilities require explicit grant (no ambient MMIO access)
  - Per-device windows bounded to exact BAR/window (no overmap)
- **determinism**
  - Marker strings stable and non-random
  - Tests bounded (no infinite/unbounded waits)
  - UART output deterministic for CI verification
  - QEMU runs bounded by RUN_TIMEOUT + early exit on markers
- **build hygiene**
  - OS services use `--no-default-features --features os-lite`
  - Forbidden crates: `parking_lot`, `parking_lot_core`, `getrandom`
  - `just dep-gate` MUST pass before OS commits
  - `just diag-os` verifies OS services compile for riscv64

## Open threads / follow-ups
- `tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md` — READY (virtio-blk MMIO access proven)
- `tasks/TASK-0034-delta-updates-v1_1-persistent-bootctl.md` — blocked on TASK-0009 (needs statefs)
- DMA capability model (future) — out of scope for MMIO v1
- IRQ delivery to userspace (future) — separate RFC needed
- virtio virtqueue operations beyond MMIO probing — follow-up after statefs proven

## Known risks / hazards
- **virtio-blk driver**: currently only MMIO probing proven, not full virtqueue operations
  - Risk: statefs block backend will need full read/write operations
  - Mitigation: scaffold exists in `source/drivers/storage/virtio-blk/`, extend incrementally
- **Policy timing**: early boot race between policyd readiness and init cap distribution
  - Current: retry loops with bounded timeout (1s deadline)
  - Future: explicit readiness channel to avoid retry polling
- **MMIO slot discovery**: dynamic probing of virtio-mmio devices at fixed addresses
  - Current: hardcoded QEMU virt addresses (0x10001000 + 0x1000 * slot)
  - Future: proper device tree parsing if targeting real hardware

## DON'T DO (session-local)
- DON'T add kernel MMIO grants via name-checks (init-controlled distribution only)
- DON'T skip policy checks for "trusted" services (deny-by-default always)
- DON'T emit `ready` or `ok` markers for stub/placeholder paths
- DON'T add `parking_lot` or `getrandom` to OS service dependencies
- DON'T extend TASK-0009 scope to include VFS mount (statefs authority first, mount is follow-up)
- DON'T assume real reboot/VM reset works in v1 (soft reboot = statefsd restart only)
