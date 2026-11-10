---
schema: 1
title: "Bundle supply chain"
role: reference
subsystem: updates
sources: [tools/sbom, tools/repro, tools/nxb-pack, source/services/keystored/src/full_impl.rs, source/services/bundlemgrd/src/std_server.rs, policies]
contracts: [RFC-0039, ADR-0021, RFC-0089]
verified: 2026-11-10
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Bundle supply chain

> **Purpose** — the per-bundle SBOM and repro metadata, the tools that make and check them, and which trust anchors sign what.
> **Owns** — `tools/sbom/`, `tools/repro/`, `recipes/signing/publishers.toml`, `policies/*-trust.toml`
> **Contracts** — [RFC-0039](../rfcs/RFC-0039-supply-chain-v1-bundle-sbom-repro-sign-policy.md) (SBOM, repro, sign policy v1), [ADR-0021](../adr/0021-structured-data-formats-json-vs-capnp.md) (JSON for interop artifacts), [RFC-0089](../rfcs/RFC-0089-ota-v2-component-manifest-ab-boot-images-nxboot-bsb.md) (OTA and volume signing)
> **Invariants** — Timestamps come only from `SOURCE_DATE_EPOCH`. Generated JSON passes a secret scan before it is written. Trust anchors are baked at build time; a malformed anchor file fails the build.
> **Read next** — [nxb.md](nxb.md)

## In every bundle

| file | format | bound by |
|---|---|---|
| `meta/sbom.json` | CycloneDX JSON, `specVersion` 1.5 | `manifest.nxb` `sbomDigest` (SHA-256) |
| `meta/repro.env.json` | JSON, `schema_version = 1` | `manifest.nxb` `reproDigest` (SHA-256) |

`nxb-pack` writes both on every pack ([nxb.md](nxb.md)). JSON is deliberate (ADR-0021); integrity
comes from the manifest digests.

## SBOM (`tools/sbom`)

- Inputs: bundle name/version, publisher, payload digest + size, manifest digest,
  `SOURCE_DATE_EPOCH` (missing or invalid fails generation).
- Output: stable key order, fixed timestamp, no wall-clock time.
- Secret scan (`nexus_evidence::scan_for_secrets_with`) runs before write; known digests are
  allowlisted so digest-like fields do not trip it. A hit fails the pack.

```bash
SOURCE_DATE_EPOCH=1700000000 cargo run -p sbom -- \
  <bundle_name> <bundle_version> <publisher_hex> \
  <payload_sha256> <payload_size> <manifest_sha256> <output_path>
```

## Repro metadata (`tools/repro`)

`ReproEnvV1` (`deny_unknown_fields`): `schema_version`, `source_date_epoch`, `payload_sha256`,
`manifest_sha256`, `sbom_sha256`, `rustc`, `target`, `rustflags`. Same secret scan as the SBOM.

```bash
cargo run -p repro -- capture <manifest.nxb> <payload.elf> <meta/sbom.json> <meta/repro.env.json>
cargo run -p repro -- verify  <meta/repro.env.json> <payload_sha256> <manifest_sha256> <sbom_sha256>
```

`verify` fails closed on a schema mismatch, an unknown field or a digest mismatch.

## Trust anchors

| file | signs | baked into | used by |
|---|---|---|---|
| `policies/os-trust.toml` | boot descriptors and the system volume's NXSV | `source/boot/nxboot/build.rs`, `source/services/bundlemgrd/build.rs` | nxboot; bundlemgrd verifying the volume |
| `policies/update-trust.toml` | update publishers (`.nxs`) | `userspace/updates/build.rs` | updated |
| `policies/nxra-trust.toml` | break-glass recovery tokens (RFC-0088) | `userspace/nxra/build.rs` | bootctld, statefsd |

Format of the first two: `[[publisher]]` blocks with one `pubkey = "<64 lowercase hex>"`
(shared parser `userspace/updates/build_trust.rs`). No runtime mutation path.

On the device, bundles are trusted through the volume signature, not per bundle: the
`publisher`/`sig` fields of the repo manifests are zero placeholders.

## Publisher allowlist (host build only)

`recipes/signing/publishers.toml` and the `IsKeyAllowed` call
(`tools/nexus-idl/schemas/keystored.capnp`) exist only in the host builds:

- keystored `full_impl.rs` compiles only with `idl-capnp` and not for the OS target. It loads the
  allowlist once at start (`NEXUS_SIGNING_ALLOWLIST` overrides the path).
- Format: `version = 1`, `[[publishers]]` with `id` (hex), `enabled`, `allowed_algs`, `keys`
  (several for rotation).
- bundlemgrd `std_server.rs` (host only) installs in the order verify → `IsKeyAllowed` → digest
  check → audit; any failure rejects the install, and an unreachable audit sink fails closed.
- policyd maps answers to stable labels (`source/services/policyd/src/supply_chain.rs`):
  `policy.publisher_unknown`, `policy.key_unknown`, `policy.alg_unsupported`, `policy.disabled`.

The OS bundlemgrd has no `IsKeyAllowed` path.

## Proof

```bash
cargo test -p sbom -- determinism
cargo test -p nxb-pack -- supply_chain
cargo test -p repro -- verify
cargo test -p keystored -- is_key_allowed
cargo test -p bundlemgrd -- supply_chain
cargo test -p bundlemgrd test_reject_unknown_publisher
cargo test -p bundlemgrd test_reject_payload_digest_mismatch
cargo test -p bundlemgrd test_reject_repro_schema_invalid
cargo test -p bundlemgrd test_reject_audit_unreachable
```
