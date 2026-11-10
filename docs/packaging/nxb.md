---
schema: 1
title: "Bundles (.nxb)"
role: reference
subsystem: app-platform
sources: [tools/nexus-idl/schemas/manifest.capnp, tools/nxb-pack, userspace/bundlemgr, source/services/bundlemgrd, tools/nx/src/commands/app.rs, userspace/storage/src/pkgimg_bundles.rs]
contracts: [ADR-0020, ADR-0021, ADR-0060, RFC-0089, RFC-0111, ADR-0084]
verified: 2026-11-10
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Bundles (.nxb)

> **Purpose** — the `.nxb` bundle layout, the `manifest.toml` → `manifest.nxb` fields, and how bundles become the system volume that `pkg:/` serves.
> **Owns** — `tools/nexus-idl/schemas/manifest.capnp`, `tools/nxb-pack/`, `userspace/bundlemgr/`
> **Contracts** — [ADR-0020](../adr/0020-manifest-format-capnproto.md) (Cap'n Proto manifest), [ADR-0021](../adr/0021-structured-data-formats-json-vs-capnp.md) (JSON only for interop metadata), [ADR-0060](../adr/0060-verified-system-volume-bundlemgrd-verifier-init-spawner.md) (verified system volume), [RFC-0089](../rfcs/RFC-0089-ota-v2-component-manifest-ab-boot-images-nxboot-bsb.md) §12 (pkgimg v3 volume), [RFC-0111](../rfcs/RFC-0111-process-lifecycle-freeze-thaw-kill-the-parent-orders-the-process.md) §6 + [ADR-0084](../adr/0084-one-permission-authority-policyd-holds-every-app-grant-published-by-the-bundle-manager-asked-by-the-launchers.md) (one permission authority)
> **Invariants** — `manifest.nxb` is the signable contract; TOML is input only. Schema fields are append-only. A system-role permission is refused at pack time unless the bundle type holds that role.
> **Read next** — [artifact-kinds.md](artifact-kinds.md), [supply-chain.md](supply-chain.md), [../storage/README.md](../storage/README.md)

## Layout

```text
<bundle>/
├── manifest.nxb           Cap'n Proto BundleManifest (canonical, deterministic, signable)
├── payload.elf            native service/app ELF         — or —
├── payload.nxir           payload_kind = "ui-program": the compiled DSL program
└── meta/
    ├── sbom.json          CycloneDX 1.5 (digest bound in manifest.sbomDigest)
    ├── repro.env.json     repro metadata (digest bound in manifest.reproDigest)
    ├── launch.json        services only: { "stack_pages": N, "features": "…" } — marks a spawnable bundle
    └── app.properties     apps only: the registry sidecar (below)
```

`nxb-pack` names the payload file from the manifest's `payloadKind`. Digest and size fields are
payload-agnostic.

## Sources

| bundle | source | built by |
|---|---|---|
| app | `userspace/apps/<app>/manifest.toml` (next to `ui/`, `assets/`, …) | `nx app compile --app <dir> --out <payload> --meta <props>` + `nxb-pack --toml` |
| service | TOML generated per service in `scripts/build.sh` (`bundle_type = "service"`) | `nxb-pack --toml` + `meta/launch.json` |
| `system` | generated in `scripts/build.sh`; payload is `build.prop` | `nxb-pack --toml` |

```bash
cargo run -p nxb-pack -- --toml manifest.toml <payload> out/<bundle>/   # from TOML
cargo run -p nxb-pack -- <payload.elf> out/<bundle>/                    # default manifest
cargo run -p nxb-pack -- --hello out/<bundle>/                          # fixture bundle
```

Pack steps: TOML → `manifest.nxb` (`schemaVersion = 2`) → write payload → `meta/sbom.json` →
`meta/repro.env.json` → rewrite the manifest with `payloadDigest`/`payloadSize`/`sbomDigest`/
`reproDigest`. A native payload must pass the `exec_image` page-grid check (ADR-0080).

## Manifest fields

`tools/nexus-idl/schemas/manifest.capnp`, `struct BundleManifest`:

| @ | capnp field | TOML key | type / rule |
|---|---|---|---|
| 0 | `schemaVersion` | — | `UInt8`; nxb-pack writes 2 |
| 1 | `name` | `name` | non-empty |
| 2 | `semver` | `version` | SemVer |
| 3 | `abilities` | `abilities` | `List(Text)`, non-empty entries |
| 4 | `capabilities` | `caps` | `List(Text)` permission names |
| 5 | `minSdk` | `min_sdk` | SemVer |
| 6 | `publisher` | `publisher` | 16 bytes (32 hex); default zero placeholder |
| 7 | `signature` | `sig` | 64 bytes (128 hex); default zero placeholder |
| 8 | `payloadDigest` | — | SHA-256 of the payload, written by nxb-pack |
| 9 | `payloadSize` | — | payload bytes |
| 10 | `sbomDigest` | — | SHA-256 of `meta/sbom.json` |
| 11 | `reproDigest` | — | SHA-256 of `meta/repro.env.json` |
| 12 | `bundleType` | `bundle_type` | `BundleType`, default `app` ([artifact-kinds.md](artifact-kinds.md)) |
| 13 | `dependencies` | `dependencies` | `"name"` or `"name@<constraint>"` (default `*`) |
| 14 | `providedServices` | `provides` | `List(Text)` |
| 15 | `resources` | `resources` | `"kind:path"`; kind ∈ icon, cursor, font, wallpaper, sound, data (default data) |
| 16 | `payloadKind` | `payload_kind` | `elf` (default) or `"ui-program"` (`uiProgram`) |
| 17 | `exports` | `exports` | `[{ ability, permission }]`; permission must be `app.<name>.<CAP>` |

TOML keys that are not in `manifest.nxb`: `label`, `icon`, `icon_svg`, `background`,
`restorable`. They reach the device through `meta/app.properties` or build-time icon rendering.

Host parser: `userspace/bundlemgr/src/manifest.rs` — `Error::Decode`, `MissingField`,
`InvalidField { field, reason }`; publisher must be 16 bytes, signature 64, digests empty or 32.

### Pack-time role gate (`tools/nxb-pack/src/main.rs`)

| permission | allowed `bundle_type` |
|---|---|
| `nexus.permission.SESSION` | `greeter` |
| `nexus.permission.LAUNCH`, `ENUMERATE` | `shell` |
| `nexus.permission.SETTINGS`, `SCREENCAP` | `settings` or `shell` |
| `nexus.permission.FILES` | `filemanager` |
| `nexus.permission.IME` | `ime` |
| `nexus.permission.UPDATES` | `settings` |
| `nexus.permission.INPUT_STREAM` | none (declared, not grantable; `test_reject_stream_class_without_a_grant`) |

### `meta/app.properties`

Written by `nx app compile --meta` (`tools/nx/src/commands/app.rs`), parsed by bundlemgrd
(`volume.rs::parse_app_properties`):

```
label=<label or capitalized name>
icon=<icon>
bundle_type=<type>
payload_kind=ui-program
permissions=<caps, comma-joined>
background=<modes, comma-joined>
restorable=<true|false>
```

## Grants: one authority

- bundlemgrd reads every app's `permissions`/`background`/`restorable` from the verified volume
  and publishes them to policyd once per boot. abilitymgr (launch) and execd (SDK routes) ask
  policyd for grants (ADR-0084).
- An unknown permission rejects the whole app at publish (`policyd: app grants reject (app= cap=)`).
- `background = ["audio", …]` (≤ 8 modes of 1..=32 `[a-z0-9-]`, `tools/nxb-pack/src/background.rs`)
  becomes `nexus.permission.BACKGROUND`: a hidden app is not frozen.
- `restorable = true` (`tools/nxb-pack/src/restorable.rs`, RFC-0100 §6) becomes
  `nexus.permission.STATE`: the app's own `/state/app/@self/…` record via `svc.state`.
- Naming `BACKGROUND` or `STATE` in `caps` directly is a pack-time error.
- Exports: a consumer declares `app.<bundle>.<CAP>` in `caps`; the grant resolves only if an
  app exports the ability. abilitymgr's export table (`APP_EXPORTS`, `find_export`) is generated
  at build time from the app manifests (`source/services/abilitymgr/src/caps.rs`), fail-closed.

## The system volume (pkgimg v3)

On the device `pkg:/` is the verified system volume:

1. `scripts/build.sh` emits `build/system-bundles/<name>/` for every service, every
   `payload_kind = "ui-program"` app under `userspace/apps/`, and `system`.
2. `nx image build --system-bundles <dir>` packs them into a pkgimg v3 volume
   (`userspace/storage/src/pkgimg_bundles.rs`): per-file sha256, a bundle table with each
   bundle's data window, window sha256 and launch parameters (`stack_pages` 1..=64,
   `global_pointer`), bound by an NXSV signed against `policies/os-trust.toml`.
