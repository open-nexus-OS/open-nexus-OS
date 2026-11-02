---
schema: 1
title: "Capability names"
role: reference
subsystem: security
sources: [policies/base.toml, source/services/policyd/src/app_grants.rs, source/services/policyd/build.rs, userspace/policy/src/lib.rs, source/libs/nexus-sdk-routes]
contracts: [ADR-0014, RFC-0005, ADR-0084, RFC-0111]
verified: 2026-11-02
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Capability names

> **Purpose** — what a capability string means, who checks it, and where a grant is written down; the table of current holders is generated from the policy.
> **Owns** — `policies/base.toml` (`[allow]`), `source/services/policyd/src/app_grants.rs` (app permissions)
> **Contracts** — [ADR-0014](../adr/0014-policy-architecture.md) (policyd decides), [RFC-0005](../rfcs/RFC-0005-kernel-ipc-capability-model.md) (kernel capabilities), [ADR-0084](../adr/0084-one-permission-authority-policyd-holds-every-app-grant-published-by-the-bundle-manager-asked-by-the-launchers.md) (app grants)
> **Invariants** — a capability string is authority only when policyd answers for it. No grant ⇒ deny. Never hand-list holders in a doc; the table below is generated.
> **Read next** — [policyd.md](policyd.md), [abi-filters.md](abi-filters.md), [sandboxing.md](sandboxing.md)

## Two kinds of capability

| | Kernel capability | Policy capability (this page) |
|---|---|---|
| What | a handle in a task's cap slot (endpoint, VMO, IRQ, MMIO window) with rights | a string such as `crypto.sign` that names an authority |
| Who enforces | the kernel, on every syscall that uses the slot | the service that owns the operation, by asking policyd |
| How it is held | init pins it into a declared slot (`nexus-service-topology`), or it is moved over IPC (`CAP_MOVE`) | an entry in `policies/base.toml [allow]`, or a published app grant |
| Contract | [RFC-0005](../rfcs/RFC-0005-kernel-ipc-capability-model.md) | [ADR-0014](../adr/0014-policy-architecture.md) |

A policy capability without the matching kernel handle grants nothing, and the reverse also holds: `device.mmio.net` is the policy name, the MMIO window is the kernel handle init gives to its one holder.

## Who checks

The service that performs the operation is the enforcement point. It asks policyd with the subject's kernel sender id:

- about itself — `OP_CHECK_CAP`;
- about the caller it serves — `OP_CHECK_CAP_DELEGATED`, which requires the asking service to hold `policy.delegate`.

Examples in the code: keystored checks `crypto.sign`, `device.keygen`, `device.pubkey.read`, `device.key.reload`; rngd checks `rng.entropy`; init asks `ipc.core` (route) and `proc.spawn` (exec) per request; updated checks `updates.manage` for the sender. The wire is in [policyd.md](policyd.md).

## Where grants live

| Subject | Grant source | Name shape |
|---|---|---|
| Services, drivers, init, selftest | `policies/base.toml` → `[allow] "<service>" = ["cap", …]`, compiled into policyd at build time | lowercase `<domain>.<verb>` or `<domain>.<resource>.<verb>`; trimmed and lowercased before evaluation |
| Apps | the app manifest's `caps = [...]` (e.g. `userspace/apps/calculator/manifest.toml`) → verified volume sidecar → bundlemgrd publishes → policyd's app-grant table ([ADR-0084](../adr/0084-one-permission-authority-policyd-holds-every-app-grant-published-by-the-bundle-manager-asked-by-the-launchers.md)) | `nexus.permission.<NAME>` from `KNOWN_PERMISSIONS`, or app-owned `app.<bundle>.<CAP>`; case is kept |

Derived app grants never appear in `caps`: `background = [...]` becomes `nexus.permission.BACKGROUND`, `restorable = true` becomes `nexus.permission.STATE`. An unknown name rejects the whole app at publish. execd turns an app's routable grants into SDK service routes (`source/libs/nexus-sdk-routes/`).

