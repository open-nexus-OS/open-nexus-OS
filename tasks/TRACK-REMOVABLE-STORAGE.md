---
title: TRACK Removable Storage (USB/SD/external disks): contracts + gated roadmap for content providers + SAF + formatting
status: Living
owner: @runtime @platform
created: 2026-01-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Policy flow: docs/architecture/11-policyd-and-policy-flow.md
  - Content providers (pathless streams): tasks/TASK-0081-ui-v11a-mime-registry-content-providers.md
  - Doc picker (SAF flows): tasks/TASK-0083-ui-v11c-document-picker-open-save-openwith.md
  - Scoped grants (persistable): tasks/TASK-0084-ui-v12a-scoped-uri-grants.md
  - FileOps/Trash backbone: tasks/TASK-0085-ui-v12b-fileops-trash-services.md
  - Files/SAF polish + privacy gates: tasks/TASK-0233-content-v1_2b-os-saf-flows-files-polish-privacy-gates-selftests.md
  - Persistence (/state authority): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Device access model (MMIO caps): tasks/TASK-0010-device-mmio-access-model.md
  - DriverKit ABI policy: docs/adr/0018-driverkit-abi-versioning-and-stability.md
---

## Goal (track-level)

Make removable media (USB sticks, SD cards, external disks) **usable** for end-users (open/copy/move/save/format)
while remaining **sandboxed and auditable**:

- apps never receive filesystem paths or raw block access,
- access is mediated via `content://` providers + scoped grants (SAF flows),
- privileged operations (format, mount/eject, provider enable) are deny-by-default via `policyd`,
- everything is bounded and deterministic (proof via host tests and/or QEMU markers once meaningful).

## Non-Goals

- This file is **not** an implementation task.
- No “global POSIX mount in vfsd” as the primary app-facing mechanism.
- No kernel changes are defined here (kernel/device work stays in its own tasks).
- No attempt to ship a Linux USB/storage stack as-is; we build device-class services in userspace.

## Contracts (stable interfaces to design around)

- **App-facing document model**: `content://...` URIs + stream handles only (no paths).
  Sources: `TASK-0081`, `TASK-0083`.
- **Sandbox crossing**: `grantsd` issues scoped grants; `contentd` enforces at `openUri` boundaries.
  Source: `TASK-0084`.
- **Cross-provider operations**: copy/move are performed by `fileopsd` via streams; apps do not bulk-copy by raw paths.
  Source: `TASK-0085`.
- **Policy authority**: `policyd` is single authority; identity binds to kernel `sender_service_id`.
  Source: `docs/architecture/11-policyd-and-policy-flow.md`.
- **Persistence for remember-access / metadata**: persistable grants and storage-related metadata live under `/state` via the `/state` authority plan.
  Source: `TASK-0009`.
- **Device-class boundary**: any real userspace block/USB frontend relies on capability-gated device access (MMIO caps now; IRQ/DMA later).
  Source: `TASK-0010` (and DriverKit ABI policy `ADR-0018`).

Clarifications (to avoid drift):

- `/state` is a **logical namespace** served by a `/state` authority (v1: `statefsd`), not a global POSIX mount in `vfsd`.
  This keeps removable storage metadata/grants consistent with `TASK-0009`’s v1 scope.
- “Device access model” means the v1 foundation: init-controlled capability distribution + `policyd` as authority (deny-by-default),
  not kernel name/string checks as a long-term mechanism (see `TASK-0010` normative contract).

## Gates (RED / YELLOW / GREEN)

- **RED (blocking)**:
  - “Real” userspace device frontends require a real device access model (MMIO caps now; IRQ/DMA isolation later).
    Gate: `TASK-0010` (and follow-ups for IRQ/DMA when needed).
  - Persistence-backed “remember access” (persistable grants) is not real until `/state` exists.
    Gate: `TASK-0009` (which is itself gated on `TASK-0010`).
  - OS build hygiene: removable FS implementations must not pull forbidden crates into OS graphs (`parking_lot`, `getrandom`, `std`).
    Gate: `docs/standards/BUILD_STANDARDS.md` + `just dep-gate`.
