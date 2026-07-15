# RFC-0071: nxfs — user-data filesystem (container/volumes, transactional CoW track, integrity, encryption classes) contract seed

- Status: Draft (2026-07-15) — user decision 2026-07-15: production-grade, future-proof ("Apple-like"), not the minimal solution; engine ships in phases but the contract is CoW-ready from day one.
- Owners: @runtime
- Created: 2026-07-15
- Last Updated: 2026-07-15
- Links:
  - Tasks: `tasks/TASK-0292-nxfs-v1-core-host-first.md` (P1 execution + proof), `tasks/TASK-0293-nxfsd-os-bringup-gpt-mount-data-keepblk.md` (P2 execution + proof)
  - ADRs: `docs/adr/0043-user-data-in-dedicated-cow-fs-statefs-stays-service-kv.md`, `docs/adr/0044-single-blk-device-gpt-partitions-block-layer.md`
  - Related RFCs: `docs/rfcs/RFC-0018-statefs-journal-format-v1.md` (service-state KV — NOT this), `docs/rfcs/RFC-0072-vfs-v2-writable-providers-readdir-stable-errors.md` (mount surface), `docs/rfcs/RFC-0073-app-files-surface-svc-files-permission-filemanager-role.md` (app surface), `docs/rfcs/RFC-0040-zero-copy-vmos-v1-plumbing-host-first-os-gated.md` (data plane), `docs/rfcs/RFC-0016-device-identity-keys-v1.md` (key material)
  - Track: `tasks/TRACK-STASH-USER-DATA-FS.md`
  - Supersedes: the "securefsd" overlay direction of `tasks/TASK-0182`/`tasks/TASK-0183` (encrypted user data is an nxfs encryption class, not a separate overlay filesystem); absorbs the user-data snapshot ambitions of `tasks/TASK-0134` (statefs-side remainder stays there).

## Status at a Glance

- **Phase 0 (contract + on-disk format v1 spec)**: 🟨 (this document)
- **Phase 1 (host-first core: transactions, extents, checksums, replay, fsck)**: ⬜ — `TASK-0292`
- **Phase 2 (OS bring-up: nxfsd on GPT partition, writable `/data` mount, cold-boot persistence)**: ⬜ — `TASK-0293`
- **Phase 3 (CoW checkpointing + snapshots/clones)**: ⬜ — task seeded when P2 is proven
- **Phase 4 (encryption classes via keystored-derived keys)**: ⬜ — task seeded when P3 is proven

Definition:

- "Complete" means the **contract** is defined and the **proof gates** are green (tests/markers). It does not mean "never changes again".

## Scope boundaries (anti-drift)

This RFC is a **design seed / contract**. Implementation planning and proofs live in tasks.

- **This RFC owns**:
  - The nxfs on-disk format (superblock, object table, extents, journal, checksum discipline, versioning).
  - The transaction model (crash-atomicity semantics, replay rules, generation counters).
  - The container/volume model and its capability surface (snapshots/clones as declared, versioned capabilities).
  - The encryption-class model and key-derivation contract (keystored → HKDF → volume/file keys).
  - The nxfsd service boundary: which block extent it owns, what it exposes to vfsd, the VMO data-plane rule.
  - The `fsck-nxfs` offline tool contract (exit codes, repair semantics).
- **This RFC does NOT own**:
  - The vfsd wire protocol, ReadDir/write ops, or the stable error-code SSOT (→ RFC-0072).
  - The app-facing `svc.files.*` surface, permissions, or the filemanager role (→ RFC-0073).
  - statefs — the service-state KV keeps its own contract (RFC-0018) and hardening tasks (TASK-0025/0026/0027).
  - GPT partition discovery / `PartitionView` block layer (→ ADR-0044; implemented under TASK-0293).
  - Kernel VMO sealing guarantees (→ RFC-0040 / TASK-0290).

### Relationship to tasks (single execution truth)

- `TASK-0292` implements + proves Phase 1 (host).
- `TASK-0293` implements + proves Phase 2 (OS/QEMU).
- Phases 3 and 4 get **new tasks** (seed-when-ready); their contracts are already fixed here so the P1 format does not need breaking changes to grow into them.

## Context

