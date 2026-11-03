---
schema: 1
title: "Sandboxing v1"
role: reference
subsystem: security
sources: [source/services/vfsd/src/sandbox.rs, source/services/vfsd/src/namespace.rs, userspace/nexus-vfs, source/services/execd/src/lib.rs]
contracts: [RFC-0042]
verified: 2026-11-03
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Sandboxing v1

> **Purpose** — what confines an app subject today, which rejects prove it, and where the boundary stops.
> **Owns** — `source/services/vfsd/src/sandbox.rs`, `source/services/vfsd/src/namespace.rs`, `userspace/nexus-vfs/`
> **Contracts** — [RFC-0042](../rfcs/RFC-0042-sandboxing-v1-vfs-namespaces-capfd-manifest-permissions-host-first-os-gated.md) (VFS namespaces, CapFd, manifest permissions)
> **Invariants** — confinement comes from which capabilities a subject receives at spawn. App subjects never get direct `packagefsd` / `statefsd` capabilities, or `device.mmio.net`.
> **Read next** — [capabilities.md](capabilities.md), [network-policy.md](network-policy.md), [abi-filters.md](abi-filters.md)

## Scope

Sandboxing v1 is a userspace confinement floor. It does not claim kernel-enforced namespaces or syscall containment.

- The kernel is not involved.
- Confinement depends on spawn-time capability distribution: init and execd decide what a child holds.
- App subjects reach files only through vfsd's namespace view, never through a direct `packagefsd` or `statefsd` capability.

## Network boundary

netstackd's facade enforces outbound and inbound policy ([network-policy.md](network-policy.md)). A subject holding `device.mmio.net` drives the NIC itself and bypasses both. The grant is the boundary, so never give it to a sandboxed app subject.

## Enforcement floor

- Namespace paths are canonicalized; `..` traversal is rejected deterministically.
- CapFd tokens fail closed on integrity, replay, subject binding, expiry and rights subset.
- Unauthorized namespace paths and rights mismatches are rejected the same way every time.

## Required reject proofs

```bash
cargo test -p vfsd -- --nocapture
cargo test -p nexus-vfs -- --nocapture
cargo test -p execd --lib test_reject_direct_fs_cap_bypass_at_spawn_boundary -- --nocapture
```

| Test | Where |
|---|---|
| `test_reject_path_traversal` | `source/services/vfsd/src/sandbox.rs`, `userspace/nexus-vfs/src/lib.rs` |
| `test_reject_unauthorized_namespace_path` | same |
| `test_reject_forged_capfd` | `source/services/vfsd/src/sandbox.rs` |
| `test_reject_replayed_capfd` | `source/services/vfsd/src/sandbox.rs` |
| `test_reject_capfd_rights_mismatch` | `source/services/vfsd/src/sandbox.rs` |
| `test_reject_direct_fs_cap_bypass_at_spawn_boundary` | `source/services/execd/src/lib.rs` |

## QEMU markers

- `vfsd: namespace ready`
- `vfsd: capfd grant ok`
- `vfsd: access denied`
- `SELFTEST: sandbox deny ok`
- `SELFTEST: capfd read ok`
