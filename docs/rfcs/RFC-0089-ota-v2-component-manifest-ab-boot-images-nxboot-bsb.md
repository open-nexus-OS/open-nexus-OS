<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# RFC-0089: OTA v2 — Component Manifest (`.nxs` v2), A/B Boot Images, `nxboot` First-Stage Loader, Boot Selection Block

- Status: Draft (contract seed for the Updates/OTA lane)
- Owners: @runtime @security @tools-team
- Created: 2026-08-25
- Last Updated: 2026-08-25
- Links:
  - Tasks (execution + proof, in lane order):
    - `tasks/TASK-0198-supply-chain-v2b-os-enforcement-store-updater-bundlemgrd.md` (Phase 1: device trust anchor)
    - `tasks/TASK-0036-ota-ab-v2-userspace-healthmux-rollback-softreboot.md` (health-commit v2 + BSB projection)
    - `tasks/TASK-0314-virtio-blk-driver-v2-multisector-queue-irq.md` (block substrate)
    - `tasks/TASK-0260-provisioning-recovery-v1_0a-host-image-builder-flasher-protocol-deterministic.md` (`nx image`)
    - `tasks/TASK-0315-block-topology-consolidation-gpt-virtioblkd-sole-owner.md` (single GPT disk)
    - `tasks/TASK-0289-boot-trust-floor-v1-verified-boot-rollback-measurement.md` (`nxboot` + boot trust floor)
    - `tasks/TASK-0179-updated-v2-offline-feed-delta-health-rollback.md` (apply engine v2 + crown proof)
    - `tasks/TASK-0140-updates-v1-ui-cli-settings-offline.md` (UI/CLI surfaces)
    - `tasks/TASK-0034-delta-updates-v1-bundle-nxdelta.md` + `tasks/TASK-0035-delta-updates-v1b-system-set-nxs.md` (delta components)
  - ADRs:
    - `docs/adr/0058-boot-selection-block-dual-actor-discipline.md` (BSB write matrix)
    - `docs/adr/0059-first-stage-boot-chain-nxboot-handoff.md` (loader position + measured-boot handoff ABI)
    - `docs/adr/0055-bootctld-single-boot-state-authority.md` (record ownership — unchanged)
    - `docs/adr/0044-single-blk-device-gpt-partitions-block-layer.md` (block topology — executed by TASK-0315)
    - `docs/adr/0043-user-data-in-dedicated-cow-fs-statefs-stays-service-kv.md` (staging must not live in `/state`)
  - Related RFCs:
    - `docs/rfcs/RFC-0012-updates-packaging-ab-skeleton-v1.md` (superseded in part — see Scope)
    - `docs/rfcs/RFC-0087-reliability-failure-model-v1.md` (§4 boot targets — untouched, cross-referenced)
    - `docs/rfcs/RFC-0088-nxra-signed-recovery-action-tokens.md` (break-glass — untouched, composes)
    - `docs/rfcs/RFC-0039-supply-chain-v1-bundle-sbom-repro-sign-policy.md` (bundle-scoped signing baseline)

## Status at a Glance

- **Phase 1 (device trust anchor + verifier verdict authority)**: ✅ 2026-08-25 (TASK-0198 Phase 1 — baked anchor + verdict finality + OS deny lane, test-all green)
- **Phase 2 (health-commit v2: record v3 + quorum + deadline)**: ✅ 2026-08-25 (TASK-0036-A — private commit behind the mask, deadline armed at switch, 22 host tests + gated quorum chain)
- **Phase 3 (block substrate: virtio-blk v2 + single GPT disk)**: ✅ 2026-08-25 (TASK-0314 request ring/runs + TASK-0315 single GPT disk, virtioblkd sole owner, partition gates, IRQ completion LIVE)
- **Phase 4 (host image builder `nx image`)**: ✅ 2026-08-25 (TASK-0260 image scope — build/verify/patch/ota, deterministic, 6 integration tests; shared layout/GPT/codec authorities in `storage::layout` + `bootfmt`)
- **Phase 5 (`nxboot` loader + boot flip + measured handoff)**: ✅ 2026-08-30 (TASK-0289-A — loader complete per §7, ADR-0059 addresses frozen post-audit (home 0x9200_0000, handoff 0x9300_0000), boot flip landed: every QEMU lane boots `-kernel nxboot.bin` from the GPT disk; kernel captures the handoff pre-SATP; fallback fixture proven)
- **Phase 6 (BSB runtime projection)**: ✅ 2026-08-30 (TASK-0036-B — record commits FIRST then projects; startup resync never overwrites loader actuator effects; BSB.active maps to the standing known-good slot; full loop proven: keep-blk boot 2 loads through a bootctld-projected block)
- **Phase 7 (apply engine v2 + offline feed + crown proof)**: ✅ 2026-08-31 (TASK-0179 — path staging replaces the inline stage, component apply with NXBD-last, commit-time floor raise; CROWN PROOF green: two boots, one uart, a DIFFERENT build id chosen by the loader)
- **Phase 8 (boot trust floor closure: backstop proofs + measured surface)**: ✅ 2026-08-31 (TASK-0289-B — measured surface: `SYSCALL_BOOT_HANDOFF` 57 + bootctld `OP_GET_MEASURED` 12, cross-checked in every proof lane; three loader-backstop lanes gated: tamper→digest, downgrade→`rollback 0 < min 1`, tries-exhaustion with DEAD userspace → loader flip + record rollback observation)
- **Phase 9 (UI/CLI)**: ⬜ (TASK-0140)
- **Phase 10 (delta components)**: ⬜ (TASK-0034/0035)
- **Phase B (bundle-set granularity)**: contracted here, executed by a follow-on task family (§12)

