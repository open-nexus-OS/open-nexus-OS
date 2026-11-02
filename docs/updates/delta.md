---
schema: 1
title: "Delta updates"
role: guide
subsystem: updates
sources: [userspace/nxdelta, userspace/updates/src/delta_apply.rs, userspace/updates/src/bundle_delta.rs, tools/nx/src/commands/image_ota.rs, source/services/updated/src/delta_os.rs]
contracts: [RFC-0090, RFC-0089]
verified: 2026-11-02
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Delta updates

> **Purpose** — how `.nxdelta` deltas are emitted on the host and reconstructed on the device, for boot images (kind 3) and system-volume bundles (kind 4).
> **Owns** — `userspace/nxdelta/`, `userspace/updates/src/delta_apply.rs`, `userspace/updates/src/bundle_delta.rs`
> **Contracts** — [RFC-0090](../rfcs/RFC-0090-nxdelta-boot-image-delta-stream-format.md) (the `.nxdelta` stream format), [RFC-0089](../rfcs/RFC-0089-ota-v2-component-manifest-ab-boot-images-nxboot-bsb.md) (the `.nxs` v2 container a delta rides in)
> **Invariants** — a delta is a component kind, not a new container. The signed digest covers the delta bytes, so no unsigned input reaches the decoder. The base must be the active slot, checked before any write.
> **Read next** — [README.md](README.md)

## Emission (host)

```bash
nx image ota --kernel new-boot.bin --delta-from old-boot.bin \
  --out update.nxs --sign-publisher pub.seed --sign-os os.seed \
  --build-id dev-X --rollback-index 3
```

- `--delta-from` names the image the device is running; the stream binds to its digest.
- Emission is deterministic (rollsum block index, greedy scan, COPY coalescing): same inputs, same bytes.
- The publisher key must be in the device anchor (`policies/update-trust.toml`) for the container to stage.

## Device apply — boot image (kind 3)

| step | check | reject |
|---|---|---|
| 1 | component `sha256` over the delta stream (signed manifest) | `digest` |
| 2 | stream `base_sha256` == active slot NXBD `image_sha256` (loader-verified this boot, O(1)) | `delta-base` |
| 3 | stream decodes within bounds | `delta-format` |
| 4 | reconstruction (COPY from active slot + stored ADD literals) into the inactive slot, read back against the component's NXBD | `digest` |
| 5 | NXBD written last | — |

## Bundle deltas (kind 4)

`nx image ota --bundle-set <dir> --delta-from-volume <active.img | dir>` emits, for each bundle whose window changed and that has a same-named predecessor on the device volume, a `bundle-delta` component (`bundles/<name>@<ver>.nxdelta`, `kindData` = the base window's sha256). Unchanged bundles are reused; new bundles ship in full.

On the device:

- the base is looked up in the active volume's index before any write (`delta-base` otherwise);
- the stream's target must be in the new index (`bundle-not-in-index` otherwise);
- the rebuilt window takes the normal readback-verified, journalled bundle path: `updated: component bundle-delta reconstructed (name=…)`, then `updated: component bundle verified (name=…)`.

## Limits

| topic | behaviour |
|---|---|
| resume, boot image | idempotent restage: a torn apply leaves the slot NXBD-invalid; staging again converges (no checkpoint files) |
| resume, bundles | per window through the `NXSJ` stage journal |
| ADD payloads | stored (`algo` 0, `nxdelta::ALGO_STORED`); zstd (`algo` 1) is reserved behind an RFC-0009 decision |

## Proof

| layer | where |
|---|---|
| format | `tests/nxdelta_host` (roundtrip, determinism, tamper, bounds) |
| engine, boot image | `tests/updates_host/tests/component_set_delta.rs` (byte identity, `delta-base` before any write, truncation → `delta-format`, power-cut restage) |
| engine, bundle | `tests/updates_host/tests/component_set_bundle_delta.rs` |
| QEMU, boot image | `updated: stage rejected (delta-base)` → `SELFTEST: ota delta base deny ok` → `SELFTEST: ota delta stage ok` |
| QEMU, bundle | `just ci-os-ota-bundle-delta` → `SELFTEST: ota bundle delta ok` |
