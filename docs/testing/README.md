---
schema: 1
title: "Testing guide"
role: guide
subsystem: devx
sources: [scripts/qemu-test.sh, source/apps/selftest-client/proof-manifest, tests, ".github/workflows/ci.yml"]
contracts: [RFC-0013, RFC-0014, RFC-0038, ADR-0025, ADR-0063]
verified: 2026-11-12
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Testing guide

> **Purpose** — which test layer proves what, which command to run for a change, and where the result lands.
> **Owns** — `scripts/qemu-test.sh`, `source/apps/selftest-client/proof-manifest/`, `tests/`
> **Contracts** — [RFC-0013](../rfcs/RFC-0013-boot-gates-readiness-spawn-resource-v1.md) (readiness), [RFC-0014](../rfcs/RFC-0014-testing-contracts-and-qemu-phases-v1.md) (phases), [RFC-0038](../rfcs/RFC-0038-selftest-client-production-grade-deterministic-test-architecture-refactor-v1.md) (manifest, evidence, replay), [ADR-0025](../adr/0025-qemu-smoke-proof-gating.md), [ADR-0063](../adr/0063-proof-lane-resource-envelope-and-authority.md)
> **Invariants** — host first, QEMU last. A UART marker is printed only after the real behavior (no fake green). A lane is selected by its profile, never by hand-set `SMP=`/`REQUIRE_*` env.
> **Read next** — [layers.md](layers.md), [os-markers.md](os-markers.md), [run-logs.md](run-logs.md), `.claude/skills/verify/SKILL.md`, `.claude/skills/boot-proof/SKILL.md`

## Test layers

| Layer | Proves | Command | Page |
|---|---|---|---|
| Userspace libraries (`userspace/`) | logic, wire formats, reject paths (`test_reject_*`) | `just test-host` / `cargo test -p <crate>` | [layers.md](layers.md) |
| Services (`source/services/`) | IPC adapter contracts on the host | `cargo test -p <svc>` | [layers.md](layers.md) |
| Kernel host tests | pure kernel tables (waitset, fence, frames, …) | `just test-kernel` | [layers.md](layers.md) |
| Host E2E (`tests/*e2e*`) | several real daemons over loopback IPC | `just test-e2e` | [e2e.md](e2e.md) |
| Miri | UB in host crates | `just miri-strict`, `just miri-fs` | [layers.md](layers.md) |
| QEMU lane | the booted system: UART marker ladder per profile | `just test-os <profile>` / `just ci-os-*` | [os-markers.md](os-markers.md) |
| QEMU 2-VM | cross-VM DSoftBus | `just ci-os-os2vm` | [network-distributed-debugging.md](network-distributed-debugging.md) |
| Board | the desk board, judged like a lane | `just board-test board-headless` | `docs/board/bpi-f3.md` |

## Which command for which change

The minimum gate per kind of change is the ladder in `.claude/skills/verify/SKILL.md`; running and
decoding a QEMU lane is `.claude/skills/boot-proof/SKILL.md`. In short:

- Fast pre-commit: `just check`; then `just test-host` (+ `just test-e2e` for cross-service changes).
- OS services: add `just dep-gate` and `just diag-os` (warnings are failures).
- Kernel: `just build-kernel`, `just lint-kernel`, `just test-kernel`, then a QEMU lane.
- Boot or marker behavior: the QEMU lane that carries the marker.
- End of a work package: `just test-all` (check, diag, host + e2e tests, Miri, kernel, then every
  `ci-os-*` lane listed in the `test-all` recipe in `justfile`).

## QEMU lanes

`just test-os <profile>` runs `scripts/qemu-test.sh --profile=<profile>` (default `headless`).
The profile is positional: `just test-os PROFILE=x` is parsed as a second recipe name.
Each `ci-os-*` recipe wraps one profile with its timeout. Profiles live in
`source/apps/selftest-client/proof-manifest/profiles/harness.toml`; a profile declares its topology
(harts, icount) and its `REQUIRE_*` gates, and a contradicting caller env is a hard error.

| Recipe | Profile | What it adds |
|---|---|---|
| `just ci-os-headless` | `headless` | full service chain, no display (the CI lane) |
| `just ci-os-smp1` | `smp1` | one hart + icount: the deterministic gate |
| `just ci-os-smp` | `smp` | two harts, MTTCG, secondary-hart proofs, bounded retry; its clock is the host's wall clock, so the timing verdicts named in `config/fail-marker-allow-mttcg.txt` are rungs there, judged on `smp1` and the board |
| `just ci-os-visible` / `-fhd` / `-2d` | `visible*` | virgl (or 2D) compositor over VNC + host pixel proof |
| `just ci-os-usb` / `ci-os-usb-visible` | `usb*` | xHCI + hub + HID; the desktop driven over USB alone |
| `just ci-os-lowmem`, `ci-os-many-windows`, `ci-os-many-apps` | `lowmem`, `many-windows`, `many-apps` | declared scenarios: real memory pressure, twelve windows and a compositor restart, forty launches |
| `just ci-os-sdhci` | `sdhci` | system image on eMMC behind SDHCI, no virtio disk |
| `just ci-os-reset`, `ci-os-ota*` | `reset`, `ota-*` | reboot, A/B flip, bundles, tamper/downgrade/fallback |
| `just ci-os-dhcp`, `ci-os-dhcp-strict`, `ci-os-quic` | `dhcp*`, `quic-required` | network proofs (`just ci-network` aggregates them) |
| `just input-flood` | `visible` | ~900 pointer moves/s for 45 s; asserts no `alloc-fail` |