Definition:

- “Complete” means the **contract** is defined and the **proof gates** are green (tests/markers). It does not mean “never changes again”.

## Scope boundaries (anti-drift)

This RFC is a **design seed / contract**. Implementation planning and proofs live in tasks.

- **This RFC owns**:
  - The end-to-end OTA contract: acquire (offline v1) → verify → stage → switch → boot-verify → health-commit → rollback, with anti-downgrade.
  - The `.nxs` v2 **component manifest** contract (container, schema evolution, component kinds, verification order).
  - The **NXBD** (Nexus Boot Descriptor) at-rest format — the loader-verified trust artifact.
  - The **BSB** (Boot Selection Block) at-rest format (actor discipline: ADR-0058).
  - The GPT disk layout for A/B boot images (partition roles; block-layer mechanics stay ADR-0044/TASK-0315).
  - The `updated` v2 service contract (path-based staging, component apply pipeline, idempotency).
  - The bootctl record v3 evolution (fields; ownership stays ADR-0055).
  - The device trust model for update publishers (baked anchor; verifier verdict authority).
  - Anti-downgrade semantics (manifest rollback index vs. persisted floor; loader backstop).
  - The **Phase B seam**: how bundle-set granularity attaches WITHOUT changing loader/NXBD/BSB/transport/trust.
- **This RFC does NOT own**:
  - Boot-target semantics (`normal|recovery|safe`) — RFC-0087 §4.
  - Break-glass authorization — RFC-0088 (`.nxra` composes on the ops surface unchanged).
  - Block-device driver mechanics and partition-scoped IPC — ADR-0044 + TASK-0314/0315.
  - The `.nxdelta` binary diff format — its own RFC at TASK-0034 execution (§11 reserves the seam).
  - Sigchain envelopes, transparency log, provenance, key ROTATION execution — TASK-0197/0198 later phases (§4 reserves the rotation seam).
  - App/store distribution (`.nxb` install via bundlemgrd, per-app A/B) — RFC-0039/TASK-0239 territory.
  - Network transport — named later phase (§9); the staging contract is transport-agnostic by construction.
  - Hardware root of trust — the QEMU-bound software root is labeled honestly (§13); hardware anchoring is future work referenced by TASK-0289.

### Supersession of RFC-0012

RFC-0012 remains the historical v1 contract. This RFC supersedes it in part:

- **Superseded**: the `.nxs` v1 single-purpose container (bundles-only index), the inline
  `MAX_STAGE_BYTES` = 8 KiB stage API, the non-persistent state model (reality has been
  persistent since TASK-0034 Goal 2; ownership moved to bootctld per ADR-0055), and the
  marker `updated: ready (non-persistent)`.
- **Carried forward verbatim**: deterministic tar rules, size-bound doctrine, path-safety
  rules, digest binding, fail-closed failure model, audit discipline (all restated in §3
  so this RFC reads standalone).
- **Untouched**: `.nxb` bundle contract (ADR-0020), bundlemgrd publication contract.

### Relationship to tasks (single execution truth)

- Tasks (`tasks/TASK-*.md`) define **stop conditions** and **proof commands**.
- This RFC must link to the task(s) that implement and prove each phase/milestone.

## Context

The A/B boot-state *machine* is real, persisted and QEMU-proven (bootctld, ADR-0055,
TASK-0050). The A/B *payload* is not: the system ships as ONE flat boot image
(`neuron-boot.bin`, services embedded in init, loaded via the VMM kernel option), the
device “slot switch” republishes a synthetic `build.prop` string, and `updated` verifies
a `.nxs` in RAM and discards the bytes. Two structural holes make honest OTA impossible
today:

1. **No second copy, no chooser**: nothing stores an alternative system image and nothing
   at boot time can select or verify one.
2. **No device trust anchor**: the `.nxs` signature is verified against the publisher key
   *read from the archive being verified* — any self-signed archive passes.

The end state (user decision 2026-08-25): granular **bundle-set updates** are the target
UX; the lane builds **full-image OTA first**, architected so the bundle-set evolution
adds component kinds without reworking transport, verification, trust, staging, or the
loader (§12).

