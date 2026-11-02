---
schema: 1
title: bundlemgrd
role: guide
subsystem: app-platform
sources: [source/services/bundlemgrd, source/libs/nexus-wire/src/bundlemgrd.rs, scripts/system-volume-services.txt]
contracts: [ADR-0060, RFC-0089, ADR-0080, ADR-0084, RFC-0012, ADR-0044]
verified: 2026-11-02
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# bundlemgrd

> **Purpose** — what the bundle authority verifies and serves, to whom, and over which ops.
> **Owns** — `source/services/bundlemgrd/`; wire in `source/libs/nexus-wire/src/bundlemgrd.rs`
> **Contracts** — [ADR-0060](../adr/0060-verified-system-volume-bundlemgrd-verifier-init-spawner.md) (verified system volume), [RFC-0089](../rfcs/RFC-0089-ota-v2-component-manifest-ab-boot-images-nxboot-bsb.md) §12, [ADR-0080](../adr/0080-a-process-runs-from-a-sealed-image-text-mapped-from-its-frames-data-copied-no-other-exec-path.md) (sealed images), [ADR-0084](../adr/0084-one-permission-authority-policyd-holds-every-app-grant-published-by-the-bundle-manager-asked-by-the-launchers.md) (app grants), [RFC-0012](../rfcs/RFC-0012-updates-packaging-ab-skeleton-v1.md)
> **Invariants** — nothing is served before the volume verified against the baked OS anchor; bytes are hashed against the index digest before the header is written (header last); only init may pull service images.
> **Read next** — [../packaging/nxb.md](../packaging/nxb.md), [../architecture/04-bundlemgr-manifest.md](../packaging/nxb.md), [../boot-init/nexus-init.md](../boot-init/nexus-init.md), [../boot-init/execd.md](../boot-init/execd.md)

## Role

bundlemgrd is a CORE service (in the bootfs, never on the volume). It is the one verifier and
reader of the system volume (`system-a/b`) and the installed-app registry. Nothing about apps
is baked into its binary.

Non-goals: parallel bundle formats (`manifest.nxb` is canonical — see
[nxb.md](../packaging/nxb.md)); policy decisions (policyd owns them).

## System volume

On the first volume op bundlemgrd:

1. attaches the partition paired with the **measured** boot slot over the block plane;
2. verifies the NXSV against the baked anchor (`policies/os-trust.toml`, the key nxboot holds),
   binds it to the booted image digest and rollback line;
3. reads the bounded pkgimg v3 index.

Marker: `bundlemgrd: system volume verified (slot=<s> build=<id8> bundles=N)`; any failure is
`… FAIL (<reason>)` and nothing is served.

## Ops

| Op | Who may call | What |
|---|---|---|
| `QUERY_BUNDLE` | init + allowlist | size + launch params of a volume service |
| `GET_BUNDLE_IMAGE` | init only | the service's `payload.elf` read into the caller's armed VMO in one block-plane round trip ([ADR-0044](../adr/0044-single-blk-device-gpt-partitions-block-layer.md) `OP_READ_VMO`), sealed, hashed against the index digest, header last (`bundlemgrd: bundle served (name=…)`) |
| `VOLUME_STATUS` | init + allowlist | volume state |
| `GET_INDEX` | init + allowlist (packagefsd) | the verified index, from which packagefsd derives `pkg:/` |
| `GET_FILE_VMO` | init + allowlist (packagefsd) | one file into the caller's VMO, on demand |
| `LIST_APPS` | allowlist + installed app-hosts | the registry (launcher grid) from each bundle's `meta/app.properties` |
| `GET_PAYLOAD` | allowlist (execd) | an app's `payload.nxir` (ui program) into the armed VMO |
| `OP_ARM_VMO` | VMO callers | arms the destination VMO before the request |
| `OP_SET_ACTIVE_SLOT` | allowlist (updated) | emits `bundlemgrd: slot <a|b> active`; re-publishes nothing — the served volume is the measured slot's until reboot |
| `FETCH_IMAGE` | — | retired: answers `UNSUPPORTED` |

Callers are identified by kernel `sender_service_id`; the allowlist is the boot-safe set in `src/os_lite.rs` (`is_allowed_sender`); app-hosts are recognized by the
`app:<bundle_id>` name execd gives them (`src/app_sender.rs`), derived from the same registry.

## App grants

Once per boot, after policyd's leg is pinned and before the first answer that follows,
bundlemgrd publishes every installed app's manifest grants to policyd (`src/grants_publish.rs`,
ADR-0084). A refused publish is `bundlemgrd: FAIL app grants (app= status=)`; that app stays
unpublished and policyd denies it.

## Code map

| File | Role |
|---|---|
| `src/volume.rs` | attach, verify, index, bundle lookup |
| `src/payload_ops.rs` | VMO-serving ops (header-last contract) |
| `src/grants_publish.rs` | app grants → policyd |
| `src/app_sender.rs` | app-host sender recognition (host-tested) |
| `src/os_lite.rs` | main loop, frame routing |
| `src/std_server.rs` | host backend |

## Proof

Host-first tests in the crate; QEMU markers above (the `ota-bundle` lane cross-checks a flipped
volume). When you change manifest fields, trust gating, publication or exec handshakes, update
the owning task's proof and [04-bundlemgr-manifest.md](../packaging/nxb.md).