Open Nexus has no writable user-data filesystem. `statefs` (RFC-0018, Done) is a journaled KV store for
boot-critical **service state** — 255-byte keys, ~8 KiB effective values over IPC, full-RAM `BTreeMap`,
whole-device journal. `packagefsd`/`vfsd` are read-only. The stash app (file manager) ships as an honest
mock because there is nothing to list or write. Meanwhile the platform ambitions (privileged
`filemanager` role, pickers, media apps) all assume a real `/data`.

We want the end state to be production-grade in the way modern consumer OS filesystems are:
crash-safe without fsck-on-boot, copy-on-write with cheap snapshots/clones, integrity-checked, and
encrypted at rest by class — while staying honest about what each phase actually proves.

## Goals

- One dedicated user-data filesystem service (`nxfsd`) owning one GPT partition, mounted read-write at
  `/data` through vfsd (RFC-0072 provider API).
- **Crash-atomic transactions**: after any power cut, replay yields a state containing exactly the
  committed transactions — never a torn file, never a half-applied rename.
- **Integrity**: every metadata block carries a checksum; corruption is detected fail-closed, never
  silently returned to apps.
- **Container/volume model**: one container per partition; volumes share the container's free space
  (no fixed sub-partitioning); volume = unit of snapshot/clone/encryption policy.
- **Encryption classes** (Phase 4): per-volume/per-file AEAD keyed from keystored-managed device
  material via HKDF; class None / Device in v1 scope, per-user classes reserved.
- **Zero/low-copy data plane**: bulk file bytes move as VMO handles (RFC-0040), never through the
  8 KiB IPC inline frame; inline fallback only for small reads/writes below a fixed threshold.
- **Offline tooling**: `fsck-nxfs` host tool with deterministic output and stable exit codes.

## Non-Goals

- POSIX completeness (no hardlinks, no permissions bits beyond owner-subject in v1, no mmap-backed
  shared writable mappings, no xattrs in v1 — the format reserves room, the contract does not promise them).
- Replacing statefs. Boot-critical service state stays in statefs (ADR-0043).
- Multi-device containers, RAID, external/removable media (→ `tasks/TRACK-REMOVABLE-STORAGE.md`).
- Kernel changes. nxfs is pure userspace over the existing block driver + VMO syscalls.
- Key escrow / user-credential-derived keys (reserved as class hooks, not v1 scope).

## Constraints / invariants (hard requirements)

- **Determinism**: replay is deterministic; host crash-injection tests replay byte-identical images to
  identical states. No timing-dependent recovery.
- **No fake success**: `nxfsd: mounted /data` only after superblock + checkpoint validated; no marker
  before real behavior.
- **Bounded resources**: bounded journal replay window; bounded per-transaction dirty set; bounded
  directory-entry and name sizes (name ≤ 255 bytes UTF-8, path depth ≤ 32 in v1); bounded open-handle
  table. All caps are explicit constants in the format spec.
- **Bounded parsing**: every on-disk structure has explicit length fields validated before use;
  malformed structures → deterministic error, never UB or panic. No `unwrap`/`expect` in the daemon.
- **Security floor (even in bring-up)**: access only via capability-gated vfsd routes; nxfsd never
  trusts client-supplied paths without canonicalization (reuses the RFC-0042 discipline); checksums
  validated before content is served.
- **Stubs policy**: any stubbed capability (e.g. snapshots before Phase 3) reports `Unsupported`
  deterministically — it never fakes success.
- **statefs untouched**: no change to RFC-0018 on-disk format or statefsd semantics from this track.

## Proposed design

### Architecture

```
apps (svc.files.*, RFC-0073)
        │ capability-gated route
      vfsd  ── mount table: /packages (RO, packagefsd) + /data (RW, nxfsd)   [RFC-0072]
        │ provider protocol (control: Cap'n Proto; data: VMO handles)
      nxfsd ── nxfs engine (userspace/nxfs crate, host-first)
        │ BlockDevice trait (userspace/storage; RemoteBlockDevice client)
  virtioblkd ── device owner: virtio queue + GPT parse, serves               [ADR-0044]
        │       partition-scoped block IO ("data" → nxfsd, "state" → statefsd)
  virtio-blk device (single, GPT: state | data)
```

