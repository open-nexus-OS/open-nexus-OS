---
schema: 1
title: "System map and boundaries"
role: guide
subsystem: devx
sources: [source, userspace, tools/arch-check, tools/nx]
contracts: [ADR-0001, ADR-0080, ADR-0081]
verified: 2026-11-10
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# System map and boundaries

> **Purpose** — what lives where in the tree, which component holds which role, and the boundaries that keep host-first development real.
> **Owns** — the repository layout (no code of its own)
> **Contracts** — [ADR-0001](../adr/0001-runtime-roles-and-boundaries.md) (one implementation per role), [ADR-0080](../adr/0080-a-process-runs-from-a-sealed-image-text-mapped-from-its-frames-data-copied-no-other-exec-path.md), [ADR-0081](../adr/0081-the-boot-image-is-one-verified-container-kernel-plus-bootfs-every-core-program-runs-from-it-no-program-embeds-another.md)
> **Invariants** — userspace crates never depend on services or the kernel. Domain logic lives in host-testable libraries; services are thin. Markers mean real behavior.
> **Read next** — [README.md](README.md), [concurrency.md](concurrency.md), [../kernel/README.md](../kernel/README.md)

## The tree

```text
source/
  boot/nxboot/          first-stage boot: verifies and loads the boot image (kernel + bootfs)
  kernel/neuron/        the microkernel (no_std): sched, mm, ipc, caps
  kernel/neuron-boot/   kernel entry stub (_start, relocations, high-half switch)
  init/nexus-init/      init orchestrator library: service graph, supervision, affinity
  apps/init-lite/       the OS init binary over nexus-init
  apps/selftest-client/ the proof client and its proof manifest
  services/<name>d/     userspace daemons (windowd, policyd, samgrd, execd, memd, …)
  drivers/              userspace drivers: gpud, virtio-*, sdhci, xhcid, socd, …
  libs/                 shared no_std libraries (nexus-abi, nexus-driverkit, nexus-wire, …)
userspace/              host-testable domain crates (nexus_env = host | os)
userspace/apps/         DSL `.nx` apps (desktop-shell, launcher, settings, …) and window-kit
tools/nx/               the nx CLI: DSL compiler, ledger, packaging, chain tests
tools/arch-check/       the layering guard
tests/                  host integration and E2E harnesses (no QEMU)
recipes/  policies/     service/app recipes · the policy tree and trust roots
scripts/                build, QEMU launch and proof harness
tasks/                  planning as data (RFC-0115) — execution truth
docs/                   pages, RFCs, ADRs
```

## Runtime roles

One implementation per role (ADR-0001):

| role | component |
|---|---|
| boot loader | `source/boot/nxboot/` — one verified container, every core program runs from it (ADR-0081) |
| init / supervisor | `source/init/nexus-init/` run as `source/apps/init-lite/` |
| spawner and reaper | `source/services/execd/` |
| loader | the kernel's `exec_vmo` from a sealed image (ADR-0080); image layout rule in `tools/exec-image/` |
| service registry | `source/services/samgrd/` over `userspace/samgr/` |
| policy authority | `source/services/policyd/` with `policies/` |
| memory authority | `source/services/memd/` |
| compositor | `source/services/windowd/`; GPU driver `source/drivers/gpud/` |
| app runtime | `source/services/app-host/` + `userspace/apps/window-kit/` |
| logging | `source/services/logd/` behind `source/libs/nexus-log/` |
| proof | `source/apps/selftest-client/` + `source/apps/selftest-client/proof-manifest/` |

## Boundaries

- **No userspace → OS dependency.** `tools/arch-check` (`just arch-check`) fails when a crate under
  `userspace/` depends on a crate under `source/services/`, on `samgrd` or on the kernel.
  `nexus-abi` is the shared ABI and allowed.
- **Services are thin.** A daemon wires IPC, validates input and forwards into a `userspace/` crate
  built with `nexus_env="os"`. Business logic is tested on the host.
- **Kernel is minimal.** No policy, crypto, IDL parsing or drivers in `source/kernel/`.
- **Window UI is widgets.** windowd composes; window chrome and controls live in app-host widgets.
- **No fake green.** A `*: ready` or `SELFTEST: … ok` marker only after real behavior. Marker truth:
  `source/apps/selftest-client/proof-manifest/` and `tools/nx/chains/markers.txt`.
- **Contracts in the right place.** Tasks own scope and proof, RFCs own interfaces and invariants,
  ADRs own narrow decisions, pages explain how pieces connect.

## When you change something

| change | also update |
|---|---|
| behavior | the task's proof (`nx ledger`), the RFC/ADR if the contract moved |
| a marker | `scripts/qemu-test.sh`, `tools/nx/chains/markers.txt`, the proof manifest, [../testing/os-markers.md](../testing/os-markers.md) |
| a new syscall, service API or wire format | an RFC seed first (`docs/rfcs/RFC-TEMPLATE.md`) |
| a boundary crossing | an ADR |
