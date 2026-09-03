# RFC-0090: `.nxdelta` v1 — Boot-Image Delta Stream Format (`boot-image-delta` component kind)

- Status: Implemented (TASK-0034)
- Owners: @runtime @tools-team
- Seed: RFC-0089 §11 (the reserved delta seam); TASK-0034 (execution ledger)
- Related: RFC-0089 (container/trust/staging/machine — all UNCHANGED here),
  ADR-0058/0059 (BSB/loader — untouched), RFC-0009 (dependency hygiene, §Algorithms)

## Context

RFC-0089 §11 fixes the shape: delta updates are `.nxs` v2 **component kinds**, not a
new container. This RFC is the normative stream format for the first kind,
`boot-image-delta` (kind **3**; kind 2 stays reserved for `bundle`, §3 table): the
device reconstructs the target boot image from the ACTIVE slot's bytes plus a delta
stream, into the INACTIVE slot — then the identical digest/readback/NXBD-last tail
runs. Manifest, trust anchor, staging, anti-downgrade and machine paths are byte-for-
byte the RFC-0089 paths; that is the component-model payoff this format cashes in.

## Scope boundaries (anti-drift)

- IN: the `.nxdelta` v1 stream format; the `boot-image-delta` manifest-component
  contract; the device apply semantics (base binding, bounds, reject vocabulary);
  host emission (`nx image ota --delta-from`).
- OUT: `bundle-delta` and multi-component orchestration (Phase B seam, RFC-0089 §12;
  TASK-0035). Network transport. Checkpoint FILES (see §Resume). Kernel/loader/BSB.

## The component contract (normative)

A `boot-image-delta` component rides the `.nxs` v2 manifest exactly like
`boot-image`, with one deliberate reinterpretation:

- `kind` = **3**, `name` = `"boot-image-delta"`, `payloadPath` = `"boot.img.nxdelta"`.
- `size` / `sha256` describe the **delta stream** (the payload entry). The engine's
  existing streamed-digest gate therefore covers the delta bytes — **the delta
  payload is signature-bound (manifest Ed25519) before any decoder sees it**. A
  robust-but-unsigned decoder input path does not exist.
- `kindData` carries the signed 512-byte **NXBD of the TARGET image, verbatim** —
  the same descriptor a full-image container would carry. Target truth
  (`image_sha256`, `image_size`, `build_id`, `rollback_index`) lives there; the
  reconstruction is verified against it by the SAME readback path, and the NXBD
  lands last (commit point discipline unchanged).
- Binding (stage-time, before any byte): NXBD `build_id`/`rollback_index` must equal
  the manifest's (as for kind 1). The kind-1 rule `image_sha256 == component.sha256`
  is replaced by the base/target rules below — for kind 3 those two digests
  legitimately differ.

## `.nxdelta` v1 stream (normative, little-endian)

```
header (96 bytes):
  magic       [8]  = "NXDELTA1"
  version     u16  = 1
  algo        u8   = 0 (stored)     # 1 = zstd RESERVED, see §Algorithms
  reserved    [5]  = 0
  base_size   u64                   # byte length of the base image
  target_size u64                   # byte length of the reconstructed image
  base_sha256   [32]                # digest of the base image bytes
  target_sha256 [32]                # digest of the reconstructed image bytes

records (in order, until END):
  0x01 COPY  { base_off u64, len u32 }   # copy len bytes from base[base_off..]
  0x02 ADD   { len u32, bytes[len] }     # literal bytes (algo 0: stored)
  0xFF END   { out_total u64 }           # closes the stream

trailer: nothing after END — trailing bytes are a format error.
```

Bounds (normative; enforced before any effect):

- record `len` ∈ [1, 4 MiB]; `base_off + len <= base_size`;
- running output total never exceeds `target_size`; at END it must EQUAL both
  `target_size` and `out_total`;
- unknown tag, short read, `version != 1`, unsupported `algo`, or bytes after END ⇒
  reject `delta-format`;
- header `target_size`/`target_sha256` must equal the NXBD's `image_size`/
  `image_sha256` (the stream and the descriptor never disagree) ⇒ else `delta-format`.

