---
title: TASK-0315 Block topology consolidation: one GPT device + virtioblkd as sole queue owner (ADR-0044 end state, staging retired)
status: Draft
owner: @runtime
created: 2026-08-14
depends-on:
  - TASK-0314
follow-up-tasks:
  - TASK-0317
links:
  - Vision: docs/architecture/vision.md
  - Playbook: CLAUDE.md
  - Contract (this task executes it): docs/adr/0044-single-blk-device-gpt-partitions-block-layer.md
  - Store split: docs/adr/0043-user-data-in-dedicated-cow-fs-statefs-stays-service-kv.md
  - Substrate (built, unwired): userspace/storage/src/gpt.rs + userspace/storage/src/blockproto.rs
  - Ladder: tasks/TRACK-STASH-USER-DATA-FS.md
---

## Context

ADR-0044's end state is contracted but unowned: **one** virtio-blk device, GPT-partitioned
(`state` + `data`), `virtioblkd` as the **sole** MMIO + virtqueue owner serving partition-scoped
block IO over IPC. What ships today is the explicitly transitional 2026-07-15 staging:

- **Two devices** (`build/blk.img` for statefs, `build/data.img` for nxfs).
- `virtioblkd` is a **70-line proof stub** (maps the MMIO window, prints a marker, parks).
- statefsd still drives the device directly via MMIO cap slot 48 — and init grants that **same
  window to virtioblkd too** (`bootstrap/orchestrator.rs` + `helpers.rs:105`): two MAP holders
  on one device, violating the ADR's own one-owner rule.
- vfsd holds the second device's MMIO cap at slot 49 for the in-process DataStore.
- The GPT parser + `PartitionView` + `blockproto` codec are **already landed and host-tested**
  (`userspace/storage/`, 12 tests) — dead code in the OS image.

Per user direction 2026-08-14 the ladder builds the end architecture, not further staging.

## Goal

ADR-0044 decision items 1/2/3/5 executed literally:

- **One GPT device**: launcher prepares a GPT image host-side (`state` + `data` partitions);
  services never format partitions implicitly. `NEXUS_KEEP_BLK=1` keeps working.
- **virtioblkd real**: virtqueue driver (from TASK-0314) moved in, one-time RO CRC-validated GPT
  parse, `blockproto` server loop serving partition-scoped block IO; per-partition access checks
  (statefsd → `state` only, nxfs owner → `data` only), policyd-gated routes.
- **Clients demoted to least privilege**: statefsd and the nxfs store use `RemoteBlockDevice`
  (IPC) instead of device MMIO; the MMIO cap moves to virtioblkd alone; the duplicate slot-48
  grant is removed; **the direct-MMIO path in statefsd is deleted once the switch is
  boot-proven** (no permanent dual path — whole-device fallback only for the no-GPT dev case,
  as the ADR allows).
- Bulk sectors move via VMO where it pays (reusing the RFC-0040 transfer discipline); inline
  frames stay bounded.

## Non-Goals

- nxfsd process extraction (TASK-0317 — until then the `RemoteBlockDevice` client for `/data`
  lives where the DataStore lives).
- Driver-internal performance (TASK-0314 delivers it first).
- Removable media / hotplug (TRACK-REMOVABLE-STORAGE; this task creates the topology it needs).
- Journal byte-format changes (RFC-0018 and nxfs format untouched — a partition is just a base
  offset via `PartitionView`).

## Constraints / invariants (hard requirements)

- **Boot-critical blast radius is the RED gate**: keystored/updated/settingsd contract tests and
  the statefs persist markers must stay green through the statefsd switch; staged fallback stays
  until green, then is deleted in the same task (ADR rule: no permanent dual path).
- One virtqueue = one process. After this task, `grep` for device MMIO caps in
  statefsd/vfsd/nxfsd must come back empty (least-privilege proof).
- GPT parsing is read-only, bounded, CRC-validated, fail-closed (`userspace/storage` contract).
- Partition access is deny-by-default: a `test_reject_*` proves statefsd cannot read `data`
  sectors and the nxfs client cannot read `state` sectors.

## Stop conditions (Definition of Done)

### Proof (Host) — required

- `userspace/storage` tests extended: `RemoteBlockDevice` roundtrip over the blockproto codec,
  partition-scope rejection (`test_reject_cross_partition`), malformed-GPT fail-closed.

### Proof (OS / QEMU) — required

- `virtioblkd: gpt ok (parts=2)` + `virtioblkd: serving state,data`
- `statefsd: ready` + existing statefs persist markers over the IPC block path
- `nxfsd: mounted /data (rw, clean)` over the IPC block path
- `SELFTEST: blk cross-partition deny ok`
- Cold boot (`NEXUS_KEEP_BLK=1`): statefs + `/data` both persist across the GPT image.

## Touched paths (allowlist)

- `source/services/virtioblkd/`, `userspace/storage/`
- `source/services/statefsd/`, `source/services/nxfsd/`, `source/services/vfsd/`
- `source/init/nexus-init/` (cap grants, routes), `policies/base.toml`
- launcher/image prep under `scripts/` (GPT image creation), `source/apps/selftest-client/`,
  `scripts/qemu-test.sh`
- `docs/adr/0044-…` (staging amendment closed), `docs/storage/`