## Goals

- One update container (`.nxs` v2) whose verification, trust and staging code is built
  once in end form; growth happens by adding component kinds only.
- A real A/B boot path: two boot-image slots on one GPT disk, selected and verified by a
  minimal first-stage loader (`nxboot`) before any OS code runs.
- Anti-downgrade with a persisted monotonic floor, enforced twice: at stage time
  (updated) and at boot time (loader backstop).
- Health-commit v2: quorum over declared reporters plus a wall-clock deadline, deciding
  commit vs. scheduled rollback deterministically.
- Power-cut safety at every step: torn staging, torn switch, and torn BSB writes must
  each leave the previous state authoritative.
- Offline-first acquisition that is byte-identical in contract to the later network path.

## Non-Goals

- Bundle-set execution (services leaving the embedded image) — contracted in §12,
  executed later.
- Delta formats (§11 seam only), network transport (§9 names the phase), key rotation
  execution (§4 names the seam), flashing/provisioning (TASK-0260 residual + TASK-0261).
- Any scheme that skips signature verification “for tests” (hard prohibited).

## Constraints / invariants (hard requirements)

- **Determinism**: `.nxs` v2 generation, `nx image build`, NXBD and BSB encodings are
  deterministic (stable ordering, no wall-clock in signed bytes or markers).
- **No fake success**: `nxboot: verify ok` only after signature + digest + rollback
  checks all passed; `updated: stage done` only after readback verification; measured
  handoff absent ⇒ the kernel says `handoff absent`, never fakes `ok`.
- **Bounded resources**: every at-rest/wire artifact has explicit maxima checked before
  allocation (container bounds §3; NXBD/BSB are fixed 512-byte sectors; staging streams
  in bounded chunks). The loader has a hard binary-size budget (ADR-0059).
- **Security floor**: manifest signature against the DEVICE anchor before any component
  byte is trusted; a verifier verdict of “invalid” is FINAL (no local re-adjudication);
  keys never logged; deny-by-default on the ops surface (policyd), `.nxra` composes
  additively per RFC-0088.
- **Single authority**: boot state = bootctld (ADR-0055). BSB and the measured surface
  are DERIVED projections, never second authorities (ADR-0058/0059).
- **No silent fallback**: loader fallback to the previous slot is loud
  (`nxboot: fallback ...`); keystored-unavailable fallback verification is loud
  (`updated: verify fallback (keystored unavailable)`) and still anchor-bound.
- **Stubs policy**: any stub is labeled `stub`/`placeholder` and never claims `ok`.

## Proposed design

### 1. Terminology

- **Slot**: `a` or `b`; a boot-image partition pair member.
- **Boot image**: the flat kernel+init(+embedded services) binary a slot carries.
- **Component**: one typed payload inside an `.nxs` v2 container.
- **NXBD**: 512-byte signed boot descriptor at slot-partition LBA 0.
- **BSB**: boot selection block — the loader-readable projection of boot state.
- **Staging source**: a VFS path the update container is streamed from.
- **Rollback floor** (`rollback_min_index`): persisted monotonic minimum; images with a
  lower index are rejected.

### 2. GPT disk layout (normative roles; mechanics per ADR-0044)

One disk image (`build/nexus.img`), GPT-partitioned. Partition-type GUIDs live in ONE
table in `userspace/storage` (shared by `nx image`, `virtioblkd`, `nxboot`):

| name | type | size (v1) | content |
|---|---|---|---|
| `bsb` | `NEXUS-BSB-v1` | 1 MiB | BSB double block (§6) |

(Amendment 2026-08-25, TASK-0260: the disk is GPT-only — `storage::gpt`
deliberately writes no protective MBR; nothing boots via MBR and `nxboot`
parses GPT directly. The concrete table below is realized verbatim in
`userspace/storage/src/layout.rs`, the ONE shared layout authority.)
| `boot-a` | `NEXUS-BOOT-v1` | 56 MiB | NXBD @ sector 0, boot image from sector 8 |
| `boot-b` | `NEXUS-BOOT-v1` | 56 MiB | same layout; zeroed NXBD = invalid slot |
| `system-a` | `NEXUS-SYS-v1` | 32 MiB | **reserved** (Phase B system volume) |
| `system-b` | `NEXUS-SYS-v1` | 32 MiB | reserved |
| `state` | `NEXUS-STATE-v1` | 64 MiB | statefs journal |
| `data` | `NEXUS-DATA-v1` | 128 MiB | nxfs (`/data`), incl. `/data/updates/` staging |

Rules:

- Slot partitions are written only by `updated` (inactive slot only) and provisioning
  tools; `bsb` only by bootctld (runtime), `nxboot` (boot-time actuator fields) and the
  factory image builder. Cross-partition access is deny-by-default (TASK-0315).
- Per-slot image budgets are gated in `scripts/check-image-budgets.sh` — growth is a
  conscious act, never silent.
