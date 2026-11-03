---
schema: 1
title: "Test layers"
role: reference
subsystem: devx
sources: [userspace, source/services, source/kernel/neuron/src/lib.rs, tests, scripts/qemu-test.sh, tools/os2vm.sh]
contracts: [RFC-0014, ADR-0025]
verified: 2026-11-03
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Test layers

> **Purpose** — what each test layer proves, how it is run, and one example of each.
> **Owns** — the layer split; per-task requirement matrices live in the task ledgers (`./target/debug/nx ledger show TASK-NNNN`).
> **Contracts** — [RFC-0014](../rfcs/RFC-0014-testing-contracts-and-qemu-phases-v1.md) (host-first contract tests + phased QEMU), [ADR-0025](../adr/0025-qemu-smoke-proof-gating.md) (opt-in network proofs)
> **Invariants** — logic is proven on the host; QEMU proves only what needs the booted system. Daemons stay thin; logic lives in libraries.
> **Read next** — [e2e.md](e2e.md), [os-markers.md](os-markers.md), [SECURITY_STANDARDS.md](../standards/SECURITY_STANDARDS.md)

## Layers at a glance

| Layer | Proves | Does not prove | Run |
|---|---|---|---|
| Userspace library | logic, encodings, bounds, reject classes | IPC, kernel, timing | `cargo test -p <crate>` |
| Service host test | the daemon's request handling over an in-process transport | the OS transport, init wiring | `cargo test -p <svc>` |
| Kernel host test | pure kernel tables | trap, scheduler, MMU paths | `just test-kernel` |
| Kernel target gate | types, exhaustiveness, zero warnings on riscv | behavior | `just diag-kernel`, `just lint-kernel` |
| Host E2E | several real daemons together over loopback | the OS transport | `just test-e2e` |
| Host goldens | deterministic pixels/layout of UI and DSL apps | the GPU path | `cargo test -p <suite>` |
| Static chain contract | the marker chain a service contract emits | the real boot | `cargo test -p nx` |
| QEMU lane | the booted system, end to end | real hardware timing | `just test-os <profile>` |
| QEMU 2-VM | discovery, sessions and remote proxy across two VMs | single-VM paths | `just ci-os-os2vm` |
| Board | the real SoC and peripherals | — | `just board-test <profile>` |

## Userspace libraries (`userspace/`)

- `#![forbid(unsafe_code)]`; built for the host by default (`.cargo/config.toml` sets
  `nexus_env="host"`). `#[cfg(nexus_env = "host")]` selects in-memory backends,
  `#[cfg(nexus_env = "os")]` the syscall backends.
- Tools: unit tests, `proptest`, golden vectors, Miri (`just miri-strict` with isolation,
  `just miri-fs` with `-Zmiri-disable-isolation`; keep property tests small under `#[cfg(miri)]`).
- Security paths carry `test_reject_*` tests ([SECURITY_STANDARDS.md](../standards/SECURITY_STANDARDS.md)).
- Example: `userspace/dsoftbus/tests/mux_contract_rejects_and_bounds.rs` — the mux reject
  taxonomy and backpressure bounds, `cargo test -p dsoftbus --test mux_contract_rejects_and_bounds`.

## Services (`source/services/<name>d/`)

- A daemon translates IPC frames into library calls; no `unwrap`/`expect` on input.
- Host tests drive the request handler in-process; new services ship `src/` + `tests/`
  (`just structure-gate`).
- Example: `source/services/policyd/tests/lite_protocol_inprocess.rs` — policyd's frame protocol
  over an in-process transport, `cargo test -p policyd`.
- OS cross-build check for one service:
  `cargo +nightly-2025-01-15 check -p <svc> --target riscv64imac-unknown-none-elf --no-default-features --features os-lite`.
  The OS target's flags come from `.cargo/config.toml`; never override `RUSTFLAGS` for it.

## Kernel (`source/kernel/neuron/`)

