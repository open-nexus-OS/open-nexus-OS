---
title: TASK-0179 updated v2: component apply engine + path-based staging + offline feed — owner of the first real OTA flip (crown proof)
status: Draft
owner: @runtime
created: 2025-12-27
updated: 2026-08-25
depends-on:
  - TASK-0198   # Phase 1: device trust anchor + verifier verdict authority
  - TASK-0289   # Phase A: nxboot loader + boot flip
  - TASK-0036   # Phase B: BSB projection (and Phase A record v3 fields)
  - TASK-0260   # nx image emits the OTA container fixtures
follow-up-tasks:
  - TASK-0140
  - TASK-0034
links:
  - Contract: docs/rfcs/RFC-0089-ota-v2-component-manifest-ab-boot-images-nxboot-bsb.md (§3 manifest, §8 updated v2, §9 feed, §10 anti-downgrade)
  - Authority: docs/adr/0055-bootctld-single-boot-state-authority.md (updated stays a client)
  - Staging placement: docs/adr/0043-user-data-in-dedicated-cow-fs-statefs-stays-service-kv.md (/data, never /state KV)
  - Block substrate: tasks/TASK-0315-block-topology-consolidation-gpt-virtioblkd-sole-owner.md
  - Testing contract: scripts/qemu-test.sh
---

## REWRITE 2026-08-25 (RFC-0089 lane recut — supersedes the whole pre-rewrite body)

The 2025-12-27 body predated bootctld and carried `bootctld.setActive`/marker-only
`reboot()`/"minimal healthd"/soft-reboot-simulation/`state:/slots/<a|b>/root.img`
— all dead: bootctld is real with real SBI reset (TASK-0050), health quorum +
deadline live in the bootctld machine (TASK-0036), recovery/boot semantics are
RFC-0087 §4, slot payloads are GPT partitions (RFC-0089 §2), and bulk bytes never
land in the `/state` KV (ADR-0043 — the old `state:/slots/` path was a violation
on paper). The TASK-0178 link is gone: 0178 is Superseded by TASK-0050. The old
"NUB"/second-payload-format warning is resolved structurally — the ONLY container
is `.nxs` v2.

## Context

After TASK-0289-A the system boots from disk through `nxboot`, and slots are real
partitions — but nothing fills them: `updated` still carries the v1 8-KiB inline
`OP_STAGE` that verifies bytes in RAM and discards them. This task replaces that
with the end-state apply engine and delivers the lane's crown proof: the first
update that actually changes what the machine boots.

## Goal

`updated` v2 (service name unchanged; stays a bootctld CLIENT):

1. **`.nxs` v2 component-manifest verify** in `userspace/updates` (evolves
   `system_set.rs`; schema SSOT `tools/nexus-idl/schemas/system-set.capnp`,
   schemaVersion 2). Verification order per RFC-0089 §3: device-anchor signature
   (Phase-1 anchor from TASK-0198) → stage-time anti-downgrade → streamed
   per-component sha256 → kind checks. v1 dispatch table has exactly ONE kind:
   `boot-image`; unknown kinds reject deterministically.
2. **Path-based staging**: `OP_STAGE_SOURCE { path }` replaces inline `OP_STAGE`
   (removed in the same change — no dual API; nexus-abi/wire updates are
   approval-zone). Streams from `/data/updates/*.nxs` or `pkg://updates/` in
   64-KiB bounded chunks.
3. **Apply pipeline** (per component): stream-digest → write to the INACTIVE slot
   via partition-scoped block IPC (write access: inactive slot only) → readback
   verify → write NXBD LAST (verbatim from the container — never re-signed) →
   `bootctld OP_STAGE`; switch on request via `OP_SWITCH(tries=2)`.
   Idempotent: restage converges; torn stage leaves the slot NXBD-invalid.
4. **Anti-downgrade, stage-time + commit-time**: reject
   `manifest.rollbackIndex < floor` at stage; on health commit bootctld raises
   `rollback_min_index` to the committed NXBD's index and projects to BSB
   (`bootctld: rollback-min raised (<old>-><new>)`) — the machine-side raise is
   implemented HERE (the v3 field exists since TASK-0036-A).
5. **Offline feed v1**: `OP_FEED_LIST`/`OP_CHECK` enumerate `/data/updates/` +
   `pkg://updates/` fixtures deterministically. Network = later phase per
   RFC-0089 §9; nothing here changes for it.