- Staging bytes live under `/data` (nxfs) per ADR-0043 — never in the `/state` KV.

### 3. `.nxs` v2 — signed component manifest (normative)

#### Container

Deterministic tar (rules carried from RFC-0012: `manifest.nxo` first,
`manifest.sig.ed25519` second, component payload entries in manifest order; entry paths
relative, no `..`/absolute/NUL; unknown extra entries ⇒ reject).

#### Manifest schema (`manifest.nxo`, Cap’n Proto)

Schema SSOT: `tools/nexus-idl/schemas/system-set.capnp`, evolved additively with
`schemaVersion = 2`:

```text
schemaVersion: UInt8 = 2
publisherKeyId: Data (8 bytes; fingerprint into the device anchor set)
buildId: Text (deterministic build identifier; printed on uart at boot)
rollbackIndex: UInt32 (monotonic per publisher line)
components: List(Component)
  - kind: UInt8 (see table)
  - name: Text
  - size: UInt64
  - sha256: Data (32 bytes; over the payload entry bytes)
  - payloadPath: Text (tar entry path)
  - kindData: Data (kind-specific, bounded; empty for boot-image)
```

#### Component kinds

| kind | name | v1 | payload |
|---|---|---|---|
| 1 | `boot-image` | ✅ built | flat boot image + embedded `boot.nxbd` entry (the signed NXBD, written verbatim to the slot) |
| 2 | `bundle` | reserved (§12) | `.nxb` bundle for the system volume |
| 3 | `boot-image-delta` | reserved (§11) | `.nxdelta` stream, base = active slot |
| 4 | `bundle-delta` | reserved (§11/§12) | per-bundle delta |
| 5 | `rotation-record` | reserved (§4) | signed trust-set extension |

Unknown kind ⇒ deterministic reject (`updated: component kind unsupported`), never skip.

#### Size bounds (normative; enforced before allocation)

Carried from RFC-0012 and extended: `MAX_NXS_ARCHIVE_BYTES` 100 MiB,
`MAX_MANIFEST_BYTES` 1 MiB, `MAX_COMPONENTS_PER_SET` 256, per-kind payload caps
(`boot-image` ≤ slot partition size − 4 KiB descriptor region).

#### Verification order (normative)

1. Manifest signature (Ed25519 over raw `manifest.nxo` bytes) against the **device
   anchor** (§4). The publisher key inside the archive is a LOOKUP HINT
   (`publisherKeyId`), never a trust input.
2. `rollbackIndex` ≥ persisted floor (stage-time anti-downgrade, §10).
3. Per component, while streaming: sha256 over payload bytes must match the manifest
   entry before any byte is written to its destination’s commit point.
4. Kind-specific checks (e.g. `boot-image`: embedded NXBD fields must match the manifest
   — same buildId, same rollbackIndex, image digest binds).

### 4. Trust model (normative)

- **Device anchor v1 = build-baked publisher set**: `policies/update-trust.toml` →
  `BAKED_PUBLISHERS` via build-script codegen (same pattern and rationale as
  RFC-0088’s `BAKED_TRUST`: the anchor is configuration-shaped SECURITY STATE; runtime
  mutability without a verified root would be attack surface). Honest label: exactly as
  strong as image integrity; the loader (§7) extends that integrity to the image itself.
- **Verifier verdict authority**: when keystored answers a verify request, its verdict is
  FINAL. A local fallback verify exists only for keystored **unavailability** (transport
  failure), is loud, and binds to the same baked anchor.
- **Rotation seam (reserved)**: `rotation-record` components extend the baked set at
  runtime, each record signed by an already-trusted key, persisted append-only,
  monotonic. Executed under TASK-0197/0198; the mechanism EXTENDS the anchor, never
  replaces it.
- `.nxra` (RFC-0088) stays the break-glass path on the ops surface: standing gates first,
  token rescues a denied request; nothing here weakens standing paths.

### 5. NXBD — Nexus Boot Descriptor (at-rest, normative)

One 512-byte sector at slot-partition sector 0; the image payload begins at sector 8
(4 KiB aligned). Fixed layout, little-endian:

```text
[0..8)    magic "NXBD0001"
[8..10)   version u16 (=1)
[10..12)  flags u16 (reserved, must be 0)
[12..16)  rollback_index u32
[16..24)  image_size u64
[24..56)  image_sha256 [32]
[56..88)  build_id [32] (ascii, NUL-padded)
[88..96)  load_addr u64
[96..104) pubkey_id [8]
[104..448) reserved (must be 0)
[448..512) ed25519_sig over bytes [0..448)
```

Rules:

- Signed at build time by `nx image build --sign`; `updated` writes it VERBATIM — the
  device never re-signs.
- **NXBD-last discipline**: during staging the image body is written and readback-verified
  first; the NXBD sector is written LAST. A slot without a valid NXBD is invalid by
  definition — a torn stage can never produce a bootable half-slot.