- Host: most modules are `#[cfg(target_os = "none")]`; the un-gated pure modules in
  `source/kernel/neuron/src/lib.rs` (e.g. `waitset`, `fence`, `frames`) run under `just test-kernel`.
- Target: `just build-kernel`, `just lint-kernel` (`-D warnings`), `just diag-kernel`.
- Behavior: in-kernel selftests print `KSELFTEST:` markers during boot; they are declared in the
  proof manifest like every other marker. Example: `KSELFTEST: ipc call budget ok` asserts the
  `ipc_call` round trip against its budget on every boot (RFC-0096).
- Features: default `boot_banner`, `selftest_priv_stack`, `selftest_time`, `timer_irq`,
  `failpoints`; opt-in `trap_symbols` (name+offset for `sepc`), `trap_ring` (last trap frames),
  `selftest_ipc`/`_caps`/`_sched`/`_map`. Set per run with `NEURON_BOOT_FEATURES=…` (`scripts/build.sh`).

## Host E2E (`tests/`)

In-process integration of real daemons: `nexus-e2e`, `remote_e2e`, `logd-e2e`, `vfs-e2e`,
`e2e_policy` — all under `just test-e2e`. Example: `tests/vfs_e2e/tests/vfs_roundtrip.rs` publishes
a bundle through `packagefsd` and reads it back through `vfsd`. Details: [e2e.md](e2e.md).
`just test-init` (`scripts/host-init-test.sh`) runs `nexus-init` on the host and checks readiness.

## Host goldens and conformance (`tests/`)

Deterministic renderer and app suites with checked-in goldens: `tests/ui_host_snap`,
`tests/ui_v10_goldens`, `tests/dsl_goldens`, `tests/dsl_apps_conformance`, … Example:
`tests/dsl_apps_conformance/tests/calculator_pixels.rs`. The shell conformance tests read the same
`scenarios/targets.toml` points the QEMU injector clicks, so a moved widget fails here first
([proof-manifest.md](proof-manifest.md)).

## Static chain contract (`tools/nx/`)

`tools/nx/chains/markers.txt` lists the cross-service marker chains (input, GPU, display, os2vm, …).
`cargo test -p nx` checks that the simulated service contracts under `tools/nx/src/chain/contract/`
emit every one; `scripts/check-chain-markers.sh` greps the same list in a real `uart.log`.
Example: group `input-route` — `inputd: chain I5 delivered to windowd` →
`windowd: chain I6 input recv (state applied)`.

## QEMU lanes (`scripts/qemu-test.sh`)

- Boots nxboot → kernel → init → services → `selftest-client`, records the UART, and judges it:
  the profile's marker ladder (from the proof manifest), lane-specific ladders in the script,
  `verify-uart` (unexpected/forbidden lines), the FAIL gate and the chain-marker contract
  ([os-markers.md](os-markers.md)).
- `selftest-client` is an observer: it probes services and prints what it saw; it never
  synthesizes a success.
- Example: `just ci-os-smp1` — one hart with icount; with `RUN_UNTIL_MARKER=1` QEMU stops shortly after the ladder's end marker.

## QEMU 2-VM (`tools/os2vm.sh`)

Two QEMU nodes on one virtual network: discovery, Noise session, mux, remote proxy, perf budgets
and soak, with typed failure codes. Example: `just ci-os-os2vm` (runs `tools/os2vm.sh`, then the
`os2vm` chain group on node A's log). Operations: [network-distributed-debugging.md](network-distributed-debugging.md).

## Board (`scripts/board-test.sh`)

The same marker vocabulary on the reference board: the image is flashed, the boot trace pulled
from eMMC (`just board-log`) and judged by `verify-uart`, the FAIL gate
(`config/fail-marker-allow.txt` + `config/fail-marker-allow-board.txt`) and the runner's required
ladder. `board-visible` adds operator-acknowledged `board-visual:` rungs (`just board-ack`).
Without a board, an archived capture is judged with
`scripts/board-test.sh --profile=board-headless --log=<uart.log>`. See `docs/board/bpi-f3.md`.