The decoder is a bounded streaming state machine: it needs only a
record-header-sized carry buffer; ADD payload streams through without being
buffered whole. No allocation per record (os-service bump-heap discipline).

## Base binding (normative — the O(1) trick)

The apply path never hashes the base to check it. The ACTIVE slot's NXBD sector —
**loader-verified this very boot** — already carries `image_sha256`/`image_size` of
the running image. Apply reads that one sector and requires:

- header `base_sha256 == active NXBD.image_sha256` and
  `base_size == active NXBD.image_size` ⇒ else reject **`delta-base`** before any
  write reaches the inactive slot.

COPY reads then address the active slot's body (`IMAGE_START_SECTOR` + byte
offset) through the same partition-scoped block service the readback verify
already uses; virtioblkd's gate scopes the SENDER (`updated`), the engine scopes
the slot (writes go only to the inactive one).

## Reject vocabulary (additive; RFC-0089 §8 list grows)

`delta-format` (code 10) · `delta-base` (code 11). All other rejects
(`untrusted publisher | sig | digest | bounds | path | component kind unsupported |
downgrade | io | slot-active`) keep their meaning: a tampered delta stream fails
the SIGNED stream digest as `digest`, exactly like a tampered full image.

## Determinism (normative for emission)

`nx image ota --delta-from <base>` emits byte-identical output for identical
inputs: fixed 4096-byte base blocks indexed by rolling checksum (rsync-shape
`(a,b)` sums) + SHA-256 confirmation, greedy left-to-right target scan, adjacent
COPY coalescing, ADD flushed on match, stable header fields, no wall clock.
Emitting twice ⇒ identical `.nxdelta` bytes and (with the deterministic tar rules
of RFC-0089 §3) identical `.nxs` bytes.

## Resume

None of the RFC-0089 §8 idempotency rules change: staging is restartable at any
cut point, a torn apply leaves the inactive slot NXBD-invalid, and a re-stage of
the same container converges. For a boot image (tens of MB over the block plane)
re-running the stage IS the resume story — v1 defines **no checkpoint files**.
Per-component checkpointing returns with Phase-B multi-component orchestration
(TASK-0035) if the payload economics ever demand it.

## Algorithms (honest v1 boundary)

`algo = 0` (stored ADD bytes) is the only v1 algorithm. The bandwidth win comes
from COPY coverage of unchanged regions; compressing the ADD residue is a real
but second-order gain. `algo = 1` (zstd) is RESERVED: enabling it on-device means
admitting a decompressor into the update trust path of a `no_std` service — an
RFC-0009 D4 allowlist decision (a pure-Rust `no_std` decoder such as `ruzstd`)
that deserves its own review, not a side effect of the format RFC. Host-side
`zstd` stays legal for tooling.

## Security considerations

- The delta payload is covered by the manifest signature (component `sha256`)
  BEFORE decoding — the decoder never parses unsigned attacker bytes short of a
  publisher-key compromise, and it is bounds-checked as if it did anyway.
- Base substitution is closed by the `delta-base` binding to the loader-verified
  active NXBD; reconstruction-from-wrong-base cannot silently commit because the
  readback digest gate (target `image_sha256`) still guards the NXBD-last write.
- Anti-downgrade is unchanged: the manifest `rollbackIndex` gates at stage time,
  commit raises the floor, the loader backstops (RFC-0089 §10).

## Proof obligations (TASK-0034 DoD)

- Host: make/apply byte-identity, determinism (make twice ⇒ identical bytes),
  tamper ⇒ `digest`, base-mismatch ⇒ `delta-base`, bounds matrix ⇒ `delta-format`,
  power-cut convergence via the existing SlotFake matrix.
- QEMU (gated): `SELFTEST: ota delta stage ok` (a real delta fixture reconstructs
  the os-B image into the inactive slot through the full engine) and
  `SELFTEST: ota delta base deny ok` (a wrong-base delta rejects `delta-base`
  state-neutrally). Boot-equivalence: the reconstructed bytes are digest-identical
  to the full-image os-B container the flip lane boots.