- The loader accepts a slot only if: magic/version ok, signature verifies against the
  loader’s baked anchor, streamed image sha256 matches, `rollback_index ≥` BSB floor.
- When Phase B arrives, the NXBD is UNCHANGED; a system-volume descriptor (same shape,
  own type) is verified by the then-verified boot image, not by the loader (§12).

### 6. BSB — Boot Selection Block (at-rest, normative; actor discipline: ADR-0058)

Two 512-byte blocks at `bsb` partition sectors 0 and 1. A reader takes the block with
valid magic+CRC and the higher `seq`. A writer always writes the OTHER block (one-sector
write = power-cut atomic; a torn write leaves the previous block authoritative).

```text
[0..8)   magic "NXBSB1\0\0"
[8..16)  seq u64 (monotonic)
[16..18) version u16 (=1)
[18]     active_slot u8 (0=a, 1=b)
[19]     next_slot u8 (0xFF = none)
[20]     tries_left u8
[21]     health u8 (bit0 = committed)
[22]     boot_target u8 (projection of RFC-0087 §4; loader treats it as opaque pass-through)
[23]     reserved u8 (0)
[24..28) rollback_min_index u32
[28..508) reserved (0)
[508..512) crc32 over [0..508)
```

Writers (full matrix in ADR-0058): **bootctld** = runtime authority, projects the record
after every committed mutation (record commits FIRST, BSB second; a crash between the two
is healed by idempotent re-projection at bootctld start). **nxboot** = boot-time actuator:
may only decrement `tries_left` and, on exhaustion, flip `active_slot`/clear `next_slot`.
**`nx image`** = factory initialization. The BSB is a derived artifact — bootctld’s
statefs record stays the single boot-state authority (ADR-0055).

**Rollback observation (TASK-0289-B, normative)**: when bootctld attaches and the
on-disk pair shows the ACTUATOR exhausted a trial the record still carries
(`next_slot` cleared, `tries_left == 0`, `health_committed == false` — the shape only
the loader's exhaustion write produces) it does NOT re-project the pending trial (that
would hand a broken image its tries back); it rolls the RECORD back to the recorded
rollback slot, persists, and announces `bootctld: rollback observed (trial exhausted)`.
The `health_committed` bit separates this from the crash window between record commit
and projection: that crash leaves the PRE-switch block, a committed steady state, and
re-projects the trial as before. This is a record-side rule — the ADR-0058 write matrix
is unchanged.

### 7. `nxboot` first-stage loader (normative behavior; placement per ADR-0059)

`nxboot` is a minimal bare-metal S-mode loader (`source/boot/nxboot/`), entered by the
SBI firmware, self-relocating, with its own bounded polling virtio block reader and an
alloc-free/bounded GPT walk. Behavior per boot:

1. Read BSB (double-block rule). Unreadable/corrupt BSB on both blocks ⇒ deterministic
   panic marker + system reset (never guess a slot).
2. Select slot: `next_slot` if set and `tries_left > 0` (decrement via alternate-block
   write BEFORE loading — a boot loop into a broken image converges); else `active_slot`.
   On exhaustion: flip to `active_slot`, clear `next_slot`, marker
   `nxboot: fallback (slot=<s> exhausted) -> slot=<s'>`.
3. Read NXBD + image; verify signature (baked anchor `policies/os-trust.toml` →
   build-baked, nxra pattern), streamed sha256, `rollback_index ≥ rollback_min_index`.
   Any failure: `nxboot: verify FAIL (slot=<s> <reason>)` + fallback to the other slot
   (same checks); both slots failing ⇒ loud panic + reset.
4. Write the measured-boot handoff page (ADR-0059: magic, slot, image_sha256,
   rollback_index, bsb_seq, tries_decremented, crc) and jump to `load_addr` with the
   firmware-provided hart/DTB registers restored.

Markers (normative): `nxboot: bsb ok (slot=<s> seq=<n>)`,
`nxboot: tries <n>-><n-1> (slot=<s> trial)`,
`nxboot: verify ok (slot=<s> build=<id8> rbidx=<n>)`, `nxboot: jump slot=<s>`,
`nxboot: verify FAIL (slot=<s> <reason>)`, `nxboot: fallback ...`, `nxboot: PANIC ...`.
Stable FAIL reasons: `nxbd | sig | digest | rollback <n> < min <m> | io`.

The kernel probes the handoff page: present ⇒ `KSELFTEST: boot handoff ok (measured)`
and exposure to bootctld via bootinfo; absent (direct-kernel dev boot) ⇒ honest
`neuron: boot handoff absent (direct kernel)`.

### 8. `updated` v2 service contract (normative)

`updated` remains a bootctld CLIENT (ADR-0055). Wire evolves (same magic/version
discipline as v1):

- `OP_STAGE_SOURCE { path }` **replaces** inline `OP_STAGE` (removed in the same change —
  no dual API). `path` is a bounded VFS path (`/updates/...` on the data volume, or
  `pkg://updates/...`) — see the §9 namespace note.
