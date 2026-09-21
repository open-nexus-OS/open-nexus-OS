---
title: TASK-0332 A Rust `std` target for the OS (`riscv64gc-unknown-nexus`) + C library and C++ runtime port, for isolated components such as the web engine
status: Draft (seeded 2026-09-21 by the hardware fast track — `tasks/IMPLEMENTATION-ORDER.md`; rewritten to end state at its block's P0)
owner: @runtime @devx
created: 2026-09-21
depends-on: []
follow-up-tasks: []
links:
  - Execution order (the lane this belongs to): tasks/IMPLEMENTATION-ORDER.md
  - Playbook: CLAUDE.md
  - Toolchain: rust-toolchain.toml (pinned nightly), config/rustfmt.toml
  - Identity + spawn: source/services/execd (exec_v2, kernel-attributed service names)
  - Consumers: tasks/TASK-0111-ui-v19a-webviewd-sandbox-offscreen.md (recut to the engine component), tasks/TRACK-CONSOLE-AND-TOOLCHAINS.md (managed runtimes)
  - RFC seed (at P0): RFC-0103 std target + component runtime
---

## Origin

No ledger existed: the whole userspace is `no_std` os-lite on `riscv64imac-unknown-none-elf`. Minted for target picture W (the web engine is Rust + C/C++ and needs `std`); `TRACK-CONSOLE-AND-TOOLCHAINS`'s managed runtimes ride on the same target. The component runtime for std binaries under execd is `TASK-0332B` (seeded at P0).

## Context

The web engine (Rust with C/C++ parts) cannot be built for a `no_std` target; the well-known capability OS solved this with a full libc + a POSIX-lite layer and runs the engine as an isolated component behind a Context/Frame/Navigation protocol. Prerequisites: shared read-only code (M3 — a `std` process without it doubles RSS) and demand paging (M2).

## Goal

A tier-3 target `riscv64gc-unknown-nexus`: `std::sys` over our syscalls/IPC (threads, heap, fs via vfsd, sockets via the netstackd facade, time via timed), a C library + C++ runtime port with a clang cross target, `just test-std`; gate: a cross-built `hello` runs on QEMU, then threads/sockets/fs gates and a C++ test binary. R12 first: the pinned nightly builds `std` for a custom target with `-Zbuild-std`; the engine's C++ dependencies' OS requirements listed.

## Non-Goals

Making services `std` (os-lite stays), a POSIX process model in the kernel (components keep kernel-attributed identity and capability tables), the engine itself (`TASK-0111` recut).

## Packages (from the order file; the P0 rewrite fixes them)

- **W1** (0332) — target spec + `std::sys` backend + libc/C++ runtime port + `just test-std`. Gate: `hello`, threads, sockets, fs on QEMU; C++ test binary.
- **W2a** (0332B, at P0) — component runtime for std binaries under execd (identity, caps, `nexus-surface` crate absorbing app-host's client sequence).

## Constraints / invariants (hard requirements)

- **No fake success**: no `*: ready` / `SELFTEST: * ok` markers unless the real behavior happened; a
  human-visible board check is an operator-acked `board-visual:` marker, never prose.
- **The FDT is the one hardware truth**: no address, IRQ, frequency or hart count outside the parser.
- **Firmware blobs** only under `resources/firmware/<device>/` with provenance + license and a gate.
- **Vendor kernel code is reference only**; openly licensed userspace driver code may be ported.
- **Rust hygiene**: no `unwrap`/`expect` on untrusted input; `forbid(unsafe_code)` in userspace crates
  except the one documented MMIO/DMA seam per driver.

## Red flags / decision points

- **RED**: the measurements named in the order file (R-items) are done BEFORE the end-state rewrite.
- **YELLOW**: —
- **GREEN**: —

## Definition of Done

Filled at P0 from the order file's gates: host tests → QEMU profile → board lane marker(s), old
mechanism deleted with a gate against its return, docs sweep (CHANGELOG, board, RFC/ADR status).
