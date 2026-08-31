---
title: TASK-0179 updated v2: component apply engine + path-based staging + offline feed — owner of the first real OTA flip (crown proof)
status: Done — 2026-08-31 (apply engine v2 + crown proof: the first update that changes what the machine boots)
owner: @runtime
created: 2025-12-27
updated: 2026-08-31
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

## DELIVERED 2026-08-31

**Crown proof green** (`just ci-os-ota`, two boots in ONE uart):
`nxboot: verify ok (slot=a build=dev-<id> rbidx=1)` → `bootctld: rollback-min
adopted from factory bsb` → `updated: stage done (slot=b build=otaB<id>)` →
`SELFTEST: ota flip staged ok` → SBI reset → `nxboot: tries 2->1 (slot=b
trial)` → `nxboot: verify ok (slot=b build=otaB<id> rbidx=2)` (DIFFERENT
build, above the floor) → `nxboot: jump slot=b` → `bootctld: health quorum ok
(2/2)` → `bootctld: commit ok (slot=b)` → `bootctld: rollback-min raised
(1->2)` → `SELFTEST: ota flip ok`.

Shipped: `.nxs` v2 verify+apply core (`updates::component_set`, 10 host tests
incl. the power-cut matrix), `OP_STAGE_SOURCE` path staging (inline stage
RETIRED), the slot sink (stream → digest → readback → NXBD last), record v4
(`staged_rollback_index`) + commit-time floor raise, `nx image fixtures`
(5 containers, self-verified against the device anchor at build time),
partition gates, deny lanes, and the `ota-flip` lane.

### Findings — real defects this package exposed (all fixed)

1. **nxfs could not hold a container**: 4 MiB cap and whole-file
   materialisation on EVERY read and write. Recut to extent streaming
   (`nxfs::stream` + `read_into`/`splice_window`, host-tested to 19 MB);
   cap 64 MiB. This retires the whole-file CoW debt on the read path.
2. **vfsd died on the first large splice**: the 64-KiB window was allocated
   PER REQUEST on a bump heap that never frees, i.e. one file's worth of
   arena. Now one reusable window for the service lifetime; `read_into` is
   allocation-free end to end.
3. **keystored advertised 1 MiB verify payload but could receive 512 bytes**
   (the shared allocating recv path). Every signature over a >408-byte
   message failed as "malformed". The declared bound is now TRUE and pinned
   to the kernel's own per-message maximum; the 512-byte trap is named at
   its source in `nexus-ipc`.
4. **Verify-error taxonomy collapsed to `sig`**: transport/backend failures
   were reported as bad signatures — blaming the publisher for a broken
   hop. Split into `sig` / `io` / `untrusted publisher`, backend detail
   named, plus a keystored-vs-local cross-check and a verifier known-answer
   test (a verifier that always rejects would make every deny lane look
   green).
5. **The harness split slowly-emitted uart lines**: `read -t` leaves a
   PARTIAL line in the variable on timeout and it was echoed as a complete
   one. Two proofs were reported missing while the guest had printed them
   correctly (`tatefsd:`, `nxboot: verify ok`) — both previously dismissed
   as flakes. Fragments are now accumulated.
6. **Anti-downgrade was vacuous on fresh devices**: the factory wrote floor 0
   while shipping an image at index 0. Floor now equals the shipped index,
   and the record ADOPTS it at genesis (a fresh record starting at 0 would
   otherwise project over the factory value).
7. **`GET_STATUS` used an exact length check** on a payload that grew twice
   by contract, so it returned FAILED — and because every caller wraps it in
   `if let Ok(..)`, slot normalisation silently stopped working. Both sides
   now check a prefix.
8. **`extends` did not inherit marker expectations** (exact name match), so
   every derived lane saw its parent's legitimate markers as "unexpected".
   Expectation matching now walks the chain.
9. The launcher rebuilt `nx` only when the binary was ABSENT, shipping a
   stale image assembler for a whole run.

### Deliberate scope notes

- The small verify-path fixtures carry an INVALID NXBD `load_addr`: they land
  in a real slot partition, so if a lane ever left one selected the loader
  must refuse it loudly instead of jumping into pattern bytes. Only `os-B`
  carries the real entry address.
- The crown lane runs the reduced bringup+end scope. Re-running the full ota
  phase on an already-committed slot exercises the machine cycle against
  state it was not written for; the headless lane owns that cycle.
- The container is mapped as ONE read-only VMO (bounded by an explicit
  32 MiB cap) rather than copied in chunks: vfsd streams 64-KiB windows INTO
  it, and the digest/apply loop walks it in 64-KiB units, so no actor holds
  the container on a heap.

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
   approval-zone). Streams from the data volume's `/updates/*.nxs` (the volume is mounted at
   the VFS root — `/data` is the PARTITION name, not a path prefix) or
   `pkg://updates/`, in 64-KiB bounded chunks.
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
5. **Offline feed v1**: `OP_FEED_LIST`/`OP_CHECK` enumerate `/updates/` +
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
`updated: stage begin (source=/updates/os-B.nxs)` →
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