The engine is a host-first library crate (`userspace/nxfs`) exactly like `userspace/statefs`:
all format/transaction/replay logic is testable on the host against `MemBlockDevice` and image files;
`nxfsd` is a thin service shell (policy gate + IPC + VMO plumbing).

### Contract / interface (normative)

**On-disk format v1** (block size 4096; all integers little-endian; all structures versioned):

- **Superblock** (block 0, plus mirror in last block): magic `NXFS`, format version, container UUID,
  block count, checksum algorithm id, **two checkpoint pointers** (A/B, each = root address +
  generation + checksum). Mount picks the newest *valid* checkpoint; a torn checkpoint write can
  never brick the container (the other slot stays valid).
- **Checkpoint**: root of the object table + allocation info + volume table, written
  **copy-on-write**: new checkpoint blocks are written to free space, then the superblock slot flips.
  Phase 1 may keep the object table update-in-place behind a metadata journal, but the
  checkpoint-flip commit protocol is the contract from day one — Phase 3 turns the whole metadata
  tree CoW without a format break.
- **Object table**: maps `object_id (u64)` → object record (kind: file/dir/symlink-reserved,
  size, extent list, created/modified ns timestamps, owner subject, encryption class, flags).
  Extents: `(logical_offset, physical_block, block_count)` — contiguous runs, bounded count per
  record with continuation records.
- **Directories**: objects whose content is a sorted array of entries
  `(name_len u8, name utf-8, object_id u64, kind u8)` in checksummed blocks. Lookup is
  deterministic; iteration order is byte-order of names (this is the ReadDir pagination order
  promised through RFC-0072).
- **Journal**: bounded circular metadata journal. Record =
  `magic NXJL | txn_id u64 | kind u8 | payload_len u32 | payload | crc32c`. Kinds:
  `TXN_BEGIN`, `MUTATE` (object-table/dir/alloc deltas), `TXN_COMMIT`, `CHECKPOINT_SEAL`.
  Replay applies only transactions with `TXN_COMMIT` (2-phase rule, same discipline TASK-0026
  brings to statefs); everything else is discarded. Replay is bounded by journal size.
- **Checksums**: crc32c on every metadata block (journal records, directory blocks, object-table
  blocks, checkpoints, superblock). Data extents: checksum field reserved per extent; Phase 1 MAY
  leave data checksums disabled (flag in superblock), Phase 3 enables CoW data checksums. A failed
  metadata checksum is `EINTEGRITY`, fail-closed.
- **Generation counters**: every checkpoint carries a monotonically increasing generation; every
  committed txn a monotonically increasing `txn_id`. These are the anti-torn-write and
  anti-reorder spine, and (Phase 4) the AEAD nonce inputs.

**Container / volumes**:

- v1 ships exactly one volume (`data`, volume_id 1) — but the volume table is on disk from the
  start: `volume_id, name, root object_id, encryption class, flags, snapshot list (reserved)`.
- Snapshot/clone (Phase 3) = new volume-table entry pointing at a frozen checkpoint root; CoW makes
  it O(1). Until Phase 3, snapshot ops return `Unsupported`.

**Transactions (service-visible semantics)**:

- Every mutating vfsd op (create/write/rename/remove/mkdir/truncate) is one transaction; a rename is
  atomic (never both/neither name visible after crash — exactly one).
- Durability: `TXN_COMMIT` written + device flush (virtio `F_FLUSH`) before success is reported for
  ops that request `sync`; batched group-commit otherwise (bounded flush interval). The contract
  states both modes explicitly — no hidden write-back lies.

**Encryption classes (Phase 4 contract, fixed now)**:

- AEAD: `XChaCha20-Poly1305`. Per-extent encryption; nonce = deterministic construction from
  `(volume_key, object_id, extent_index, txn_id)` — never reused for the same key.
  AAD binds `(container_uuid, volume_id, object_id, logical_offset, payload_len)`.
- Key hierarchy: keystored device key material → HKDF(label `"nxfs.volume.<container-uuid>.<volume-id>"`)
  → volume key → (reserved) per-file wrapped keys. Signing keys are **never** used directly as AEAD
  keys (HKDF with labeled context mandatory).
- Classes: `None` (plaintext, integrity only), `Device` (volume key from device material —
  protects data at rest if the medium leaves the device), reserved `User` (credential-wrapped;
  out of scope). Class is per-volume in Phase 4 v1; the per-file class field exists in the object
  record from Phase 1 (value = inherit).
