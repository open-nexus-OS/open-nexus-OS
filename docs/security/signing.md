---
schema: 1
title: "Signing and trust anchors"
role: guide
subsystem: security
sources: [policies, userspace/updates, source/boot/nxboot, source/services/bundlemgrd/src/volume.rs, source/services/keystored, source/services/updated]
contracts: [ADR-0059, ADR-0060, ADR-0081, RFC-0089, RFC-0088, RFC-0016]
verified: 2026-11-03
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Signing and trust anchors

> **Purpose** — which signatures the device checks, against which key list, at which step of boot or update, and where signing happens.
> **Owns** — `policies/os-trust.toml`, `policies/update-trust.toml`, `policies/nxra-trust.toml`, `userspace/updates/build_trust.rs`
> **Contracts** — [ADR-0059](../adr/0059-first-stage-boot-chain-nxboot-handoff.md) (nxboot verifies the boot image), [ADR-0081](../adr/0081-the-boot-image-is-one-verified-container-kernel-plus-bootfs-every-core-program-runs-from-it-no-program-embeds-another.md) (one verified container), [ADR-0060](../adr/0060-verified-system-volume-bundlemgrd-verifier-init-spawner.md) (bundlemgrd verifies the system volume), [RFC-0089](../rfcs/RFC-0089-ota-v2-component-manifest-ab-boot-images-nxboot-bsb.md) (OTA v2, publisher anchor), [RFC-0088](../rfcs/RFC-0088-nxra-signed-recovery-action-tokens.md) (`.nxra` recovery tokens)
> **Invariants** — trust anchors are baked at build time and a malformed anchor fails the build; there is no runtime path that adds a key. A key read from the artifact being verified is a lookup hint, never a trust input. Every verifier fails closed.
> **Read next** — [policyd.md](policyd.md), [identity.md](identity.md), [../packaging/supply-chain.md](../packaging/supply-chain.md), [../updates/README.md](../updates/README.md)

## The chain

```text
nxboot ── verifies NXBD (boot descriptor) ── os-trust.toml
  └─ boot image (kernel + bootfs, ADR-0081), measured into the handoff (sha256, rollback_index)
       └─ bundlemgrd ── verifies NXSV (system volume) ── os-trust.toml (same keys)
            ├─ NXSV binds: boot_image_sha256 + rollback_index of the measured image, index sha256
            └─ every served payload.elf hashes to its pkgimg v3 index digest (lazy, per bundle)
updated ── verifies .nxs v2 manifest ── update-trust.toml   (via keystored OP_VERIFY)
  └─ embedded NXBDs are OS-key signed; nxboot checks them again on the next boot
bootctld ── verifies .nxra break-glass tokens ── nxra-trust.toml
```

Apps and services on the OS are not individually signed. They are trusted because the signed NXSV binds the volume index, and the index binds each bundle's digest.

## Trust anchors

| File | Baked into | Accepts | Format |
|---|---|---|---|
| `policies/os-trust.toml` | nxboot (`source/boot/nxboot/build.rs`), bundlemgrd (`BAKED_OS_KEYS`) | NXBD and NXSV signatures | `[[publisher]] pubkey = "<64 hex>"` |
| `policies/update-trust.toml` | `updates::trust::BAKED_PUBLISHERS` (`userspace/updates/build.rs`) | `.nxs` v2 manifest signatures | same |
| `policies/nxra-trust.toml` | the `nxra` crate (`userspace/nxra/build.rs`), linked by bootctld | `.nxra` tokens, per key an `actions` list (`slot-switch`, `target-set`, `fsck-repair`, `reset`) | `[[key]] pubkey`, `actions` |

The first two share one narrow parser, `userspace/updates/build_trust.rs`. The shipped entries are proof keys (deterministic dev seeds). The comments in each file say so, and a production image replaces the list. The anchor is only as strong as the integrity of the image that carries it; there is no hardware root yet.

## Who signs

| Artifact | Signed by | Key |
|---|---|---|
| NXBD, NXSV | `nx image build --sign <seed>`, `nx image patch --sign <seed>` | OS-image Ed25519 seed (dev: `keys/dev-os-image.ed25519.seed`) |
| `.nxs` v2 OTA | `nx image ota --sign-publisher <seed> --sign-os <seed>`; fixtures: `nx image fixtures` | publisher seed (must be in `update-trust.toml`) + OS seed for embedded NXBDs |
| `.nxs` v1 system set | `tools/nxs-pack --key <hex>` | host tests only — see [../updates/system-set.md](../updates/system-set.md) |
| on-device payloads | keystored `OP_DEVICE_SIGN` | the device key ([identity.md](identity.md)) |

`nx image verify` re-parses a built disk and checks the NXBD signature and digest. Signing keys never enter the device: the device holds verifying keys only, plus its own device key in keystored.

## keystored's part

| Op | Use |
|---|---|
| `OP_VERIFY` (4) | Ed25519 verify with a caller-supplied key; `updated`'s `KeystoredVerifier` uses it for the OTA manifest |
| `OP_DEVICE_SIGN` (12) | signs with the device key; needs `crypto.sign` (or `crypto.derive.statefs` for the statefs key-derivation label) |
| `OP_SIGN` (5) | checks `crypto.sign`, then answers `STATUS_UNSUPPORTED` — there is no service signing key |

## Host-only pieces

These exist only in host builds (`std`, Cap'n Proto). The OS image does not contain them:

- keystored `full_impl.rs`: loads anchors from `recipes/keys/` (`NEXUS_ANCHORS_DIR`) and a publisher allowlist from `recipes/signing/publishers.toml` (`NEXUS_SIGNING_ALLOWLIST`), and answers `IsKeyAllowed`.
- bundlemgrd `std_server.rs`: verifies a `.nxb` manifest signature through keystored `Verify` + `IsKeyAllowed`. Without a keystore client it skips verification and logs that.
- policyd `supply_chain.rs`: maps an allowlist answer to an install decision label.

`tools/nxb-pack` copies `publisher` / `signature` hex from the manifest TOML into the `.nxb`. It does not sign, and accepts all-zero placeholders.

## What does not exist

None of these exist: an enforcement mode (`policy.enforcement=`), `/etc/nexus/policy.conf`, `dev-mode` / `bring-up` features, `keystored generate-key`, `policyd add-publisher`, `nxb-pack --sign-key`, a sideload consent flow. `recipes/policy/` is a legacy note (`recipes/policy/README.md`); capability policy lives in `policies/` ([policyd.md](policyd.md)).

## Proof

- Host: `cargo test -p updates`, `tests/updates_host/`, `tests/nxra_host/`, `source/boot/nxboot/tests/`.
- QEMU: `bundlemgrd: system volume verified …`, `bootctld: nxra accept (…)` / `SELFTEST: nxra accept ok`, the `updated:` component lines of the OTA lanes.