- `OP_FEED_LIST` / `OP_CHECK`: offline feed enumeration (§9).
- `OP_SWITCH / OP_HEALTH_OK / OP_GET_STATUS / OP_BOOT_ATTEMPT`: unchanged pass-throughs.

Apply pipeline (normative, component-dispatched, per §3 verification order):

1. Stream the container from the staging source in bounded chunks (64 KiB).
2. Verify manifest signature (device anchor) + stage-time anti-downgrade (§10).
3. Per component (v1: `boot-image`): stream-digest → write to the INACTIVE slot via the
   partition-scoped block service → readback-verify → write NXBD LAST (§5).
4. `bootctld OP_STAGE`, then (on request) `OP_SWITCH(tries=2)` — machine semantics
   unchanged from RFC-0012/ADR-0055.

Idempotency (normative): staging is restartable at any cut point; a re-stage of the same
container converges to the same slot state; a torn stage leaves the slot invalid
(NXBD-last) and the machine unaware (`bootctld` stage happens after readback).

Markers: `updated: ready (bootctl client)` (unchanged),
`updated: stage begin (source=<path>)`, `updated: component <kind> verified (sha=<8>)`,
`updated: stage done (slot=<s> build=<id8>)`, `updated: stage rejected (<reason>)`,
`updated: restage clean`, `updated: verify fallback (keystored unavailable)`.
Stable reject reasons: `untrusted publisher | sig | digest | bounds | path |
component kind unsupported | downgrade | io | slot-active`.

Audit: every op emits a structured record (scope=`updated`) — op, source, buildId,
result, reject reason; never key material.

### 9. Acquisition: offline feed v1, network later (contract-stable)

- v1: containers appear in the DATA VOLUME's `updates/` directory (developer/
  provisioning drop) or as build-time fixtures under `pkg://updates/`.
  NAMESPACE NOTE (TASK-0179): that volume is mounted at the VFS root — its home
  layout is `/Bilder`, `/Dokumente`, … — so the wire path is `/updates/<name>.nxs`,
  NOT `/data/updates/...`. `/data` names the partition, never a VFS prefix. `OP_FEED_LIST` enumerates deterministically.
- Network phase (later, blocked on the 2-VM CI lane): a downloader lands bytes at the
  SAME staging location, then the identical §8 pipeline runs. The contract is
  transport-agnostic by construction; resumable downloads fall out of path-based staging.

### 10. Anti-downgrade (normative)

- The floor is `rollback_min_index` (bootctl record v3 + BSB projection).
- **Stage-time**: `updated` rejects `manifest.rollbackIndex < floor`
  (`updated: stage rejected (downgrade)`).
- **Commit-time**: on health commit of a slot whose NXBD carries index `n > floor`,
  bootctld raises the floor to `n` in the record and projects to BSB
  (`bootctld: rollback-min raised (<old>-><new>)`). The floor NEVER decreases.
- **Boot-time backstop**: the loader rejects `rollback_index < floor` (§7) — a bypassed
  or compromised userspace cannot boot a downgraded image.
- Honest label: without a hardware monotonic counter this floor is as strong as disk +
  image integrity (QEMU-soft-root); TASK-0289 records the hardware seam.

### 11. Delta seam (reserved; format RFC at TASK-0034 execution)

Delta updates are component kinds (`boot-image-delta`, `bundle-delta`), not a new
container: the manifest, trust, staging and machine paths are UNCHANGED; the apply
dispatch reconstructs the target (base = active slot bytes) into the inactive slot, then
the identical digest/readback/NXBD tail runs. The `.nxdelta` stream format (rollsum +
zstd, resume checkpoints, determinism) gets its own normative RFC when TASK-0034
executes — this section is the RFC seed its ledger was blocked on.

### 12. Phase B — bundle-set granularity (contracted, NOT built in this lane)

Target UX: per-service/app granular updates. The seam, fixed now:

- `bundle` components (§3) target a **system volume** (`system-a/b` partitions, reserved
  in §2), an RO image (pkgimg lineage) whose root descriptor (same 512-byte shape as
  NXBD, own magic) is listed as a manifest component and verified BY THE BOOT IMAGE
  after the loader verified the boot image — the chain extends downward, the loader
  never changes.
- Services migrate out of the embedded init image into the verified system volume
  (dynamic loading via the existing execd/bundlemgrd payload discipline); the boot image
  shrinks to kernel + init + the loader-of-services floor.
- Updates then ship changed bundles only; unchanged bundles are reused from the active
  system volume during apply (component-level dedup — the user-facing win).
- **Invariant (the proof of preparation)**: Phase B changes component POPULATION and the
  system-volume verifier; it changes NOTHING in `.nxs` v2 container rules, trust anchor,
  staging pipeline, NXBD, BSB, or `nxboot`.

### 13. Record v3 (bootctld-internal; ownership per ADR-0055)

