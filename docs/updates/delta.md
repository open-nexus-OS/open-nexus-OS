# Delta updates v1 — `.nxdelta` boot-image deltas (TASK-0034, RFC-0090)

<!--
CONTEXT: How boot-image delta updates work end to end: emission
(`nx image ota --delta-from`), the component contract (kind 3 riding the
unchanged `.nxs` v2 pipeline), device reconstruction from the ACTIVE
slot, and the honest boundaries (stored ADDs, no checkpoint files).
OWNERS: @runtime @tools-team
STATUS: Functional
API_STABILITY: Unstable
ADR: docs/rfcs/RFC-0090-nxdelta-boot-image-delta-stream-format.md
-->

Deltas are `.nxs` v2 **component kinds**, not a new container (RFC-0089 §11):
a `boot-image-delta` (kind 3) component's payload is the RFC-0090 `.nxdelta`
stream; manifest, trust anchor, staging, anti-downgrade and machine paths are
byte-for-byte the full-image paths.

## Emission (host)

```bash
nx image ota --kernel new-boot.bin --delta-from old-boot.bin \
  --out update.nxs --sign-publisher pub.seed --sign-os os.seed \
  --build-id dev-X --rollback-index 3
```

`--delta-from` names the image the device is RUNNING — the stream binds to
its digest. Emission is deterministic (rollsum block index, greedy scan,
COPY coalescing): emitting twice yields identical bytes. An append-shaped
change (the os-B fixture shape) deltas from ~19 MB down to kilobytes.

## Device apply

The engine's signed-digest gate covers the DELTA bytes (the component's
`sha256` describes the stream — no unsigned input ever reaches the
decoder). The stream's `base_sha256` must equal the **ACTIVE slot NXBD's**
`image_sha256` — loader-verified this very boot, checked in O(1); a
mismatch rejects **`delta-base`** before any write. Reconstruction (COPY
reads from the active slot + stored ADD literals) flows into the inactive
slot through the UNCHANGED readback-digest gate (target truth from the
component's NXBD) and NXBD-last commit discipline. Malformed streams
reject **`delta-format`**; a tampered stream fails the signed digest as
`digest`, exactly like a tampered full image.

## Bundle deltas (kind 4, TASK-0035 P3)

On the system-volume seam (RFC-0089 §12) the same `.nxdelta` stream carries
ONE changed bundle: `nx image ota --bundle-set <dir> --delta-from-volume
<active.img | dir>` emits, for every bundle whose window changed and that has a
same-named predecessor on the device's volume, a `bundle-delta` component
(`bundles/<name>@<ver>.nxdelta`, `kindData` = the base window's sha256);
unchanged bundles are reused, bundles new to the volume ship in full. On the
device the base is looked up in the ACTIVE volume's index BEFORE any write
(`delta-base` otherwise), the stream header's target is bound by the
assembler to the NEW index (`bundle-not-in-index` otherwise), and the
reconstructed window takes the ordinary readback-verified, journalled path —
`updated: component bundle-delta reconstructed (name=…)` followed by
`component bundle verified`. The `ota-bundle-delta` lane proves the flip with
metricsd shipped as a delta (`SELFTEST: ota bundle delta ok`); host:
`tests/updates_host/tests/component_set_bundle_delta.rs`.

## Honest boundaries

- **Resume = idempotent restage** (RFC-0089 §8): a torn apply leaves the
  slot NXBD-invalid and a re-stage converges. v1 keeps **no checkpoint
  files** — for a boot image, re-running the stage is the resume story.
- **ADD payloads are stored** (`algo` 0). zstd is reserved (`algo` 1)
  behind an RFC-0009 D4 decision — the COPY coverage is where the
  bandwidth win lives.
- `bundle-delta` (kind 4) and the bundle-set orchestration are live on the
  Phase-B seam (RFC-0089 §12, TASK-0035) — see „Bundle deltas“ above; for
  bundles the NXSJ journal (TASK-0035 P1) makes a torn stage resume per
  window, boot images keep the idempotent-restage story.

## Proof surfaces

Host: `tests/nxdelta_host` (format: roundtrip/determinism/tamper/bounds)
and `tests/updates_host/tests/component_set_delta.rs` (the REAL engine
driving the REAL adapter: reconstruction byte-identity, `delta-base`
before any write, truncation ⇒ `delta-format`, power-cut restage).
QEMU (gated in headless/smp1): `updated: stage rejected (delta-base)` →
`SELFTEST: ota delta base deny ok` → `updated: component
boot-image-delta verified` → `SELFTEST: ota delta stage ok` — a real
reconstruction whose COPY window reads back the active slot's bytes.
Boot-equivalence: the reconstructed image is digest-identical to what the
flip lane boots.
