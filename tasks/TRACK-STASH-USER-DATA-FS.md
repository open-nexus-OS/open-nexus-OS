---
title: TRACK Stash + user-data filesystem: production-grade storage ladder (vfs v2 → nxfs → zero-copy → CoW/encryption)
status: Living
owner: @runtime
created: 2026-07-15
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Contract seeds: docs/rfcs/RFC-0071-nxfs-user-data-filesystem-contract.md, docs/rfcs/RFC-0072-vfs-v2-writable-providers-readdir-stable-errors.md, docs/rfcs/RFC-0073-app-files-surface-svc-files-permission-filemanager-role.md
  - Decisions: docs/adr/0043-user-data-in-dedicated-cow-fs-statefs-stays-service-kv.md, docs/adr/0044-single-blk-device-gpt-partitions-block-layer.md
  - statefs hardening (parallel lane): tasks/TASK-0025-statefs-write-path-hardening-integrity-atomic-budgets-audit.md, tasks/TASK-0026-statefs-v2a-2pc-compaction-fsck.md, tasks/TASK-0027-statefs-v2b-encryption-at-rest.md
  - Role model: docs/dev/app-platform/privileged-roles.md
  - Related track: tasks/TRACK-REMOVABLE-STORAGE.md
---

## Goal (track-level)

End state: an **Apple-grade user-data storage stack** — crash-atomic, integrity-checked,
snapshot-capable, encrypted-by-class, zero-copy on the bulk path — with **stash** as the working,
privileged file manager on top. Staged so every rung is boot-proven before the next starts, and the
first user-visible win (real listing) lands before any new storage engine exists.

## The milestone ladder

| # | Milestone | Task | Proof headline |
|---|---|---|---|
| 1 | ✅ **stash lists real content** — vfs ReadDir + stable errors + `svc.files` + FILES permission + `filemanager` role; `/packages` RO listing | `TASK-0291` (Done, boot-proven) | `stash: listing real (n=<n>)` + screenshot |
| 2 | ✅ **nxfs engine exists (host)** — RFC-0071 P1 format/txn/replay/fsck, crash-injection determinism | `TASK-0292` (In Review, host-proven) | `cargo test -p nxfs` (17 unit + 5 crash-injection) |
| 3 | ✅ **stash writes + persists** — v1 staging: 2nd virtio-blk device + nxfs `/data` hosted in-process by vfsd (`nxfsd::DataStore`); write path AND keep-blk cold-boot persistence boot-proven (ADR-0043/0044 amended). Fixed a mount deadlock (full-journal read on 2nd device) via incremental journal read | `TASK-0293` (In Review) | `nxfsd: mounted /data (rw, clean)` · `files.mkdir ok` + folder visible · cold-boot → folder persists (n=1, screenshot) |
| 4 | ✅ **file-type icons** — mime SSOT (`resources/mimetypes/mimetypes.toml`) + `nexus-mime-icons` bake + `Image { source: "mime:…" }`; app-host emits `mime:<stem>` per entry, stash `FileRow` renders it; `/data` seeded with varied first-run files | `TASK-0294` (In Review, boot-proven) | `stash: mime icons resolved (n=9)` + screenshot (distinct PDF/ZIP/PNG/MP3/TXT/MD/JSON/folder icons) |
| 5 | ✅ **zero-copy bulk path** — `OP_READ_VMO` cross-process CAP_MOVE splice at the vfsd seam (pkg + nxfs `/data`), header-last fill, inline cap (`E2BIG`) enforced; VMO-backed writes deferred | `TASK-0295` (In Review, boot-proven) | `vfsd: vmo splice read ok (bytes=19, fallbacks=0)` + `SELFTEST: vfs splice roundtrip ok` + `SELFTEST: vfs inline oversize deny ok` |
| 6 | **CoW + snapshots/clones** (RFC-0071 P3) | seeded after #3 proves | (task defines) |
| 7 | **encryption classes** (RFC-0071 P4; absorbs old TASK-0182/0183 UX pieces) | seeded after #6 proves | `nxfsd: encryption on (device-class)` |

Milestones 6/7 are deliberately **not pre-seeded as tasks** (seed-when-ready rule); their contracts
are already fixed in RFC-0071 so nothing drifts while they wait.

## Parallel lane: statefs hardening (NOT this ladder's critical path)

statefs stays the boot-critical service-state KV (ADR-0043) and hardens independently:
`TASK-0025` (authenticity envelopes + anti-rollback + budgets) → `TASK-0026` (2PC + compaction +
fsck — patterns shared with nxfs P1) → `TASK-0027` (record encryption for non-boot-critical
prefixes, reusing the RFC-0071 key hierarchy). `TASK-0026`'s cold-boot proof reuses milestone 3's
`NEXUS_KEEP_BLK` harness.

## Contracts (stable interfaces to design around)

- **One authority per store** (ADR-0043): `/state` = statefsd, `/packages` = packagefsd,
  `/data` = nxfsd; vfsd is the single client-facing surface.
- **Block topology** (ADR-0044): one virtio-blk device, GPT, `virtioblkd` as the only queue owner,
  partition-scoped block IO to storage services.
- **Error semantics**: the RFC-0072 stable code table — every storage error everywhere maps into
  it; higher layers (`TASK-0132` remainder) adopt it, never fork it.
- **App access**: `svc.files.*` behind `nexus.permission.FILES`, ceiling-gated to
  `bundle_type = filemanager` (stash); sandboxed apps wait for pickers (`TASK-0083`/`TASK-0084`,
  deferred — see also TRACK-REMOVABLE-STORAGE's content-provider model for that future seam).
- **Data plane**: control = Cap'n Proto, bulk = VMO handles (`INLINE_IO_MAX = 4096` from RFC-0071).
- **Mime SSOT**: `resources/mimetypes/mimetypes.toml` (extension → mime → icon stem + fallback
  chain); consumed by the files service and the icon bake, never duplicated.

## Superseded / absorbed by this track

- `TASK-0033` → absorbed into `TASK-0295` (zero-copy moves to the vfsd seam).
- `TASK-0132` → vfs error-code slice absorbed into RFC-0072/`TASK-0291`; higher-layer remainder stays.
- `TASK-0134` → user-data snapshots move to RFC-0071 P3; statefs-side remainder stays.
- `TASK-0182`/`TASK-0183` → superseded by RFC-0071 P4 (no securefsd overlay).

## Non-Goals

- This file is **not** an implementation task; every claim needs its task's proof.
- No POSIX-completeness promises; no removable media (own track); no kernel changes defined here.

## Gates (RED / YELLOW / GREEN)

- **RED**: statefsd's block-ownership move (milestone 3) regresses any existing persist/boot
  marker → stop, staged fallback path stays until green.
- **RED**: any milestone claims success without its deterministic marker/screenshot evidence.
- **YELLOW**: quotas for `/data` (TASK-0133 model) and memory-pressure honesty (TASK-0286/0287)
  are tracked dependencies for calling the stack "production-grade", not for the ladder itself.
- **GREEN**: each milestone's marker set green on host + QEMU before the next starts.
