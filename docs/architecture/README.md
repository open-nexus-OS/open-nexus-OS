---
schema: 1
title: Architecture
role: index
subsystem: devx
verified: 2026-11-02
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Architecture

> **Purpose** — the cross-cutting pages (vision, system map, concurrency) and a map from each subsystem to the folder that explains it.
> **Owns** — `docs/architecture/`
> **Invariants** — a page here spans subsystems; a page about one subsystem lives in that subsystem's folder.
> **Read next** — [system-map.md](system-map.md), [../kernel/README.md](../kernel/README.md)

## What this folder holds

| page | answers |
|---|---|
| [vision.md](vision.md) | the product and architecture direction every change is judged against |
| [system-map.md](system-map.md) | what lives where in the tree, who holds which role, the boundaries |
| [concurrency.md](concurrency.md) | the kernel's lock model, its rules and the anti-patterns that cost real boots |

## Subsystem → folder

| subsystem | folder |
|---|---|
| kernel: syscalls, scheduling, memory, capabilities, SMP | `docs/kernel/` |
| boot chain, nexus-init, execd, service lifecycle | `docs/boot-init/` |
| how a service is built, samgr, bundlemgrd | `docs/services/` |
| policyd, capabilities, identity, signing, sandboxing | `docs/security/` |
| packagefs, vfs, statefs, nxfs, VMOs, quotas | `docs/storage/` |
| `.nxb` bundles, artifact kinds, supply chain | `docs/packaging/` |
| OTA, delta updates, system sets | `docs/updates/` |
| DSoftBus, network authority, addresses | `docs/network/` |
| logging, metrics, tracing, crash dumps, recovery | `docs/reliability/` |
| windowd, gpud, display chain, app surfaces, nexus-gfx | `docs/graphics-ui/` |
| test layers, QEMU profiles, markers, run logs | `docs/testing/` |
| Rust, build, security and documentation standards | `docs/standards/` |
| contracts and decisions | `docs/rfcs/`, `docs/adr/` |
| developer guides (DSL, UI, SDK, `nx`) · board measurements | `docs/dev/`, `docs/board/` |

Execution truth (what is planned, in progress, done) is the ledger under `tasks/`, read with
`nx ledger show <ID>` and `just brief`.

## Layering quick reference

- **Domain logic** lives in `userspace/<crate>` libraries, built under exactly one of
  `nexus_env="host"` / `nexus_env="os"`, with `#![forbid(unsafe_code)]`. Host tests prove it.
- **Services** under `source/services/<name>d/` are thin adapters: they register with samgr, decode
  IPC, validate, and forward into the userspace crate built for the OS.
- **The guard**: `tools/arch-check` (`just arch-check`) fails when a `userspace/` crate depends on a
  service crate, on `samgrd` or on the kernel; `nexus-abi` is the shared ABI and allowed.
- **Control plane vs data plane**: Cap'n Proto in userspace only (`tools/nexus-idl`,
  `userspace/nexus-idl-runtime/`); the kernel moves bytes, handles and VMOs and parses no IDL. Bulk
  data travels in VMOs. The windowd↔gpud display wire is its own codec
  (`source/libs/nexus-display-proto/`, ADR-0038).
- **Kernel ABI**: syscall numbers, the trap prologue and the kernel memory map change only with an
  RFC or ADR — see [../kernel/README.md](../kernel/README.md).
