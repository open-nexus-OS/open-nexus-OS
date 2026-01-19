---
title: TASK-0031 Zero-copy VMOs v1: shared RO buffers via existing VMO syscalls + handle transfer (plumbing, host-first, OS-gated)
status: Draft
owner: @runtime
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - Zero-Copy App Platform (consumer track): tasks/TRACK-ZEROCOPY-APP-PLATFORM.md
  - IPC/rights model: docs/rfcs/RFC-0005-kernel-ipc-capability-model.md
  - Depends-on (OS DSoftBus mux v2): tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md
  - Depends-on (persistence/statefs): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Depends-on (supply-chain digests): tasks/TASK-0029-supply-chain-v1-sbom-repro-sign-policy.md
  - Testing contract: scripts/qemu-test.sh
  - Unblocks: tasks/TRACK-DRIVERS-ACCELERATORS.md (zero-copy DMA buffers for GPU/NPU/VPU/Audio/Camera/ISP)
  - Unblocks: tasks/TRACK-NETWORKING-DRIVERS.md (zero-copy packet buffers)
---

## Context

The vision explicitly calls for **VMO/filebuffer** on the data plane for large payloads (low/zero copy).
The repo already exposes OS VMO syscalls in `nexus-abi`:

- `vmo_create`, `vmo_write`, `vmo_map`, `vmo_destroy`
- `as_map` and a `Rights::MAP` bit for capability transfer.

However, many consumers in the roadmap (remote-fs, mux v2 VMO frames, statefs fast paths) are not yet implemented.
So v1 must focus on **plumbing** and **honest gating**: provide a robust VMO abstraction and prove sharing works
where the kernel ABI already supports it.

Track alignment: this is a cross-cutting foundation for “device-class” services (GPU/NPU/Audio/Video) and future
networking zero-copy paths (see `tasks/TRACK-DRIVERS-ACCELERATORS.md` and `tasks/TRACK-NETWORKING-DRIVERS.md`).

## Goal

Provide a userspace “VMO handle” abstraction that:

- can represent large read-only buffers,
- can be mapped in-process for streaming hash/verify without extra copies,
- can be transferred to another process **if the kernel capability model supports it**,
- is bounded and testable on host and OS.

## Non-Goals

- Full “VFS splice → VMO” (requires writable VFS + provider hooks; separate task once VFS/statefs exist).
- DSoftBus mux VMO frames (separate task once mux v2 exists).
- Kernel changes (this task must only use existing syscalls/capabilities).

## Constraints / invariants (hard requirements)

- Kernel untouched.
- No fake success: “zero-copy” markers only after verifying a consumer mapped/consumed the shared VMO.
- Bounded memory: cap max VMO length per operation; cap number of live VMOs in registries.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **RED (cross-process share feasibility)**:
  - VMOs are already a `CapabilityKind::Vmo { base, len }` in the kernel cap table and can be moved via
    `SYSCALL_CAP_TRANSFER` (subject to rights). That suggests cross-process sharing is feasible without new kernel work.
  - Still required: an end-to-end proof in QEMU that a VMO cap can be transferred and mapped read-only by the receiver.
- **YELLOW (read-only sealing semantics)**:
  - We need a clear definition for “sealed RO” in OS:
    - preferred: kernel enforces RO mapping and prevents later write mappings,
    - acceptable v1: library-level convention + only map as RO (documented as not a hard boundary).

## Security considerations

### Threat model

- **VMO content tampering**: Receiver modifies "read-only" VMO if sealing not enforced
- **Capability leakage**: VMO handle transferred to unauthorized process
- **Information disclosure**: Sensitive data in VMO leaked to wrong recipient
- **Use-after-free**: Sender destroys VMO while receiver still holds mapping
- **Size confusion**: Receiver maps VMO with wrong size, reads beyond bounds

### Security invariants (MUST hold)

- VMO transfer MUST be capability-gated (only holders of VMO cap can transfer)
- RO-sealed VMOs MUST NOT allow write mappings (kernel-enforced or library convention)
- VMO mappings MUST respect capability rights (READ/WRITE/MAP)
- VMO handles MUST be unforgeable (kernel-managed capability slots)
- VMO size MUST be immutable after creation (no resize after seal)

### DON'T DO

- DON'T allow write mappings of RO-sealed VMOs
- DON'T transfer VMO capabilities to untrusted processes
- DON'T include sensitive data in VMOs without access control
- DON'T allow VMO resize after RO seal
- DON'T trust receiver-provided size (use VMO's intrinsic size)

### Attack surface impact

