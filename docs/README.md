<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Documentation index

Start here. This page maps the documentation tree and the repository layout.
Authority model: `tasks/` is execution truth, `docs/rfcs/` are design
contracts, `docs/adr/` are architecture decisions — when they conflict, the
task ledger wins for status, the RFC wins for the contract.

## Documentation tree

| Directory | Contents |
|---|---|
| [`architecture/`](architecture/README.md) | Canonical architecture entry point: layering, kernel, SMP, storage, UI/windowing, DSL platform, graphics (`graphics/`), inference (`inference/`), vision |
| [`rfcs/`](rfcs/README.md) | Design contracts (RFC-0001…) with index + template |
| [`adr/`](adr/README.md) | Architecture decision records (ADR-0001…) with index + template |
| [`testing/`](testing/README.md) | Test layers, QEMU profiles, marker ladder, E2E, troubleshooting |
| [`dev/`](dev/) | Developer guides: DSL (`dev/dsl/`), UI design system (`dev/ui/`), SDK, platform, performance, `nx` CLI, configuration |
| [`standards/`](standards/) | Rust / build / security / documentation standards |
| [`security/`](security/) | Authority model, signing & policy, identity & sessions, policy as code |
| [`storage/`](storage/) | nxfs, VFS v2, statefs, VMO semantics |
| [`distributed/`](distributed/) | DSoftBus lite/mux, remote-fs |
| [`observability/`](observability/) | Logging, metrics, tracing |
| [`services/`](services/) | Service lifecycle, os-lite backends |
| [`packaging/`](packaging/) | Artifact kinds, `.nxb` format, system set, A/B update skeleton |
| [`supplychain/`](supplychain/) | Reproducibility, SBOM, signing policy |

Agent guidance lives at the repo root: `CLAUDE.md` (SSOT) and `AGENTS.md`
(pointer). Run logs and their decode reference: `docs/testing/run-logs.md`.

## Repository layout

- `source/kernel/neuron/` — NEURON microkernel library (no_std, RISC-V);
  `source/kernel/neuron-boot` is the minimal `_start` binary wrapper.
- `source/services/*d` — long-running daemons; thin adapters that register
  with `samgrd`, expose IDL bindings, and forward into userspace crates.
- `source/apps/` — system-level applications (privileged, `nexus_env="os"`),
  e.g. `selftest-client/`.
- `source/libs/` — core libraries (`nexus-abi`, `nexus-ipc`, `nexus-gfx`,
  `nexus-display-proto`, `nexus-driverkit`, `nexus-virtio`, …).
- `userspace/` — domain libraries and SDK crates: `#![forbid(unsafe_code)]`,
  host-first, shaped for `cargo test` and Miri; the single source of truth
  for business rules that daemons adapt. `userspace/apps/` holds sandboxed
  user applications deployed as bundles.
- `tools/` — developer tools: `nx` (DSL compiler/CLI + chain tests),
  `nexus-idl`, `arch-check`, generators and linters.
- `tests/e2e/` — in-process loopback integration tests (no QEMU).
- `recipes/` — reproducible build/development recipes shared by devs and CI.
- `schemas/` — JSON/TOML schemas (incl. `recipe-schema.toml`).
- `policies/` — live policy tree (`nexus.policy.toml`).
- `config/` — lint/deny/rustfmt/toolchain configuration.
- `scripts/` — build/QEMU/check scripts (thin, reproducible wrappers).
- `podman/` — container definitions mirroring CI images.
- `resources/` — fonts, icons, themes, wallpapers, manifests
  (see `../resources/README.md`).
- `tasks/` — task ledgers (execution truth), status board, tracks.

## How to navigate

- **Kernel work** starts in `source/kernel/neuron` (library) and
  `source/kernel/neuron-boot` (entry glue); pair with `scripts/` updates when
  boot sequencing shifts. See `architecture/01-neuron-kernel.md`.
- **Service work** touches `source/services/*d` (adapter) plus the matching
  `userspace/` crate (logic + tests). See
  `architecture/08-service-architecture-onboarding.md`.
- **App development**: declarative `.nx` apps (see `dev/dsl/overview.md`);
  `userspace/apps/` for sandboxed bundles, `source/apps/` for privileged
  system apps.
- **Boot-gate guidance**: RFC-0013 defines readiness vs ready, spawn failure
  reasons, and resource/leak sentinels; diagnostics via
  `testing/README.md` and `docs/testing/run-logs.md`.
