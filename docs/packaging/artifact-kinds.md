---
schema: 1
title: "Bundle types"
role: reference
subsystem: app-platform
sources: [tools/nexus-idl/schemas/manifest.capnp, tools/nxb-pack/src/main.rs, source/services/bundlemgrd/src/os_lite.rs, source/services/abilitymgr/build.rs]
contracts: [ADR-0020, RFC-0073, RFC-0075]
verified: 2026-11-02
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Bundle types

> **Purpose** — what each `bundle_type` value means: which permissions it may carry, whether the user can launch it, and how the launcher treats it.
> **Owns** — the `BundleType` enum in `tools/nexus-idl/schemas/manifest.capnp`
> **Contracts** — [ADR-0020](../adr/0020-manifest-format-capnproto.md) (manifest format), [RFC-0073](../rfcs/RFC-0073-app-files-surface-svc-files-permission-filemanager-role.md) (filemanager role), [RFC-0075](../rfcs/RFC-0075-ime-v2-text-focus-composition-delivery.md) (IME role)
> **Invariants** — The type is a privilege ceiling: what a bundle MAY request, enforced fail-closed by `nxb-pack` at pack time. The product assigns which bundle plays a role; the type only permits it. New values are appended, never renumbered.
> **Read next** — [nxb.md](nxb.md)

## Values

`bundle_type` in `manifest.toml` (default `app`) → `bundleType` in `manifest.nxb`:

| @ | type | user-launchable | may hold (system-role permissions) | in the repo |
|---|---|---|---|---|
| 0 | `app` | yes | none | `calculator`, `chat` |
| 1 | `service` | no | none | every service bundle `scripts/build.sh` generates (with `meta/launch.json`) |
| 2 | `library` | no | none | `window-kit` (DSL components compiled into the apps that list it in `dependencies`); `system` (`build.prop`) |
| 3 | `driver` | no | none | — |
| 4 | `framework` | no | none | — |
| 5 | `shell` | no | `LAUNCH`, `ENUMERATE`, `SETTINGS`, `SCREENCAP` | `desktop-shell` |
| 6 | `greeter` | no | `SESSION` | `greeter` |
| 7 | `settings` | yes | `SETTINGS`, `SCREENCAP`, `UPDATES` | `settings` |
| 8 | `filemanager` | yes | `FILES` | `stash` |
| 9 | `ime` | no | `IME` | `ime-ui` |

Permission names are `nexus.permission.<NAME>`. Any type may carry ordinary permissions
(`WINDOW`, `NOTIFY`, …); the table lists only the gated ones. `INPUT_STREAM` is refused for every
type.

## What reads the type

| reader | rule | where |
|---|---|---|
| `nxb-pack` | the privilege ceiling above; unknown type is a pack error | `tools/nxb-pack/src/main.rs` |
| bundlemgrd `OP_LIST_APPS` | lists only `app`, `settings`, `filemanager` (the launcher grid) | `source/services/bundlemgrd/src/os_lite.rs` (`is_launchable`) |
| abilitymgr | `greeter` and `ime` may launch before a session; `shell`, `greeter`, `ime` are single-instance (a second launch answers the running one) | `source/services/abilitymgr/build.rs` (`APP_PRE_SESSION`, `APP_SINGLETON`) |
| `nx app compile` | copies the type into `meta/app.properties` | `tools/nx/src/commands/app.rs` |

## Adding a type

Append to `enum BundleType` in the schema, add the TOML mapping and its ceiling in
`tools/nxb-pack/src/main.rs`, decide launchability in `is_launchable`, and add a
`test_reject_*` for a bundle that requests the new role's permission under a different type.