- **Significant**: VMO transfer is a trust boundary between processes
- **Data plane risk**: Large payloads (images, files) may contain sensitive data
- **Requires clear RO sealing semantics**: Ambiguity could lead to data corruption

### RO Sealing Semantics (Decision Point)

**v1 Decision**: Library-level convention + only map as RO

- VMO marked "sealed" in userspace metadata
- All mappings use RO flags
- Kernel does NOT enforce (no "immutable VMO" capability bit in v1)
- **Documented limitation**: Not a hard security boundary against malicious receiver

**Future (v2+)**: Kernel-enforced RO sealing

- Add `Rights::SEAL` capability bit
- Kernel rejects write mappings of sealed VMOs
- Syscall returns `EPERM` on seal violation

### Mitigations

- VMO capabilities managed by kernel (unforgeable handles)
- Transfer requires explicit `cap_transfer` syscall with rights subset
- RO mappings enforced at page table level (USER|RO, never WRITE)
- Size bounds checked at map time (reject oversized mappings)
- Documentation explicitly states v1 RO sealing is library convention

### Security proof

#### Audit tests (negative cases)

- Command(s):
  - `cargo test -p nexus-vmo -- reject --nocapture`
- Required tests:
  - `test_reject_unauthorized_transfer` — no VMO cap → transfer denied
  - `test_reject_oversized_mapping` — map beyond VMO size → denied
  - `test_ro_mapping_enforced` — RO VMO mapped with RO flags only

#### Hardening markers (QEMU)

- `vmo: producer sent handle` — transfer works
- `vmo: consumer mapped ok` — RO mapping succeeds
- `vmo: sha256 ok` — zero-copy read verified
- `SELFTEST: vmo share ok` — end-to-end proof

## Contract sources (single source of truth)

- ABI surface: `source/libs/nexus-abi/src/lib.rs` (VMO + AS map syscalls, cap_transfer)
- Vision "data plane VMO/filebuffer": `docs/agents/VISION.md`

## Stop conditions (Definition of Done)

### Proof (Host) — required

Deterministic host tests:

- `Vmo` can wrap bytes/file-range and provide slices without copying.
- A “transfer” simulation proves API shape (even if OS transfer is gated).

### Proof (OS / QEMU) — required if transfer is feasible today

Add a minimal two-process proof:

- producer allocates VMO, writes payload, seals RO (as defined), transfers handle to consumer,
- consumer maps VMO read-only and computes `sha256`,
- consumer replies digest to producer; producer compares to expected digest.

Markers (order tolerant):

- `vmo: producer sent handle`
- `vmo: consumer mapped ok`
- `vmo: sha256 ok`
- `SELFTEST: vmo share ok`

Notes:

- Postflight scripts must delegate to canonical harness/tests; no independent “log greps = success”.

## Touched paths (allowlist)

- `userspace/memory/` (new `nexus-vmo` crate)
- `source/libs/nexus-abi/` (only if wrapper fixes are required; otherwise no changes)
- `source/apps/selftest-client/` (OS proof path)
- `userspace/exec-payloads/` or a small new demo app for consumer (if needed)
- `scripts/qemu-test.sh`
- `docs/storage/vmo.md` (new)
- `docs/testing/index.md`

## Plan (small PRs)

1. **Create `userspace/memory/nexus-vmo`**
   - API:
     - `Vmo::create(len)`
     - `Vmo::write(offset, bytes)` (bounded)
     - `Vmo::map_ro()` returning a `VmoMapping` view for streaming reads
     - `Vmo::len()`, `VmoSlice`
   - Host backend:
     - uses `Arc<[u8]>` / `memmap2` for tests (not a kernel VMO).
   - OS backend:
     - uses existing `nexus-abi` VMO syscalls and maps RO.

2. **Define “transfer” surface**
   - If VMOs are capabilities:
     - provide `Vmo::transfer_to(pid, rights)` wrapper using `cap_transfer`.
   - Otherwise:
     - document limitation and keep transfer API stubbed with explicit `Unsupported`.

3. **OS selftest proof (if feasible)**
   - Add a tiny consumer process that:
     - receives a VMO handle and length via IPC,
     - maps RO and computes sha256,
     - replies digest.
   - Add deterministic markers listed above.

4. **Docs**
   - `docs/storage/vmo.md`: what a VMO is in this system, RO sealing semantics, limits, how to test.

## Follow-ups (separate tasks)

- VFS splice to VMO registries and budgets (once `/state` write path exists).
- DSoftBus mux v2 VMO frames with capability advertise/fallback (once mux v2 exists).
- Packagefs/bundlemgr/statefs fast paths (once those services exist in OS builds).