Triage helpers: `RUN_PHASE=<phase> just test-os` stops after a phase
(`bring-up`, `input-startup`, `mmio`, `routing`, `ota`, `policy`, `logd`, `vfs`, `end`) and on
failure prints `first_failed_phase=` and a bounded UART excerpt; `just test-mmio` is the `mmio`
shortcut. Runtime profiles (`NEXUS_SELFTEST_PROFILE=bringup|quick|none`, recipes
`ci-os-runtime-*`) make the guest skip phases instead.

Cold-boot durability (ADR-0044): `just test-os` seeds `SELFTEST: statefs cold-boot seeded`, then
`NEXUS_KEEP_BLK=1 REQUIRE_STATEFS_COLD_BOOT=1 just test-os` boots the preserved image and requires
`SELFTEST: statefs cold-boot persist ok`.

## Readiness contract (RFC-0013)

- `<svc>: ready` — the service serves its v1 contract. Missing it is a hard failure.
- `init: up <svc>` — init saw the service's `@ready` announce; it is not readiness by itself.

## Lane resources (ADR-0063)

A profile declares its memory envelope (`NEXUS_LANE_MEM_HIGH`/`_MEM_MAX`); `scripts/qemu-test.sh`
runs build + QEMU in a user cgroup scope with those limits and records `memory.peak` in
`hypothesis.json`. Without a cgroup scope (CI containers) it says so and runs unenforced. An
external signal writes `qemu-test: lane terminated externally (signal N)`. `build/` and `target/`
carry a `CACHEDIR.TAG`; exclude them from desktop file indexers by hand.

## Kernel code on the host

The `neuron` crate gates almost every module with `#[cfg(target_os = "none")]`, so their
`#[cfg(test)]` modules never compile on the host. A few pure modules are declared un-gated in
`source/kernel/neuron/src/lib.rs` (`waitset`, `fence`, `sync`, `frames`, `phys`, `dma_runs`, …)
and are the host oracle under `just test-kernel`. The rule for a new one: no riscv-only types
(raw `u32`/`u64` ids, `alloc` only), the gated syscall layer maps ids to kernel types, and the
integration is proven by a `KSELFTEST:` marker. The type gate for the rest is
`just diag-kernel` / `just lint-kernel` on `riscv64imac-unknown-none-elf`.

## CI

`.github/workflows/ci.yml` runs `just check`, `just test-host` + `just test-e2e`, Miri,
`just deadcode`, the kernel build/lint/tests, and a QEMU job (`scripts/qemu-test.sh --profile=headless`
then `just ci-os-smp`) that uploads `build/logs/` on failure. `.github/workflows/cla-check.yml` is
the CLA bot. The full ladder (`test-all`) runs locally; CI is the authority for the lanes it runs.

## Environment

`make initial-setup` provisions the host; `make doctor` (`scripts/check-deps.sh`) checks it and
names each fix. The toolchain is pinned in `rust-toolchain.toml` (targets
`riscv64gc-unknown-none-elf` for userspace and `riscv64imac-unknown-none-elf` for the kernel, `miri` for `test-all`). Headless lanes need no display stack;
`just start` and the virgl lanes need QEMU with GTK/EGL. Add a host package to
`scripts/install-deps.sh` and `podman/Containerfile` together.

## Logs

Every run writes `build/logs/<profile>--<timestamp>/` (`uart.log`, `hypothesis.json`,
`qemu.stderr`, boot-disk trace files); the board writes `build/logs/board--<timestamp>/`. Layout, the
`hypothesisId` grid and troubleshooting: [run-logs.md](run-logs.md). Prune with `just logs-gc`.

## Pages

| Page | Role |
|---|---|
| [layers.md](layers.md) | what each layer proves, one example each |
| [os-markers.md](os-markers.md) | marker truth, gates on the ladder, finding a marker |
| [proof-manifest.md](proof-manifest.md) | the manifest directory schema, profiles, scenarios |
| [e2e.md](e2e.md) | host E2E suites and coverage |
| [run-logs.md](run-logs.md) | run directory, `hypothesis.json`, troubleshooting |
| [network-distributed-debugging.md](network-distributed-debugging.md) | network lanes, `os2vm` |
| [evidence-bundle.md](evidence-bundle.md) | signed evidence bundle format |
| [replay-and-bisect.md](replay-and-bisect.md) | replay, trace diff, bisect |
| [SECURITY_STANDARDS.md](../standards/SECURITY_STANDARDS.md), [BUILD_STANDARDS.md](../standards/BUILD_STANDARDS.md) | security tests (`test_reject_*`), build hygiene gates |

Two JSON fixtures sit here for the replay tooling: `docs/testing/trace-diff-fixtures.json`
(trace-diff classes) and `docs/testing/bisect-good-drift-regress.json` (synthetic bisect map).