Argument-level limits on a capability (which statefs prefix, which CIDR) are RFC-0091 profiles, not capability names — see [abi-filters.md](abi-filters.md).

## Naming rules

- Name an authority, not a screen or a product: `network.http.request`, not `browser`.
- One power per name; never bundle unrelated powers.
- No two names that differ only by case or punctuation.
- A narrower prefix capability beats a broad one: `statefs.crash` for `/state/crash/`, not `statefs.write`.
- A name that is withheld on purpose is documented next to the grant in `policies/base.toml` (e.g. `crash.attach.full`).

## Adding a capability

1. Add the check at the enforcement point (`OP_CHECK_CAP` / `OP_CHECK_CAP_DELEGATED`) with a `test_reject_*` test for the denied case.
2. Grant it in `policies/base.toml` with a one-line comment on why this subject needs it.
3. Run `nx policy validate --write-manifest` and commit `policies/manifest.json`.
4. For an app permission: add it to `KNOWN_PERMISSIONS` in `source/services/policyd/src/app_grants.rs`.

## Current holders

Generated from `policies/base.toml [allow]`.

<!-- gen:capabilities -->
| capability | held by |
|---|---|
| `app.grants.publish` | bundlemgrd |
| `app.grants.read` | abilitymgr, execd, selftest-client |
| `boot.reset` | init-lite, nexus-init, selftest-client |
| `boot.target` | init-lite, nexus-init, selftest-client |
| `bundle.query` | abilitymgr, windowd |
| `crash.attach.stack` | execd |
| `crypto.derive.statefs` | selftest-client |
| `crypto.sign` | keystored |
| `crypto.verify` | keystored, updated |
| `device.key.reload` | selftest-client |
| `device.keygen` | keystored, selftest-client |
| `device.mmio.blk` | blkd |
| `device.mmio.display` | gpud |
| `device.mmio.gpu` | gpud |
| `device.mmio.input` | hidrawd, selftest-client |
| `device.mmio.mmc` | blkd |
| `device.mmio.net` | netstackd, selftest-client |
| `device.mmio.rng` | rngd |
| `device.mmio.rtc` | timed |
| `device.mmio.syscon` | socd |
| `device.mmio.usb` | xhcid |
| `device.pubkey.read` | selftest-client |
| `fs.read` | bundlemgrd, execd, init-lite, nexus-init, packagefsd |
| `fs.verify` | bundlemgrd |
| `ipc.core` | abilitymgr, blkd, bootctld, bundlemgrd, clipboardd, dsoftbusd, execd, gpud, hidrawd, imed, ingressd, init-lite, inputd, keystored, logd, memd, metricsd, netstackd, nexus-init, packagefsd, pinched, rngd, samgrd, screencapd, selftest-client, sessiond, settingsd, socd, statefsd, timed, updated, vfsd, windowd, xhcid |
| `net.expose` | selftest-client |
| `policy.abi_mode` | selftest-client |
| `policy.delegate` | bootctld, ingressd, init-lite, keystored, netstackd, nexus-init, rngd, socd, statefsd, updated, xhcid |
| `proc.spawn` | abilitymgr, execd, init-lite, nexus-init, selftest-client |
| `rng.entropy` | keystored, selftest-client |
| `session.query` | abilitymgr, windowd |
| `settings.query` | windowd |
| `soc.glue.display` | gpud, selftest-client |
| `soc.glue.gpu` | gpud |
| `soc.glue.storage` | blkd |
| `soc.glue.usb` | xhcid |
| `statefs.admin` | selftest-client |
| `statefs.boot` | bootctld, selftest-client, updated |
| `statefs.crash` | execd, selftest-client |
| `statefs.keystore` | keystored |
| `statefs.read` | dsoftbusd, imed, init-lite, logd, memd, nexus-init, selftest-client, settingsd |
| `statefs.write` | dsoftbusd, imed, init-lite, logd, memd, metricsd, nexus-init, selftest-client, settingsd |
| `time.read` | samgrd |
| `updates.manage` | selftest-client |
| `usb.hid` | hidrawd |
<!-- /gen:capabilities -->
