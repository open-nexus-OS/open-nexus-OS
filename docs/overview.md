# Repository overview

This document summarises the purpose of each top-level directory in the Open Nexus OS tree. It focuses on how to find code and supporting assets rather than low-level architecture.

## Where to start

* [Kernel runtime (`source/kernel/neuron`)](../source/kernel/neuron/): reusable scheduler, IPC, and trap handling logic exercised via host-first unit tests.
* [Services (`source/services`)](../source/services/): thin Cap'n Proto adapters such as `samgrd` and `bundlemgrd` that translate IPC into userspace library calls.
* [Userspace libraries (`userspace`)](../userspace/): host-native crates like `samgr` and `bundlemgr` containing all business rules, property tests, and Miri coverage.
* [Host E2E tests (`tests/e2e`)](../tests/e2e/): in-process loopback integration exercising real IDL handlers without QEMU.

## `kernel/`
The workspace definitions and target configuration for the NEURON kernel live here. The kernel runtime itself is split between the reusable library crate (`source/kernel/neuron`) and the `neuron-boot` binary wrapper that provides the minimal `_start` entry point. No IDL parsing or userspace policy code lives in this layer.

## `source/`
Process sources compiled into deployable OS services and applications reside here. Long-running daemons live under `source/services/*d` and intentionally stay thin: they translate IPC messages into calls to userspace libraries rather than embedding business logic directly.

## `userspace/`
Domain libraries and SDK-style crates sit in this tree. They compile with `#![forbid(unsafe_code)]`, favour host execution, and are shaped for `cargo test` and `cargo miri`. These crates are the single source of truth for behavioural and business rules that daemons adapt.

## `tools/`
Developer tools, generators, and linters (such as the `nexus-idl` toolchain) live here. They support code generation, schema validation, and other workflows required during development.

## `recipes/`
Reusable build and development recipes—shell scripts, container launchers, and bootstrap instructions—are organised here so that developer machines and CI can share the same reproducible steps.

## `podman/`
Local container definitions that mirror CI images reside here. Building these images provides an environment identical to what CI uses, keeping toolchains and dependencies in sync.

## `config/`
Workspace-wide configuration for linting and tooling lives in this directory (for example, `clippy.toml`, `cargo-deny` manifests, and shared rustfmt settings).

## `scripts/`
Runner and helper scripts (including the QEMU invocations, log management utilities, and self-test harnesses) live here. These scripts are intended to be thin wrappers around reproducible developer workflows.

## `docs/`
All project documentation—including this overview, testing guides, architecture notes, and RFCs—stays in this tree. Every new process or workflow should land documentation here.

## How to navigate
* Kernel bring-up or architecture work begins in `source/kernel/neuron` (for reusable logic) and `source/kernel/neuron-boot` (for entry glue). Pair changes with updates to the runner scripts under `scripts/` when boot sequencing shifts.
* Service work (daemons, IPC endpoints, bundle/service managers) typically touches `source/services/*d` for the adapter layer and the relevant crates in `userspace/` for core business logic and testing.
* Shared logic or domain models belong in `userspace/`, while developer utilities (IDL generators, schema checkers) should be placed under `tools/`. Update `recipes/` or `podman/` when new dependencies are required so the development environment remains reproducible.