3. bundlemgrd verifies the index at boot and each bundle's digest lazily; packagefsd mounts the
   index (`GET_INDEX`) and fetches files on demand (`GET_FILE_VMO`). Init spawns services from
   the volume (ADR-0060).
4. `/packages/<name>@<version>/…` is the versioned view; `pkg:/<name>/…` the active version.

Host-only pkgimg v2 (`userspace/storage/src/pkgimg.rs`, magic `PKGIMGV2`) remains for fixtures
and packagefsd host mode:

```bash
cargo run -p pkgimg-build -- out/packages.pkgimg path/to/<bundle>@<version>.nxb
cargo run -p pkgimg-build --bin pkgimg-verify -- out/packages.pkgimg
```

## Loader handshake

- Services: execd starts a process from a sealed image with `exec_vmo` (ADR-0080).
- Apps (`uiProgram`): execd arms a VMO at bundlemgrd and asks `GET_PAYLOAD`; the program runs in
  the app-host runtime process, which presents through its own windowd surface (ADR-0042).
- `userspace/exec-payloads/` exposes the fixture manifest and payload `nxb-pack --hello` uses.

## Proof

`cargo test -p nxb-pack` (`tools/nxb-pack/tests/payload_kind.rs`,
`tools/nxb-pack/tests/repo_bundles.rs` — the repo app manifests pack), `cargo test -p bundlemgr`, `cargo test -p storage --test pkgimg_v3`.
