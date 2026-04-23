---
title: TASK-0031 Zero-copy VMOs v1: shared RO buffers via existing VMO syscalls + handle transfer (plumbing, host-first, OS-gated)
status: In Review
owner: @runtime
created: 2025-12-22
updated: 2026-04-23
depends-on:
  - TASK-0009
  - TASK-0020
  - TASK-0029
follow-up-tasks:
  - TASK-0033
  - TASK-0054B
  - TASK-0054C
  - TASK-0054D
  - TASK-0290
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Contract seed (RFC): docs/rfcs/RFC-0040-zero-copy-vmos-v1-plumbing-host-first-os-gated.md
  - Zero-Copy App Platform (consumer track): tasks/TRACK-ZEROCOPY-APP-PLATFORM.md
  - UI performance philosophy: docs/dev/ui/foundations/quality/performance-philosophy.md
  - Office Suite (consumer): tasks/TRACK-OFFICE-SUITE.md
  - DAW (consumer): tasks/TRACK-DAW-APP.md
  - Live Studio (consumer): tasks/TRACK-LIVE-STUDIO-APP.md
  - Video Editor (consumer): tasks/TRACK-VIDEO-EDITOR-APP.md
  - NexusGfx SDK (consumer): tasks/TRACK-NEXUSGFX-SDK.md
  - NexusMedia SDK (consumer): tasks/TRACK-NEXUSMEDIA-SDK.md
  - Service architecture (control/data plane): docs/adr/0017-service-architecture.md
  - IPC/rights model: docs/rfcs/RFC-0005-kernel-ipc-capability-model.md
  - Depends-on (OS DSoftBus mux v2): tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md
  - Depends-on (persistence/statefs): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Depends-on (supply-chain digests): tasks/TASK-0029-supply-chain-v1-sbom-repro-sign-policy.md
  - Testing contract: scripts/qemu-test.sh
  - Security standards: docs/standards/SECURITY_STANDARDS.md
  - Rust standards: docs/standards/RUST_STANDARDS.md
  - Unblocks: tasks/TRACK-DRIVERS-ACCELERATORS.md (zero-copy DMA buffers for GPU/NPU/VPU/Audio/Camera/ISP)
  - Unblocks: tasks/TRACK-NETWORKING-DRIVERS.md (zero-copy packet buffers)
  - Early UI/kernel perf consumer follow-up: tasks/TASK-0054B-ui-v1a-kernel-ui-perf-floor-zero-copy-qos-hardening.md
  - UI/MM perf consumer follow-up: tasks/TASK-0054D-ui-v1a-kernel-mm-perf-floor-vmo-surface-reuse.md
---

## Context

The vision explicitly calls for **VMO/filebuffer** on the data plane for large payloads (low/zero copy).
The repo already exposes OS VMO syscalls in `nexus-abi`:

- `vmo_create`, `vmo_write`, `vmo_map`, `vmo_map_page`, `vmo_destroy`
- `as_map` and a `Rights::MAP` bit for capability transfer.

However, many consumers in the roadmap (remote-fs, mux v2 VMO frames, statefs fast paths) are not yet implemented.
So v1 must focus on **plumbing** and **honest gating**: provide a robust VMO abstraction and prove sharing works
where the kernel ABI already supports it.

Keystone note (avoid drift):

- This task is a **cross-track keystone**. If it is not real and proven, we should not claim “zero-copy” for:
  - Media pipelines, pro creative apps (DAW/Studio/Video), large document/data workflows (Office/BI), or device-class SDKs.
- Proof must be end-to-end (producer → transfer → consumer map_ro → verify), not “we used a byte buffer type”.

Track alignment: this is a cross-cutting foundation for “device-class” services (GPU/NPU/Audio/Video) and future
networking zero-copy paths (see `tasks/TRACK-DRIVERS-ACCELERATORS.md` and `tasks/TRACK-NETWORKING-DRIVERS.md`).

UI/perf note:

- Early renderer/compositor bring-up may be host-first, but fluid QEMU UI, glass, animation, and media surfaces
  should treat this task as the canonical bulk-buffer floor.
- Follow-up consumers `TASK-0054B` / `TASK-0054D` pull that stance forward explicitly for UI-shaped workloads.

## Current status (2026-04-23)

- `TASK-0009`, `TASK-0020`, and `TASK-0029` are `Done`; prerequisite chain is clear.
- `nexus-abi` already exports `vmo_create`, `vmo_write`, `vmo_map`, `vmo_map_page`, `vmo_destroy`,
  `cap_transfer`, and `cap_transfer_to_slot`, with `Rights::{SEND, RECV, MAP}`.
- Kernel syscall table currently wires `sys_vmo_create` and `sys_vmo_write`; there is no kernel-side
  `sys_vmo_destroy` handler yet. `nexus-abi::vmo_destroy()` currently targets a placeholder syscall ID
  and returns a deterministic syscall error until kernel support lands.
- New crate `userspace/memory` (`package: nexus-vmo`) now provides typed VMO plumbing with bounded host
  mapping, deny-by-default transfer authorization, explicit reject-path tests, and deterministic counters
  (`copy_fallback_count`, `control_plane_bytes`, `bulk_bytes`, `map_reuse_hits/misses`).
- Selftest now emits the VMO proof ladder in the canonical OS run:
  - `vmo: producer sent handle`
  - `vmo: consumer mapped ok`
  - `vmo: sha256 ok`
  - `SELFTEST: vmo share ok`
- OS proof now uses a dedicated spawned consumer task: producer transfers the VMO into a fixed
  destination slot (`cap_transfer_to_slot`), consumer maps RO in its own task context, verifies payload
  bytes deterministically, and exits with status `0` for producer-side closure.
