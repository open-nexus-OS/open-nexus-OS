---
schema: 1
title: "Rust standards"
role: reference
subsystem: devx
sources: [config/rustfmt.toml, rust-toolchain.toml, justfile, tools/arch-check, config/loc-baseline.txt]
contracts: [RFC-0009, ADR-0017, ADR-0051]
verified: 2026-11-02
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Rust standards

> **Purpose** — the rules Rust code in this repo follows: std vs no_std, layering, warnings, unsafe, errors, types, service wire frames.
> **Owns** — no code; enforced by `just check`, `just diag` and review
> **Contracts** — [RFC-0009](../rfcs/RFC-0009-no-std-dependency-hygiene-v1.md) (no_std dependency hygiene), [ADR-0017](../adr/0017-service-architecture.md) (service architecture), [ADR-0051](../adr/0051-declarative-wire-codec-nexus-wire.md) (declared wire frames)
> **Invariants** — zero warnings under host, os and kernel cfgs; no `unwrap`/`expect` on untrusted input; every `unsafe` carries its argument.
> **Read next** — [BUILD_STANDARDS.md](BUILD_STANDARDS.md), [SECURITY_STANDARDS.md](SECURITY_STANDARDS.md), [../architecture/concurrency.md](../architecture/concurrency.md)

## 1. std vs no_std

| code | mode |
|---|---|
| `userspace/` domain crates | `std` on the host for tests; the OS backend under `nexus_env="os"` |
| `source/kernel/`, `source/libs/`, OS builds of services and drivers | `no_std`, `alloc` only when needed |
| `tools/`, `tests/` | `std` |

- OS services build with `--no-default-features --features os-lite`.
- The host/OS split is the cfg `nexus_env = "host" | "os"`. Never invent another cfg.
- Dependency hygiene (forbidden crates, licenses) is RFC-0009, checked by `just dep-gate`.

## 2. Layering

- **Kernel** (`source/kernel/neuron/`): scheduling, memory, IPC and capability mechanics only. No
  policy, crypto, protocol parsing or drivers. Concurrency rules: [concurrency.md](../architecture/concurrency.md).
- **Core libraries** (`source/libs/`): small crates, explicit error types, bounded inputs. Only these
  carry the `nexus-` prefix.
- **Services** (`source/services/<name>d/`): thin adapters — wiring, validation, error
  propagation. Heavy logic moves into a host-tested `userspace/` crate. Every service ships `src/`
  and `tests/` (`config/service-layout.allow`).
- **Userspace crates**: host-first; an unimplemented OS backend returns `Unsupported`
  deterministically and never pretends.
- **Markers**: `*: ready` only after real readiness, `SELFTEST: … ok` only after real behavior.

`#![forbid(unsafe_code)]` in every userspace crate. The layering guard is `tools/arch-check`.

## 3. Toolchain, formatting, lints

- One pinned toolchain (`rust-toolchain.toml`) for lint, fmt, tests, the OS build and CI. Never a
  floating `+stable` in a recipe.
- Format only with the repo config:
  `cargo +nightly-2025-01-15 fmt --all -- --config-path config/rustfmt.toml`.
- `just lint` runs clippy with `-D warnings -D clippy::unwrap_used -D clippy::expect_used`.
- When clippy suggests an API newer than the pinned toolchain, keep the old form with
  `#[allow(unknown_lints, clippy::<lint>)]` (the `unknown_lints` guard keeps the pinned clippy quiet).
- Module size: no `.rs` file over ~600 lines unless grandfathered in `config/loc-baseline.txt`
  (it may shrink, never grow; `just structure-gate`).

## 4. Warnings and dead code

Warnings are a hard gate: `just diag` denies any warning under host, os and kernel cfgs, and the OS
build fails on per-service warnings (`NEXUS_WARN_GATE=1`). A warning usually means dead plumbing, a
changed feature path or an unfinished refactor.

