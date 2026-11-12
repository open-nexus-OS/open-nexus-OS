---
schema: 1
title: "Boot chain"
role: guide
subsystem: kernel
sources: [source/boot/nxboot, source/kernel/neuron-boot, source/kernel/neuron, source/init/nexus-init, scripts/qemu-test.sh]
contracts: [ADR-0059, ADR-0081, ADR-0080, RFC-0098, RFC-0106, RFC-0107]
verified: 2026-11-12
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Boot chain

> **Purpose** — how the system gets from the VMM/firmware handoff to a running, proven userspace.
> **Owns** — `source/boot/nxboot/`, `source/kernel/neuron-boot/`, the kernel's boot path in `source/kernel/neuron/`, `source/init/nexus-init/`
> **Contracts** — [ADR-0059](../adr/0059-first-stage-boot-chain-nxboot-handoff.md), [ADR-0081](../adr/0081-the-boot-image-is-one-verified-container-kernel-plus-bootfs-every-core-program-runs-from-it-no-program-embeds-another.md), [ADR-0080](../adr/0080-a-process-runs-from-a-sealed-image-text-mapped-from-its-frames-data-copied-no-other-exec-path.md), [RFC-0098](../rfcs/RFC-0098-board-support-contract-fdt-truth-boot-chain.md), [RFC-0106](../rfcs/RFC-0106-soc-glue-one-owner-clocks-resets-power-pinmux.md), [RFC-0107](../rfcs/RFC-0107-boot-trace-console-markers-on-the-boot-disk.md)
> **Invariants** — no boot without the loader (a kernel started alone has no bootfs and stops); unverified bytes never run (failure is a stable `FAIL`/`PANIC` marker + reset); a marker is printed only after the behavior it names.
> **Read next** — [nexus-init.md](nexus-init.md), [execd.md](execd.md), [lifecycle.md](lifecycle.md), [testing README](../testing/README.md)

## The chain

| Stage | Code | What it does | Key markers |
|---|---|---|---|
| 0. Loader | `source/boot/nxboot/` | Static PIE, the VMM `-kernel` payload. Reads the device tree, picks the kernel window (lowest free 2 MiB-aligned window of the first bank), reads the BSB from the GPT disk, selects the slot (trial decrement before load), verifies the NXBD signature against `policies/os-trust.toml` + rollback floor + image sha256, jumps. | `nxboot: bsb ok …` → `nxboot: verify ok …` → `nxboot: bootfs (at= len=)` → `nxboot: jump slot=<s>` |
| 1. Boot wrapper | `source/kernel/neuron-boot/` | `_start`, clears `.bss`, installs the trap vector, enters `kmain`. | — |
| 2. Kernel | `source/kernel/neuron/` | HAL, memory map, traps, syscalls, scheduler, IPC, kernel selftests. Captures the loader's handoff page, adopts the bootfs, spawns init from its first file via `exec_vmo`. | `KSELFTEST: boot handoff ok (measured)`, `KSELFTEST: bootfs ok (files= bytes=)` |
| 3. Init | `source/init/nexus-init/` | Reads the bootfs table, spawns core services, grants devices, spawns volume services, wires routes, advances boot stages, supervises. | `init: start`, `init: start <svc>`, `init: up <svc>`, `init: ready` |
| 4. Services | `source/services/<name>d/` | Announce `@ready` once they can serve. | `<svc>: ready` |
| 5. Selftest | `source/apps/selftest-client/` | Observes contracts end-to-end (policy, exec, VFS, networking …). | `SELFTEST: … ok` |

## Loader details (stage 0)

- **Slot image = one container** (ADR-0081): flat kernel + bootfs (init, core services, app-host,
  recv-wake probe), packed by `nx image payload` from the build manifest under one NXBD digest.
  The loader places the bootfs at the first 2 MiB boundary past the kernel footprint and passes
  the range in `/chosen/nexus,bootfs-start` / `-len`. Nothing is copied at boot: each bootfs
  file becomes a sealed kernel object (ADR-0080).
- **Boot disk**: the first candidate (virtio block, SD host in the tree, SD host behind an ECAM
  host) that carries a valid BSB. Recorded in `/chosen/nexus,boot-disk`; init turns that record
  into the one grant `blkd` gets (`init: boot disk ok (…)`, `blkd: backend ok (…)`).
- **Measured handoff**: the boot record goes to `/chosen/nexus,boot-record` in the loader's copy
  of the tree.
- **SoC glue** (RFC-0106): one service, `socd`, holds every clock/reset/power/pad window
  (`device.mmio.syscon`) and brings a consumer's node up on request. No driver maps a syscon
  window (`scripts/check-no-platform-literals.sh`). QEMU `virt` has no glue:
  `socd: ready (no soc glue in this tree)`.
- **Boot trace** (RFC-0107): loader output is kept in the `trace` partition slot for this boot
  (`nxboot: trace slot=<n> seq=<m>`); the kernel's console ring is appended by `blkd`
  (`blkd: trace os ok (…)`). Read it back from a board with `just board-log`. QEMU lanes check
  the trace against the UART byte for byte.

## Devices (kernel → init)

- The kernel reads its platform (console, PLIC, timer, memory banks) from the tree in `a1` and
  hands init a read-only alias of it.
- Init discovers virtio transports and the RTC from the tree; every device capability carries
  the node's MMIO window and interrupt line (`init: devices from fdt ok (…)`, RFC-0098). A
  service standing on a device plane the tree lacks is not resumed
  (`init: withheld svc=<n> (no <plane> plane)`); no service probes for its device.
- Each ECAM host is planned with `nexus-pci` (the planner nxboot shares): BARs on their own
  pages, INTx routed, bus mastering left to the grant (`init: devices from pci ok (…)`).

## Marker ownership

| Owner | Markers |
|---|---|
| Kernel | MMU/SATP safety, `KSELFTEST: …` |
| Init | `init: start <svc>` (spawn requested), `init: up <svc>` (the service's `@ready` observed), `init: ready`, `stage: <name>` |
| Services | `<svc>: ready`, printed by `nexus_service_entry::ready` only |
| Harness | Acceptance: presence, order, required subsets |

Marker truth lives in `source/apps/selftest-client/proof-manifest/` and
`tools/nx/chains/markers.txt`; `scripts/qemu-test.sh` enforces them. Do not copy marker lists
into docs — link the manifest.

## Dev workflow

| Goal | Command |
|---|---|
| Build / run | `make build`, `make run`, or `just start` (interactive) |
| QEMU proof | `RUN_UNTIL_MARKER=1 just test-os` (profile positional: `just test-os <profile>`) |
| Stop after a phase | `RUN_PHASE=bring-up just test-os` (phases: `scripts/qemu-test.sh`, RFC-0014) |
| Logs | `build/logs/<profile>--<timestamp>/uart.log`, `build/logs/latest` |

On failure the harness prints `first_failed_phase=<name>` and a UART excerpt for that phase.
Change harness expectations only when behavior really changed; update
[docs/testing/README.md](../testing/README.md) when the contributor workflow changes.