- Host API surface now includes `Vmo::from_bytes`, `Vmo::from_file_range` (host-only), and bounded
  `Vmo::slice` views (`VmoSlice`) with reject-path coverage.
- `docs/storage/vmo.md` now documents v1 plumbing scope, honesty constraints, and proof commands.

This task is therefore positioned as **In Review** for the v1 plumbing slice: host + OS proofs are green,
and kernel production closure items are explicitly handed off to `TASK-0290`.

## Goal

Provide a userspace “VMO handle” abstraction that:

- can represent large read-only buffers,
- can be mapped in-process for streaming hash/verify without extra copies,
- can be transferred to another process **if the kernel capability model supports it**,
- is bounded and testable on host and OS.

This task also establishes the early **zero-copy honesty contract**:

- distinguish “VMO/filebuffer capable” from “VMO/filebuffer actually used on the hot path”,
- expose bounded evidence for copy fallback and mapping reuse,
- and give later UI/media/platform tasks a measurable bulk-path baseline rather than a slogan.

## Non-Goals

- Full “VFS splice → VMO” (requires writable VFS + provider hooks; separate task once VFS/statefs exist).
- DSoftBus mux VMO frames (separate task on top of the existing mux v2 baseline).
- Kernel changes (this task must only use existing syscalls/capabilities).

## Constraints / invariants (hard requirements)

- Kernel untouched.
- No fake success: “zero-copy” markers only after verifying a consumer mapped/consumed the shared VMO.
- Bounded memory: cap max VMO length per operation; cap number of live VMOs in registries.
- Measurement posture:
  - bulk helpers must be able to report copy fallback count and bytes moved via control plane vs data plane,
  - repeated use should surface mapping reuse vs fresh map/unmap churn deterministically in host proofs.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **RED (lifecycle closure gap)**:
  - `nexus-abi::vmo_destroy()` exists, but kernel-side destroy handling is not wired yet.
  - Without explicit closure, long-lived services risk accumulating stale VMO handles/mappings.
- **YELLOW (cross-process proof gap)**:
  - The current two-process proof uses deterministic byte-match closure in the consumer + producer-side
    digest fixture check; explicit digest-by-reply IPC is still a potential hardening enhancement.
- **YELLOW (read-only sealing semantics)**:
  - v1 has no kernel-enforced VMO seal bit. RO behavior is currently a mapping/discipline contract, not an
    immutable kernel boundary.

## Security considerations

This task crosses a trust boundary (capability transfer between processes). Any ambiguity here tends to cause
either security bugs (cap leaks, write mappings) or “fake zero-copy” claims. Treat this as security-critical.

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

## Production-grade gate note

This task establishes the **plumbing + honesty floor** for zero-copy, but it does not by itself close
the kernel-side production-grade gap.

- `TASK-0290` is the closeout step for kernel-enforced sealing rights, write-map denial, and reuse/copy-fallback truth.
- UI and media consumers may cite this task as the baseline, but they should cite `TASK-0290` for a production-grade kernel data-plane claim.

### Mitigations

- VMO capabilities are kernel-managed (unforgeable handles).
- Transfers require explicit `cap_transfer` / `cap_transfer_to_slot` with subset-rights semantics.
- v1 caller policy maps shared buffers as RO (`READ|USER`) and treats writable mappings as explicit violations.
- Size bounds are checked before map/use and must reject oversized mapping requests.
- Documentation explicitly states v1 sealing is convention-level until `TASK-0290` kernel closure.

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
- Fixed fixtures prove stable counters for:
  - copy fallback count,
  - control-plane bytes vs bulk bytes,
  - and mapping reuse hit/miss behavior.

### Proof (OS / QEMU) — required if transfer is feasible today

Add a minimal two-process proof:

- producer allocates VMO, writes payload, seals RO (as defined), transfers handle to consumer,
- consumer maps VMO read-only and validates mapped bytes against the deterministic payload fixture,
- consumer returns bounded success/fail status; producer emits `vmo: sha256 ok` only after deterministic
  digest fixture check plus consumer success.

Markers (order tolerant):

- `vmo: producer sent handle`
- `vmo: consumer mapped ok`
- `vmo: sha256 ok`
- `SELFTEST: vmo share ok`

Notes:

- Postflight scripts must delegate to canonical harness/tests; no independent “log greps = success”.

### Out-of-scope handoff (must be explicit)

The following items are **not** in `TASK-0031` closure scope and must be tracked in `TASK-0290`:

- kernel-enforced seal/right semantics,
- write-map denial as a kernel authority guarantee,
- lifecycle closure for kernel destroy path (`vmo_destroy` wiring),
- production-grade Gate A/Gate C closure evidence for reuse/copy-fallback truth.

These handoff items do not block `TASK-0031` review closure once the host and OS plumbing proofs above are green.

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
     - receives a VMO handle via slot-directed transfer,
     - maps RO and validates deterministic payload bytes,
     - reports deterministic success/fail to producer.
   - Add deterministic markers listed above.

4. **Docs**
   - `docs/storage/vmo.md`: what a VMO is in this system, RO sealing semantics, limits, how to test.

## Phase plan (early architecture/perf posture)

### Phase A — Honest bulk contract floor

- land VMO abstraction + transfer/map proof,
- define deterministic copy-fallback and data-plane/control-plane counters,
- document what counts as a real zero-copy consumer.

### Phase B — Reuse-oriented bulk path

- expose mapping-reuse signals for repeated-use consumers,
- make host proofs detect map/unmap churn regressions,
- feed later UI/media/platform tasks with a measurable baseline rather than re-defining zero-copy ad hoc.

## Follow-ups (separate tasks)

See `follow-up-tasks` in the header.