- Fix the warning, or cfg-gate the item: code used only on the OS is
  `#[cfg(nexus_env = "os")]`, so the host build never compiles it.
- Do not add `#[allow(dead_code)]`. Superseded code is deleted, not kept "to document the wire".
- `NEXUS_ALLOW_WARN=1` is a local escape, never a commit state.

## 5. Unsafe

- Allowed where the hardware demands it: MMIO and CSR access, context switch, page tables, trap
  glue. Keep the block small, convert to safe types at once.
- Every `unsafe` block carries a `// SAFETY:` comment naming the invariant.
- No `unsafe impl Send/Sync` without a written argument: what is shared, why it is safe, what
  enforces it. Real example: `KernelHandles` in `source/kernel/neuron/src/core/trap/runtime.rs`
  (every mutable use goes through the BKL guard) with `static_assertions::assert_impl_all!`.

## 6. Errors and panics

- Propagate errors explicitly; `Result<T, E>` inside the kernel and services, not numeric codes.
- The syscall ABI returns `-errno` in `a0`; the conversion is in one place
  (`source/kernel/neuron/src/core/trap/errno.rs`). Map failures keep their identity (ADR-0054).
- Errors that represent a security decision or resource exhaustion are `#[must_use]`.
- No `unwrap`/`expect` on untrusted input. A panic is for a truly unreachable kernel invariant;
  security paths fail closed.

## 7. Types

- **Newtypes** (`#[repr(transparent)]`) for handles and ids that are easy to mix up: `Pid`,
  `AsHandle`, `CapSlot`, `HartId`, `CpuId` (`source/kernel/neuron/src/types.rs`), service ids,
  timestamps. Keep constructors `pub(crate)` with the authority that may mint them.
- **Phantom types** only where a runtime kind must become a compile-time one; prefer newtypes.
- **`!Send`/`!Sync` by assertion** for state that must not cross harts:
  `static_assertions::assert_not_impl_any!(Scheduler: Send, Sync);` — the kernel's `Scheduler`,
  `TaskTable`, `Router`, `CapTable`, `AddressSpaceManager` and `PageTable` all carry it.
- Kernel modules with global state document their ownership model in the module comment.

## 8. Service wire contracts

- Host builds serve Cap'n Proto (`src/std_server.rs`, schemas in `tools/nexus-idl/schemas/`); OS
  builds serve versioned byte frames (`src/os_lite.rs`). samgrd, bundlemgrd, policyd, vfsd,
  packagefsd and execd follow this shape.
- New OS frames are declared in `source/libs/nexus-wire/`, not hand-coded (ADR-0051).
- Every input is bounded before it is parsed or allocated for:

```rust
const MAX_SCOPE_LEN: usize = 64;

fn decode_append(frame: &[u8]) -> Result<AppendRequest, DecodeError> {
    if frame.len() < MIN_FRAME_LEN {
        return Err(DecodeError::TooShort);
    }
    let scope_len = frame[4] as usize;
    if scope_len > MAX_SCOPE_LEN {
        return Err(DecodeError::OversizedScope);
    }
    // ... safe to proceed
}
```

- Identity is the kernel's `sender_service_id`, never a payload string.

## 9. Waits and deadlines

- Every wait ends in a decision — ready or explicitly degraded, never "still waiting". A credit is
  a lease that expires.
- A deadline is evaluated on a path the thing it bounds cannot starve (timer, pacer, supervisor),
  never inside the handler the missing signal suppresses.
- Prefer reactivity (an endpoint, a waitset, a fence) over polling with a timeout.
- Kernel queues are bounded with a deterministic reject on saturation.

Background and observed instances: [concurrency.md](../architecture/concurrency.md#anti-patterns).

## 10. Tests

- Behavior and negative cases on the host first; security changes carry `test_reject_*` tests.
- QEMU runs are bounded and judged by markers (`just test-os <profile>`); see
  [../testing/README.md](../testing/README.md).
