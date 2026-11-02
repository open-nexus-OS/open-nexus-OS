---
schema: 1
title: "Host E2E suites"
role: reference
subsystem: devx
sources: [tests/e2e, tests/e2e_policy, tests/logd_e2e, tests/remote_e2e, tests/vfs_e2e]
contracts: [RFC-0014, RFC-0010, RFC-0011, RFC-0031]
verified: 2026-11-02
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Host E2E suites

> **Purpose** — what the in-process E2E suites under `tests/` cover, how to run them, and which QEMU lane covers the same feature on the OS.
> **Owns** — `tests/e2e/`, `tests/e2e_policy/`, `tests/logd_e2e/`, `tests/remote_e2e/`, `tests/vfs_e2e/`
> **Contracts** — [RFC-0014](../rfcs/RFC-0014-testing-contracts-and-qemu-phases-v1.md) (host-first contract tests), [RFC-0010](../rfcs/RFC-0010-dsoftbus-cross-vm-harness-v1.md) (cross-VM harness), [RFC-0011](../rfcs/RFC-0011-logd-journal-crash-v1.md) (logd journal), [RFC-0031](../rfcs/RFC-0031-crashdumps-v1-minidump-host-symbolize.md) (crashdumps)
> **Invariants** — a suite runs real daemon handlers over loopback IPC, no QEMU and no host network. A host suite never prints OS proof markers.
> **Read next** — [layers.md](layers.md), [network-distributed-debugging.md](network-distributed-debugging.md)

## Run

`just test-e2e` runs all five packages (`nexus-e2e`, `remote_e2e`, `logd-e2e`, `vfs-e2e`,
`e2e_policy`) on the pinned toolchain; CI runs it in the host-tests job. One suite:
`cargo test -p <package>`.

## Suites

| Package (dir) | Daemons wired | Covers |
|---|---|---|
| `nexus-e2e` (`tests/e2e/`) | `samgrd`, `bundlemgrd`, `keystored` | register/resolve, unknown-name reject, bundle install/query/payload, invalid signature reject, signature enforced through keystored |
| `e2e_policy` (`tests/e2e_policy/`) | `policyd`, `bundlemgrd`, `samgrd` | installs an allowed and a denied manifest and checks allow/deny plus the missing capabilities |
| `logd-e2e` (`tests/logd_e2e/`) | `logd` | APPEND → QUERY → STATS, drop-oldest on overflow, `since_nsec` pagination, crash-report events, concurrent writers |
| `vfs-e2e` (`tests/vfs_e2e/`) | `packagefsd`, `vfsd`, `bundlemgrd` | publish a bundle and read it through `vfsd`, readdir, `test_reject_readdir_error_codes` |
| `remote_e2e` (`tests/remote_e2e/`) | two nodes, each with `samgrd`, `bundlemgrd` and DSoftBus | discovery announce, Noise-authenticated session, remote resolve/install, remote packagefs stat/open/read/close, negative statuses, path and read-bound rejects |

`remote_e2e` runs both nodes over `FakeNet`, the `userspace/nexus-net` sockets facade; identity
keys come from `userspace/identity`. Large payloads (bundle bytes) are staged in the bundle
manager's artifact store first, standing in for the VMO hand-off on the OS. The daemon's own seams
are covered in `source/services/dsoftbusd/tests/` (`p0_unit.rs`, `session_steps.rs`,
`reject_transport_validation.rs`, remote packagefs/statefs protocol and reject suites).

Policy inputs live under `policies/` (`base.toml`, `nexus.policy.toml`); `recipes/policy/` holds
only a migration note and is never an authority.

## Coverage across layers

| Feature | Host unit | Host E2E | QEMU 1-VM | QEMU 2-VM |
|---|---|---|---|---|
| Service registry + bundles (samgrd, bundlemgrd) | yes | `nexus-e2e` | every lane | `os2vm` remote proxy |
| Policy allow/deny (policyd) | yes | `e2e_policy` | `SELFTEST: policy allow ok` / `deny ok` | — |
| VFS (packagefsd, vfsd) | yes | `vfs-e2e` | `SELFTEST: vfs stat ok`, `read ok`, `ebadf ok` | remote packagefs |
| logd journal + crash reports | yes | `logd-e2e` | `SELFTEST: log query ok`, `SELFTEST: crash report ok` | — |
| Crashdump artifact (execd → statefs) | `crash`, `execd`, `statefsd` | — | `SELFTEST: minidump ok` + reject markers | — |
| DSoftBus discovery + Noise session | `userspace/dsoftbus/tests/` | `remote_e2e` | `just ci-os-quic` | `just ci-os-os2vm` |
| Remote proxy / remote packagefs | `dsoftbusd` tests | `remote_e2e` | — | `just ci-os-os2vm` |
| Real network (virtio-net, smoltcp) | `nexus-net-os` | — | `just ci-os-dhcp` | `just ci-os-os2vm` |

## When to use which

- Host E2E: service integration logic, protocol and state; fast and deterministic, runs in CI.
- QEMU 1-VM: the boot chain, virtio devices, the kernel IPC path, the marker contract.
- QEMU 2-VM: cross-VM networking and distributed sessions only; opt-in, not part of `test-all`.