6. **Fixture builds**: `nx image` (TASK-0260) emits `build/ota/os-<buildid>.nxs`
   for the current build plus a second fixture with a DIFFERENT build id (and one
   with a lower rollback index for the downgrade lane) so QEMU stages a genuinely
   different image.

## Non-Goals

- Loader/boot-time backstop proofs and the measured surface (TASK-0289-B).
- Delta kinds (TASK-0034/0035 — the dispatch table is the seam).
- UI/CLI (TASK-0140). Network transport. Key rotation (TASK-0197/0198 later).
- Kernel changes beyond the approval-zone wire additions named above.

## Constraints / invariants (hard requirements)

- Fail-closed with stable reject reasons (RFC-0089 §8):
  `untrusted publisher | sig | digest | bounds | path | component kind
  unsupported | downgrade | io | slot-active`; every reason has a
  `test_reject_*` and an audit record (scope=`updated`, never key material).
- The active slot is never writable through this path (`slot-active` reject).
- Bounded memory: streaming only, no whole-container buffering (the bump-
  allocator services never see multi-MB allocations).
- Markers only after real behavior; `updated: stage done` requires readback.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/updates_host/` extensions (existing negative tests stay green):

- Full apply pipeline against an in-memory blockproto fake:
  stage → verify → write → readback → NXBD-last → bootctld stage.
- Power-cut matrix: kill after EACH pipeline step ⇒ restage idempotent, slot
  never half-valid (NXBD absent until final step).
- `test_reject_*` for every stable reason incl. downgrade and unknown kind.
- Feed enumeration determinism; manifest v2 accept/tamper vectors.
- Floor-raise on commit (machine test, injected clock).

### Proof (OS / QEMU) — new profile `ota` (two boots, ONE uart stream) + keep-blk lane

**Crown proof** (`just test-os ota`): boot 1 —
`updated: stage begin (source=/data/updates/os-B.nxs)` →
`updated: component boot-image verified (sha=<8>)` →
`updated: stage done (slot=b build=B)` → `bootctld: switch scheduled (to=b)` →
`bootctld: bsb sync (… next=b tries=2)` → `SELFTEST: ota stage ok` → SBI reset.
Boot 2 — `nxboot: tries 2->1 (slot=b trial)` →
`nxboot: verify ok (slot=b build=B rbidx=<n>)` → `nxboot: jump slot=b` →
`init: build-id B` → quorum → `bootctld: commit ok (slot=b)` →
`bootctld: rollback-min raised (…)` → `SELFTEST: ota flip ok`.

Adversarial lanes owned here:

- Tamper (stage-time): bit-flipped fixture ⇒ `updated: stage rejected (digest)` +
  `SELFTEST: ota tamper deny ok`.
- Downgrade (stage-time): low-index fixture ⇒ `updated: stage rejected (downgrade)`
  + `SELFTEST: ota downgrade deny ok` (stage rung; loader backstop rung is 0289-B).
- Power-cut-during-stage (keep-blk): run 1 killed at `updated: staging` ⇒ run 2
  boots slot A, `updated: restage clean`, `SELFTEST: ota stage resume ok`.

Regression signal: headless OTA rungs recut in the same change (old inline-stage
markers retired from `scripts/qemu-test.sh` + markers.txt + proof-manifest
together); `ci-os-reset` stays untouched-green; new `just ci-os-ota` recipe.

## Touched paths (allowlist)

- `source/services/updated/` + `userspace/updates/`
- `source/services/bootctld/` (floor raise on commit)
- `source/libs/nexus-abi` + `source/libs/nexus-wire` (OP_STAGE_SOURCE — approval)
- `tools/nexus-idl/schemas/system-set.capnp` + `tools/nx` (fixture emission)
- `source/apps/selftest-client/` + `proof-manifest/markers/`
- `tests/updates_host/`, `scripts/qemu-test.sh` + `justfile` (ota profile — approval)
- `docs/packaging/system-set.md`, `docs/updates/` sweep

## Plan (small PRs)

1. Manifest v2 schema + verifier in `userspace/updates` + host vectors.
2. `OP_STAGE_SOURCE` wire + streaming reader (vfsd/pkg) + bounds; retire inline
   OP_STAGE end-to-end.
3. Apply pipeline vs. block fake + power-cut matrix (host-complete before OS).
4. OS wiring: partition write route + policy caps; fixture emission via nx image.
5. `ota` profile + crown proof + adversarial lanes; floor raise; docs + boards.
