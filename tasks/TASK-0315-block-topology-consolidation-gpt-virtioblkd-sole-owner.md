---
title: TASK-0315 Block topology consolidation: one GPT device + virtioblkd as sole queue owner (ADR-0044 end state, staging retired)
status: Done
completed: 2026-08-25
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

## DELIVERED 2026-08-25 (test-all pending final gate; OTA-lane package 5)

Evidence (headless + keep-blk double boot + `ci-os-reset` green; uarts
2026-08-25T18-28/18-32):

- **ONE GPT disk**: the launcher boots `build/nexus.img` (built/patched by
  `nx image` — `NEXUS_KEEP_BLK=1` keeps state/data and refreshes boot-a
  via `nx image patch`, the end-state flasher shape). The two-device
  blk/data reverse-enumeration swap is structurally dead.
- **virtioblkd real** (`os_lite.rs` + `route_os.rs`): sole MMIO owner,
  one RO CRC-validated GPT parse (`virtioblkd: gpt ok (parts=7)`),
  blockproto server with kernel-attributed per-partition gates
  (statefsd → `state` rw, vfsd → `data` rw, everything else DENIED —
  `SELFTEST: blk cross-partition deny ok` gated every boot; boot/bsb/
  system selectors deny until 0179/0036-B join). ZERO-allocation serve
  path (fixed buffers — the bump heap never frees; the Vec-per-request
  first cut died of `alloc-fail` mid-ladder, recorded below).
- **IRQ completion LIVE**: init wires a dedicated notify endpoint at the
  fixed slot 0xF1; the TASK-0314 machinery binds it —
  `virtioblkd: irq endpoint bound` + `blk: irq completion on` are gated
  required markers now (the 0314 poll-fallback marker recut as planned).
- **Clients demoted to least privilege**: statefsd's `Backend::Virtio`
  (direct MMIO, slot 48) is DELETED — `Backend::Remote(RemoteBlockDevice)`
  attaches the state partition over IPC inside the same pristine upgrade
  window (`statefsd: virtio upgrade ok`, geometry unchanged: ss=512
  nsec=131072); nxfsd's DataStore likewise (`/data` = partition). The
  double slot-48 grant and the vfsd slot-49 grant are gone.
- **Wiring (the hard-won part)**: blockproto frames carry magic+nonce
  (shared reply inboxes carry policyd traffic too — replies must be
  self-identifying); clients get FIXED slots 0xF0..0xF2 transferred in the
  SPAWN-TIME distribution pass (`init: blk plane wired svc=…`) — a
  route-get-based attach loses the race against the first mutating
  statefs op and permanently closes the pristine window; the first attach
  BLOCKS bounded (8 s) exactly like the old inline device init, so the
  window can never lose to virtioblkd still bringing the device up.
- `blockproto` gained the RFC-0089 selectors (bsb/boot-a/b/system-a/b),
  `STATUS_DENIED`, and a no-alloc `_into` encoder family.

### Findings recorded (each cost one QEMU round)

1. `nx` must be built with a CLEAN env in the launcher — the OS
   `RUSTFLAGS` (nexus_env=os) on a host target breaks nexus-abi.
2. Kernel `cap_query` reports only Vmo/DeviceMmio — endpoint-presence
   gates via `slot_probe::slot_is_ipc_endpoint` are structurally FALSE
   (nexus-abi's KIND_IPC_ENDPOINT=3 is aspirational); removed.
3. Inserting transfers mid-arm in the selftest wiring SHIFTS its
   historically fixed slot numbers (0x11/0x12 keystored, 0x17/0x18 reply)
   and breaks every hardcoded probe — new selftest routes must be a
   separate post-wiring pass.
4. Per-request `Vec`s in an OS service are fatal, not slow (bump heap;
   `alloc-fail svc=virtioblkd` → service death → cascade) — the
   documented allocator trap, now enforced by the `_into` codec family.

## Rebase note 2026-08-25 (RFC-0089: pulled into the OTA lane; layout extended)

This task is now an early package of the Updates/OTA lane (user decision
2026-08-25) and executes the FULL RFC-0089 §2 layout, not just `state`+`data`:
`bsb | boot-a | boot-b | system-a | system-b | state | data` on ONE GPT disk
(`build/nexus.img`), produced host-side by `nx image build` (TASK-0260 — the
launcher consumes it instead of preparing images itself; `NEXUS_KEEP_BLK=1`
keeps the disk and refreshes boot partitions via `nx image patch`). Additional
scope riders: per-partition ACCESS policy includes the OTA roles (updated →
inactive boot slot write only, bootctld → `bsb`, everything else denied —
`SELFTEST: blk cross-partition deny ok` covers a slot-write deny too); the GUID
table is exported from `userspace/storage` as the ONE layout authority (shared
with `nx image` and later `nxboot`). Boot stays via the VMM kernel option in
this task — the loader flip is TASK-0289-A (separation keeps every package
green). The two-device blk/data reverse-enumeration swap finding dies here.
depends-on additions at execution: TASK-0260.

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
