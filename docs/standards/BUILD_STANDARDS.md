---
schema: 1
title: "Build standards"
role: reference
subsystem: devx
sources: [justfile, Makefile, scripts/build.sh, config/os-services.txt, config/deny.toml, rust-toolchain.toml]
contracts: [RFC-0009]
verified: 2026-11-02
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Build standards

> **Purpose** — how a crate is gated for host and OS, which crates may never reach the OS graph, and which gates prove a build before a commit.
> **Owns** — the build rules behind `justfile`, `Makefile`, `scripts/build.sh`, `config/os-services.txt`
> **Contracts** — [RFC-0009](../rfcs/RFC-0009-no-std-dependency-hygiene-v1.md) (no_std dependency hygiene)
> **Invariants** — OS services build with `--no-default-features --features os-lite`; `parking_lot`, `parking_lot_core` and `getrandom` never enter the OS graph; one pinned toolchain everywhere.
> **Read next** — [RUST_STANDARDS.md](RUST_STANDARDS.md), [../testing/README.md](../testing/README.md)

## The golden rule

OS services build with `--no-default-features --features os-lite`. Without it, `std`-only
dependencies leak into the bare-metal build and fail with errors like:

```text
error: can't find crate for `std`
  --> parking_lot_core/src/lib.rs
```

## Toolchain

One pinned nightly, `nightly-2025-01-15` (`rust-toolchain.toml`; `toolchain :=` in the
`justfile`, `NIGHTLY ?=` in the `Makefile`), for lint, fmt, host tests, the OS cross-build and CI.
Never a floating `+stable` in a recipe — a split silently diverges (a newer clippy accepts APIs the
pinned build rejects; a newer rustfmt reorders imports). A green `just test-all` predicts a green
`make build`, `just start` and CI because all of them use this toolchain.

## Feature gates

A crate that supports host and OS defines:

```toml
[features]
default = ["std"]   # host builds
std = []            # std-dependent code
os-lite = []        # the no_std path
```

```rust
#![cfg_attr(all(feature = "os-lite", not(feature = "std"), nexus_env = "os"), no_std)]

#[cfg(all(feature = "os-lite", nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
extern crate alloc; // required for Vec, String, BTreeMap in no_std

#[cfg(feature = "std")]
mod host_impl;
#[cfg(feature = "os-lite")]
mod os_impl;
```

- **Features select dependencies; `nexus_env` selects code.** A `std`-only crate is gated behind
  the `std` feature, never behind `#[cfg(nexus_env = "host")]` alone.
- **Propagate features explicitly.** A dependency that has its own `std`/`os-lite` is declared with
  `default-features = false` and enabled per feature:

```toml
[features]
default = ["std"]
std = ["my-dep", "my-dep/default"]
os-lite = ["my-dep", "my-dep/os-lite"]

[dependencies]
my-dep = { path = "../my-dep", optional = true, default-features = false }
```

  Without `default-features = false`, Cargo builds the dependency with its defaults and pulls
  `std`, serde or capnp into the OS build.
- **Guard feature-only modules.** A module that uses capnp sits behind `#[cfg(feature = "idl-capnp")]`.
- **Library-only OS builds.** A crate whose binary is host-only is built for the OS with `--lib`;
  `nexus-init` is a library there and the OS binary is `source/apps/init-lite/`.

## Forbidden crates (OS target)

| crate | why | instead |
|---|---|---|
| `parking_lot`, `parking_lot_core` | need OS threads, pull `std` | `spin`, `source/libs/nexus-sync/` |
| `getrandom` | needs OS entropy | rngd / the hardware RNG, deterministic seeds in tests |
| `std` | bare metal | `core` + `alloc` |

Transitive leaks count: `rand = "0.8"` pulls `getrandom` through its defaults — use
`default-features = false`. Licenses are limited to Apache-2.0, MIT and BSD (`config/deny.toml`).

## Gates

| command | checks |
|---|---|
| `just dep-gate` | forbidden crates in the OS graph of every crate in `config/os-services.txt` (runs `arch-gate` first) |
| `just deny-check` | licenses and advisories (`cargo deny`, `config/deny.toml`) |
| `just diag-os` | every OS service checks for `riscv64imac-unknown-none-elf` with `os-lite` |
| `just diag-host` / `just diag-kernel` | host and kernel builds |
| `just diag` | all three with warnings denied — the hard warning gate |
| `just build-os-workspace` | the real OS cross-build (`scripts/build.sh` under `NEXUS_WARN_GATE=1`) |
| `just deadcode` | dead code against the time-boxed `config/deadcode.allow` (`tools/deadcode-scan.sh`) |
| `just check` | fmt, clippy, deny, arch-check, structure gate, ledger gate and the repo's static gates |

When `dep-gate` fails, find the path:

```bash
cargo tree --target riscv64imac-unknown-none-elf -p dsoftbusd -i parking_lot
```

## House rules

- No `unwrap`/`expect` in daemons; propagate errors with context.
- Userspace crates keep `#![forbid(unsafe_code)]` and pass clippy's denied lints.
- No blanket `#[allow(dead_code)]` or `#[allow(unused)]`: cfg-gate the item, or add a time-boxed
  entry to `config/deadcode.allow`.
- An unused parameter on one cfg path gets a leading underscore, not an allow.
- `NEXUS_ALLOW_WARN=1` is a local escape only.

## Adding an OS service

1. `Cargo.toml` with `default = ["std"]` and `os-lite`; dependencies with `default-features = false`.
2. It builds with `--no-default-features --features os-lite`.
3. Add it to `config/os-services.txt` — the one list `just dep-gate`, `just diag-os` and
   `make dep-gate` read.
4. `just dep-gate` and `just diag` pass; it ships `src/` and `tests/` (`config/service-layout.allow`).