- **YELLOW (risky / drift-prone)**:
  - ext4 complexity (parser surface, recovery/journal semantics) and “mkfs ext4” scope.
  - encryption-at-rest (“with password”) must not become a second keystore; password handling must route through keystore/policy/consent.
  - QEMU may not model real USB mass storage reliably; proofs should start with virtio-blk style fixtures where possible.
- **GREEN (confirmed direction)**:
  - Removable storage is exposed as a `contentd` provider (“removable://” or equivalent provider id), not as raw paths.
  - SAF/Doc Picker + scoped grants are the canonical UX/security boundary for opening/saving and folder access.

## Phase map

- **Phase 0 (provider + UX contract, no new filesystems yet)**:
  - Define removable provider semantics at the content layer (listing/stat/open/create/remove/rename via streams).
  - SAF flows can browse removable provider and issue scoped grants (folder grants included).
  - FileOps can copy/move between `state://` and removable via streams.
  - Proof: host tests for provider invariants + grants + fileops (no fake success markers).
-  Notes:
   - Until `TASK-0009` exists, persistable grants must be treated as **non-persistent** (memory-only) and labeled honestly
     (no “persist ok” markers).
- **Phase 1 (FAT32 v1)**:
  - Userspace FAT32 filesystem authority behind the removable provider (read/write).
  - Format tool supports **FAT32 format (no password)** with explicit user intent and policy gating.
  - Proof: `test_reject_*` for malformed/oversized metadata; deterministic read/write fixtures.
- **Phase 2 (ext4 v1 – staged)**:
  - ext4 read-only first (optional), then ext4 read/write.
  - Format tool supports ext4 format only after stable semantics and bounds are proven.
- **Phase 3 (encryption-at-rest / “with password”)**:
  - Introduce a dedicated encryption-at-rest layer (or statefs-first, per follow-up `TASK-0027` direction).
  - “Password” is a UX concept; secrets and derived keys are handled by keystored + policy/consent flows.

## Backlog (Candidate Subtasks)

These are *not* tasks yet; they become real `TASK-XXXX` items only when they can be proven deterministically.

- **CAND-REM-001: Removable provider contract in contentd (host-first)**  
  - **What**: add provider id + semantics + deterministic fixtures (no FS impl required yet)  
  - **Depends on**: `TASK-0081`, `TASK-0084`  
  - **Proof idea**: host tests for listing order, bounded traversal, grant enforcement at open boundaries  
  - **Status**: candidate

- **CAND-REM-010: FAT32 filesystem authority (fatfsd) behind removable provider**  
  - **Depends on**: CAND-REM-001  
  - **Proof idea**: malformed FAT rejection tests + deterministic read/write roundtrip  
  - **Status**: candidate

- **CAND-REM-020: Formatting authority (formatd) for FAT32/ext4/statefs**  
  - **What**: explicit “intent token” + policy-gated destructive operations; emits audits  
  - **Depends on**: policy/consent wiring; keystore for “with password” phases  
  - **Proof idea**: deny-by-default markers; negative tests for unauthorized format  
  - **Status**: candidate

- **CAND-REM-030: USB mass storage device-class services (xHCI + MSC + SCSI)**  
  - **What**: true removable hardware path in userspace  
  - **Depends on**: device access model extensions (IRQ/DMA), DriverKit contracts  
  - **Proof idea**: staged bring-up with deterministic fixtures; avoid claiming “usb ok” without real device I/O  
  - **Status**: candidate

## Extraction rules

- Only extract a candidate into a real `TASK-XXXX` when it has:
  - deterministic proof (host tests and/or QEMU markers where meaningful),
  - explicit minimal-v1 vs future-deluxe boundaries,
  - and no contract drift (no new ad-hoc URI schemes or shadow policy/grants stores).
- After extraction, keep only a link and `Status: extracted → TASK-XXXX`.