- **Honest limitation (documented, not hidden)**: QEMU dev targets have no sealed storage / secure
  element; "Device" class protects against medium-only theft, not against an attacker with the
  device key from `/state/keystore`. This is stated in `docs/storage/nxfs.md` and the markers say
  `nxfsd: encryption on (device-class)` — nothing stronger.

**Data plane (normative)**:

- Reads/writes above `INLINE_IO_MAX = 4096` bytes move as VMO handles (RFC-0040): read returns a
  VMO the service filled (Phase 2) and later maps zero-copy from cache (TASK-0295); write passes a
  VMO the service reads. Inline byte payloads above the threshold are a protocol error (`E2BIG`),
  not a silent slow path.

**`fsck-nxfs` (host tool)**:

- Validates superblock/checkpoints/journal/object-table/dir graph + checksums; replays offline.
- Exit codes: `0` clean, `1` repaired (orphan txns aborted, dangling objects collected to
  `/lost+found`), `2` unrecoverable. `--repair` never rewrites data payloads and never "fixes"
  ciphertext; it only discards uncommitted/unreachable state. Deterministic report output.

### Phases / milestones (contract-level)

- **Phase 0**: this contract + on-disk format v1 spec frozen (fields, caps, error mapping).
- **Phase 1** (`TASK-0292`): host-first engine — format read/write, transactions + replay,
  crash-injection determinism, `fsck-nxfs`. Proof: `cargo test -p nxfs` (+ tool tests).
- **Phase 2** (`TASK-0293`): nxfsd on its GPT partition (ADR-0044), vfsd RW mount `/data`
  (RFC-0072), keep-blk cold-boot persistence proof, stash writes through RFC-0073 surface.
- **Phase 3**: full-CoW metadata tree + snapshots/clones + data checksums (new task + possibly a
  narrow ADR for the tree layout; **no format break** — capability flags flip on).
- **Phase 4**: encryption classes per the contract above (new task; requires Phase 3 CoW so
  rekey/snapshot semantics stay sane).

## Security considerations

- **Threat model**:
  - Malicious/compromised app reaching for other apps' data → mitigated by capability-gated routes +
    vfsd namespace mediation (RFC-0042/0073); nxfsd itself never sees un-mediated app input.
  - Tampered or bit-rotted medium → metadata checksums fail closed (`EINTEGRITY`); Phase 3 extends
    to data; Phase 4 AEAD makes tampering of encrypted extents cryptographically detected.
  - Torn writes / power cuts → dual checkpoint slots + 2-phase journal replay.
  - Path tricks (`..`, encoding games) → canonicalization at the vfsd boundary (RFC-0042 rules);
    nxfsd additionally rejects non-canonical names defensively.
- **Mitigations**: deny-by-default policyd caps on the nxfsd route; bounded parsing everywhere;
  HKDF-labeled key derivation; nonce-never-reuse construction bound to monotonic txn ids.
- **DON'T DO**: no signing-key-as-AEAD-key; no silent plaintext fallback when a class demands
  encryption; no fsck that invents data; no success markers before verified mount/commit; no
  path-string trust from clients.
- **Open risks**: no sealed key storage on current targets (documented above); side channels
  (timing) out of scope for v1; quota/DoS pressure handled by bounded caps in v1, real per-subject
  quotas arrive with the TASK-0133 model applied to `/data` (tracked, not claimed).

## Failure model (normative)

- Error codes are the RFC-0072 stable storage error SSOT; nxfs maps internally:
  `EINTEGRITY` (checksum/AEAD failure), `ENOSPC`, `ENOTFOUND`, `EEXIST`, `ENOTDIR`, `EISDIR`,
  `E2BIG` (cap exceeded / inline overflow), `EACCES`, `EBUSY` (open-handle conflict on remove),
  `EUNSUPPORTED` (phase-gated capability). Every code has at least one deterministic negative test.
- Replay: uncommitted txns are discarded silently (that is the contract, logged at info);
  a corrupt journal record ends replay at the last valid commit — later records are dropped and
  reported (`nxfsd: journal truncated at txn=N`), never half-applied.