Payload v2 (9 bytes) grows to v3: `+ rollback_min_index u32, health_quorum_mask u8,
commit_deadline_ns u64`. Versioned decode (v3/v2/v1/legacy), writes always v3,
snapshot-restore commit discipline unchanged. Health-commit v2 semantics:

- Quorum: a bounded, declaratively registered reporter set (init stage-graph services);
  commit fires only when the quorum mask completes (RFC-0013 `ready` vocabulary, not
  `init: up`).
- Deadline: `commit_deadline_ns` armed at switch; expiry without quorum ⇒ rollback is
  scheduled (machine semantics; the reset consumes it). Injectable clock in tests.

## Security considerations

- **Threat model**: malicious container injection; **archive-supplied-key trust bypass
  (the current live hole — closed by §4)**; digest/TOCTOU swaps between verify and write
  (closed by stream-digest + readback + NXBD-last); downgrade to vulnerable images
  (closed by §10, triple-enforced); boot-image tampering at rest (closed by §7 loader
  verification); torn-write corruption of selection state (closed by §6 double block);
  confused-deputy staging (closed by partition-scoped access + policyd gating);
  health-check manipulation (quorum over kernel-attributed reporters, RFC-0087 §2).
- **Mitigations**: baked anchors (two: publisher set for `updated`, OS key set for
  `nxboot`); verifier verdict finality; consume-before-act discipline inherited where
  `.nxra` composes; bounded everything.
- **DON'T DO**: don’t verify against archive-supplied keys anywhere, ever again; don’t
  re-adjudicate a verifier’s “invalid”; don’t write NXBD before readback; don’t claim
  hardware-root properties (label `qemu-soft-root`); don’t add a second boot-state store.
- **Open risks**: software-only rollback floor (hardware seam at TASK-0289); proof keys
  in `policies/*-trust.toml` until provisioning (production images replace the sets).

## Failure model (normative)

- Verification failure at ANY §3 step: reject deterministically, stage nothing, slot
  state untouched, stable reason in marker + audit.
- Torn stage: slot invalid (no NXBD) — next stage restarts clean (`updated: restage clean`).
- Torn BSB write: previous block authoritative — state loss ≤ one projection, healed by
  bootctld resync.
- Both slots unbootable: loader panics loudly and resets — no silent boot of unverified
  bytes (recovery = reprovisioning path, out of scope here).
- Quorum missing at deadline: scheduled rollback, marker-visible; never a silent commit.

## Proof / validation strategy (required)

### Proof (Host)

- BSB codec: seq/CRC/pick-newer/torn-block matrix.
- NXBD vectors: accept, bad-sig, bad-digest, rollback-below-floor.
- Slot-selection state table incl. tries-decrement and exhaustion-flip.
- Apply pipeline vs. in-memory block fake: full run + power-cut matrix (kill after each
  pipeline step ⇒ restage idempotent, slot never half-valid).
- Trust: `test_reject_untrusted_publisher`, `test_reject_keystored_says_invalid`,
  `test_reject_downgrade`, `test_reject_unknown_component_kind`.
- Quorum/deadline with injected clock; record v2→v3 migration.
- `nx image` determinism (build twice ⇒ identical bytes) + verify round-trip.

```bash
cd /home/jenning/open-nexus-OS && just test-host
```

### Proof (OS/QEMU)

Crown proof (profile `ota`, two boots in ONE uart stream): stage a container carrying a
DIFFERENT build id → switch → SBI reset → `nxboot` boots slot B → uart shows the new
build id → quorum commit → record shows B active + floor raised. Adversarial lanes:
stage-time tamper/downgrade rejects; boot-time backstops (fixture disks); tries-exhausted
auto-fallback (`ota-fallback` profile, fault-fixture image that HONESTLY withholds
health); power-cut-during-stage (keep-blk kill-at-marker). The headless ladder and the
`ci-os-reset` three-boot lane stay green through every phase.

```bash
cd /home/jenning/open-nexus-OS && just test-os            # headless ladder
cd /home/jenning/open-nexus-OS && just test-os ota        # crown proof (once Phase 7 lands)
```

### Deterministic markers

Marker SSOT stays `scripts/qemu-test.sh` + `tools/nx/chains/markers.txt` +
`source/apps/selftest-client/proof-manifest/markers/` — updated together per change
(repo marker doctrine). Normative strings are listed in §7/§8.

## Alternatives considered

- **Loader parses the capnp manifest**: rejected — a bare-metal trust root should verify
  one fixed 512-byte descriptor, not a schema’d container (surface, size, auditability).
- **VMO-handle staging as the v1 transport**: rejected — multi-MB images shouldn’t
  require one mapped window; the network phase needs a file anyway; VMO stays a later
  fast path.
- **Runtime trust partition for publisher keys**: rejected for v1 — mutable state that
  itself needs a verification root; rotation-records extend the baked anchor instead.
