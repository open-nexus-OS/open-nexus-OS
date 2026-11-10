---
schema: 1
title: "Updates (OTA)"
role: guide
subsystem: updates
sources: [source/services/updated, source/services/bootctld, userspace/updates, tools/nx/src/commands/update.rs, tools/nx/src/cli_update.rs, userspace/apps/settings/ui/components/sections/UpdatesView.nx]
contracts: [RFC-0089, RFC-0090, RFC-0012]
verified: 2026-11-09
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Updates (OTA)

> **Purpose** — how a developer drives the OTA engine: the `nx update` host CLI over built images, and the Settings update page over the live services.
> **Owns** — `source/services/updated/`, `userspace/updates/`, `tools/nx/src/commands/update.rs`, `userspace/apps/settings/ui/components/sections/UpdatesView.nx`
> **Contracts** — [RFC-0089](../rfcs/RFC-0089-ota-v2-component-manifest-ab-boot-images-nxboot-bsb.md) (`.nxs` v2 component manifest, A/B boot images, nxboot, boot selection block), [RFC-0090](../rfcs/RFC-0090-nxdelta-boot-image-delta-stream-format.md) (`.nxdelta` streams), [RFC-0012](../rfcs/RFC-0012-updates-packaging-ab-skeleton-v1.md) (the v1 `.nxs` system set)
> **Invariants** — commit happens only through the health quorum, never through a button. Stage/switch/rollback need `updates.manage` on the kernel-attributed sender. Neither surface holds update logic of its own.
> **Read next** — [delta.md](delta.md), [system-set.md](system-set.md)

## Shape

| piece | role |
|---|---|
| `updated` | verifies `.nxs` v2 containers from `/updates/*.nxs` on the data volume, writes the inactive slot, forwards status |
| `bootctld` | single boot-state authority: slots, trial, tries, health commit, rollback floor |
| `userspace/updates/` | the engine: `component_set.rs` (verify + apply), `volume_apply.rs` (system volume), `delta_apply.rs`, `bundle_delta.rs`, `stage_journal.rs` |
| `nx update` | host CLI over a built disk image; same codecs and engine |
| Settings update page | reads and drives the live services |

There is no host↔guest transport: the CLI works on built images, the page on the live OS.

## Rules

- **Commit = health quorum.** A switch arms a bounded trial (`tries=2`). The commit fires only when the full declared quorum reports health after the reboot (RFC-0089 §13).
- **Mutation is policy-gated.** `updated` asks policyd whether the sender holds `updates.manage` (`source/services/updated/src/manage_gate.rs`). Reads stay open. The Settings page holds the route but not the grant, so its action rows render the DENIED state.

## `nx update` (host, offline)

```bash
nx update status   [--image build/nexus.img] [--key <seed> | --pubkey <hex>] [--json]
nx update check    [--image build/nexus.img | --data <data.img>] [--json]
nx update stage <container.nxs> [--image build/nexus.img | --data <data.img>] [--json]
nx update switch   [--image build/nexus.img] [--tries 2] [--json]
nx update rollback [--image build/nexus.img] [--json]
```

| verb | does |
|---|---|
| `status` | decodes the BSB and both slot NXBDs with the `bootfmt` codecs nxboot/bootctld use; without a key descriptors report `sig=unchecked` |
| `check` | lists `/updates/*.nxs` on the data volume and verifies each through the device engine against the baked anchor and the disk's anti-downgrade floor |
| `stage` | verifies first, then writes into `/updates/`; rejects: `untrusted publisher`, `sig`, `digest`, `bounds`, `path`, `component kind unsupported`, `downgrade`, `io` |
| `switch`, `rollback` | preflights only (`applied=false`); the live transitions belong to `updated`/bootctld |

`--data` takes a standalone nxfs data image and skips the floor gate. After a QEMU OTA lane, `nx update status --image build/nexus.img` decodes the state the live machine left; the `ota-flip` profile gates on it.

## Component kinds

| kind | name | payload |
|---|---|---|
| 1 | `boot-image` | full boot image + embedded NXBD |
| 2 | `bundle` | one system-volume index window (its sha256 is the index's bundle digest) |
| 3 | `boot-image-delta` | `.nxdelta` stream against the active boot slot |
| 4 | `bundle-delta` | `.nxdelta` stream against one bundle window of the active volume |
| 6 | `system-volume` | pkgimg superblock + index, signed NXSV as `kindData`, bound to the set's boot digest |

## Bundle sets (system volume travels with the flip)

A set is `[boot-image | boot-image-delta]?, system-volume, (bundle | bundle-delta)*` (RFC-0089 §12). The device assembler (`userspace/updates/src/volume_apply.rs`) writes the inactive system slot:

1. index first;
2. shipped bundle windows, read back and verified;
3. bundles the set did not ship are copied from the active volume (`updated: bundle reused (name=…)`);
4. the whole volume read back against `volume_sha256`;
5. NXSV last — a valid NXSV exists only over a complete volume.

Extra rejects: `order`, `volume-binding`, `bundle-not-in-index`, `volume-digest`.

The volume carries the app bundles and the `system` data bundle, so `pkg:/`, the launcher grid and `GET_PAYLOAD` all come from one NXSV-verified index. A boot-image update on a volume system ships `system-volume` next to the boot image; unchanged bundles are reused.

A torn stage resumes: the `NXSJ` journal at sector 1 of the inactive system slot records verified windows; the next stage of the same set rewrites only the rest (`updated: restage resume (bundles=k/N)`). A valid NXSV zeroes the journal.

Emission:

| command | emits |
|---|---|
| `nx image ota --bundle-set <dir>` | the full set from bundle directories |
| `nx image ota --bundle-set <dir> --reuse-from <active.img>` | only bundles whose window digest changed |
| `nx image ota --bundle-set <dir> --delta-from-volume <active>` | changed bundles as `bundle-delta` (see [delta.md](delta.md)) |
| `nx image fixtures --system-bundles <dir>` | `/updates/bundle-set.nxs` fixture |

Lanes: `just ci-os-ota`, `just ci-os-ota-bundle`, `just ci-os-ota-bundle-resume`, `just ci-os-ota-bundle-delta`. The bundle lane proves `SELFTEST: ota bundle-set ok` on the second boot.

## Settings update page (live)

`UpdatesView.nx` with `userspace/apps/settings/ui/composables/updates.store.nx` reads `svc.updates.status`, `svc.updates.feed` and `svc.updates.check`. Active slot, pending trial, tries left, health commit, rollback floor and staged build are service truth, never derived client-side. Actions call stage/switch/rollback and render what comes back: success, the engine's reject, or DENIED.

| proof | where |
|---|---|
| page logic | `tests/dsl_apps_conformance/tests/settings.rs` |
| live read surface | `SELFTEST: updates surface ok` |
| engine | `tests/updates_host/tests/` |