- Mount: if both checkpoint slots are invalid → mount fails with `EINTEGRITY`, marker
  `nxfsd: mount failed (integrity)`; **no silent reformat**. Formatting is an explicit first-boot
  path gated on a blank partition signature.
- No silent fallback: if the GPT partition is missing, nxfsd reports `nxfsd: no partition (fail)` and
  stays down; it does not grab the whole device.

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p nxfs
cd /home/jenning/open-nexus-OS && cargo test -p fsck-nxfs
```

Deterministic crash-injection: write scripted op sequences against an image, truncate/corrupt at
every record boundary, replay, assert exact expected state (committed-only). Idempotent replay
(twice → same state). fsck exit-code matrix.

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os
# cold-boot persistence (Phase 2):
NEXUS_KEEP_BLK=1 <launcher> boot #1: write via stash/selftest → shutdown → boot #2: assert content
```

### Deterministic markers (if applicable)

- `nxfsd: mounted /data (rw, gen=<n>)`
- `SELFTEST: nxfs txn atomic ok`
- `SELFTEST: nxfs integrity deny ok`
- `nxfs: persisted across cold boot` (keep-blk harness)
- Phase 4: `nxfsd: encryption on (device-class)` / `nxfsd: encryption off`

## Alternatives considered

- **Extend statefs into a file store** — rejected: wrong data model (KV, 8 KiB IPC values, full-RAM
  map), and it would destabilize the proven boot-critical store (ADR-0043).
- **Port an existing FS (littlefs/ext2/FAT)** — rejected: none give CoW/snapshots/integrity/
  encryption-classes on our roadmap; foreign unsafe/no_std C code violates the userspace Rust
  discipline; FAT/ext2 have no integrity story. We already own a journal engine pattern (statefs)
  to grow from.
- **securefsd encrypted overlay on top of /state (TASK-0182/0183)** — superseded: encryption
  becomes a native nxfs volume class; an overlay would duplicate journal/atomicity logic and keep
  the 8 KiB value ceiling.
- **Separate second block *device* for nxfs** — rejected in ADR-0044 (future-proof = one device +
  GPT, like real hardware; migration is free while `blk.img` is recreated per boot).
- **New broker daemon between vfsd and nxfsd** — rejected: vfsd's provider + namespace model
  (RFC-0042) is exactly that seam already.

## Open questions

- Phase 3 metadata tree: B-tree vs. sorted-run/LSM hybrid for the object table at CoW time
  (owner: @runtime; decide before Phase 3 task is seeded — narrow ADR).
- Group-commit flush interval default (5 ms vs 20 ms) — measure on virtio-blk in TASK-0293.
- `/data` top-level layout policy (per-app home dirs vs shared user tree) — belongs to RFC-0073's
  namespace policy; nxfs itself is layout-agnostic.

## RFC Quality Guidelines (for authors)

When writing this RFC, ensure:

- Scope boundaries are explicit; cross-RFC ownership is linked.
- Determinism + bounded resources are specified in Constraints section.
- Security invariants are stated (threat model, mitigations, DON'T DO).
- Proof strategy is concrete (not "we will test this later").
- If claiming stability: define ABI/on-wire format + versioning strategy.
- Stubs (if any) are explicitly labeled and non-authoritative.

---

## Implementation Checklist

**This section tracks implementation progress. Update as phases complete.**

- [ ] **Phase 0**: contract + format v1 frozen (this doc reviewed) — proof: review + cross-links live
- [ ] **Phase 1**: host-first engine + fsck — proof: `cargo test -p nxfs` / `cargo test -p fsck-nxfs` (TASK-0292)
- [ ] **Phase 2**: nxfsd + GPT partition + `/data` RW mount + cold-boot persistence — proof: markers `nxfsd: mounted /data (rw, gen=<n>)`, `nxfs: persisted across cold boot` (TASK-0293)
- [ ] **Phase 3**: CoW tree + snapshots/clones + data checksums — proof: new task (seed-when-ready)
- [ ] **Phase 4**: encryption classes — proof: new task (seed-when-ready), markers `nxfsd: encryption on (device-class)`
- [ ] Task(s) linked with stop conditions + proof commands.
- [ ] QEMU markers appear in `scripts/qemu-test.sh` and pass.
- [ ] Security-relevant negative tests exist (`test_reject_*`: integrity, path canonicalization, unsupported-capability, oversize).