- **Second boot-state store for the loader**: rejected — the BSB is a derived projection
  of the ADR-0055 record, with a field-level write matrix (ADR-0058), not an authority.
- **Bundle-set first**: rejected as the lane opener — it requires dynamic service
  loading + a verified system volume before ANY real update can be proven; full-image
  first delivers the entire trust/transport/boot chain, which Phase B then reuses
  unchanged (§12).

## Open questions

- `system-a/b` volume format for Phase B: pkgimg v2 lineage vs. nxfs-RO — decide with the
  Phase B RFC (the §12 seam is format-agnostic by design).
- ~~Whether `OP_GET_RECORD` (bootctld op 11) becomes the measured surface read path or a
  dedicated op is added~~ — RESOLVED 2026-08-31 (TASK-0289-B): dedicated `OP_GET_MEASURED`
  (op 12) serving the kernel-validated raw handoff record behind a present flag;
  `OP_GET_RECORD` stays the persisted-record snapshot (different authority, different
  payload). Kernel surface: `SYSCALL_BOOT_HANDOFF` (57), read-only — ADR-0059 amendment.

---

## Implementation Checklist

**This section tracks implementation progress. Update as phases complete.**

- [x] **Phase 1**: device trust anchor + verdict authority — proof: `cargo test -p updates_host` (`test_reject_untrusted_publisher`, `test_accept_baked_publisher`) + `SELFTEST: updates trust reject ok` gated in headless/smp1/reset (2026-08-25)
- [x] **Phase 2**: record v3 + quorum + deadline — proof: 22 bootctld host tests + `bootctld: health quorum ok (2/2)` + `SELFTEST: bootctl quorum ok` gated every proof boot (2026-08-25)
- [x] **Phase 3**: virtio-blk v2 + single GPT disk — proof: `virtioblkd: gpt ok (parts=7)` + `blk: irq completion on` + `SELFTEST: blk cross-partition deny ok` gated; keep-blk double boot + reset lane green (2026-08-25)
- [x] **Phase 4**: `nx image build/verify/patch/ota` — proof: 6 integration tests (determinism double-build, verify round-trip via the shared parser, tamper/wrong-key rejects, patch preservation, slot-budget reject, `.nxs` v2 decode with bound NXBD) (2026-08-25)
- [x] **Phase 5**: `nxboot` + boot flip + handoff — proof: `nxboot: bsb ok`/`verify ok`/`jump slot=a` + `KSELFTEST: boot handoff ok (measured)` REQUIRED in every proof lane; loader-fallback fixture (`verify FAIL (slot=b nxbd)` → `fallback -> slot=a` → full boot); keep-blk + reset three-boot lanes green through the loader; 12 host tests (select table + flow adversarial matrix) (2026-08-30)
- [x] **Phase 6**: BSB projection — proof: `bootctld: bsb sync (seq=` + `SELFTEST: bootctl bsb ok` gated in headless/smp1; keep-blk boot 2 reads the projected block (`nxboot: bsb ok (slot=a seq=8)`); reset three-boot lane green; 5 host tests incl. actuator-absorption fail-closed matrix (2026-08-30)
- [x] **Phase 7**: apply engine v2 + offline feed + crown proof — proof: `just ci-os-ota` (gated in `test-all`): `updated: stage done (slot=b build=otaB…)` → `nxboot: verify ok (slot=b build=otaB… rbidx=2)` → `nxboot: jump slot=b` → `bootctld: commit ok (slot=b)` → `bootctld: rollback-min raised (1->2)` → `SELFTEST: ota flip ok`; headless deny lanes for untrusted/digest/downgrade; 10 host tests incl. the power-cut matrix (2026-08-31)
- [x] **Phase 8**: backstop proofs + measured surface — proof: `SELFTEST: measured boot log ok` REQUIRED in the headless/smp1 ladders (slot cross-check against the authority); `just ci-os-ota-backstops` (gated in `test-all`): `ota-tamper` (`nxboot: verify FAIL (slot=b digest)` → fallback → full ladder), `ota-downgrade` (`nxboot: verify FAIL (slot=b rollback 0 < min 1)` → fallback), `ota-fallback` (four boots one uart: staged real os-B, bricked trials via `init: health withheld (fault fixture)` + QMP power-cycles, `nxboot: fallback (slot=b exhausted) -> slot=a`, `bootctld: rollback observed (trial exhausted)`, `SELFTEST: ota fallback ok`); host: exhaustion-observed shape matrix + backstop-arm CLI test (2026-08-31)
- [ ] **Phase 9**: UI/CLI — proof: `SELFTEST: nx update status ok`
- [ ] **Phase 10**: delta components — proof: `SELFTEST: ota delta ok`
- [ ] Tasks linked with stop conditions + proof commands (lane table in `tasks/IMPLEMENTATION-ORDER.md`).
- [ ] Security-relevant negative tests exist (`test_reject_*`) for every stable reject reason.
