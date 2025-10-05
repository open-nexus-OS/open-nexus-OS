<!--
Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
-->

# Project Layout Overview

The repository is structured to keep low-level kernel code, userspace
libraries, developer tooling, and infrastructure recipes clearly separated.
This page explains **why** each top-level directory exists and what you can
expect to find there when hacking on the Open Nexus operating system.

## `kernel/`
Core NEURON kernel crates. The `neuron` library contains architecture and
platform code compiled with `#![no_std]`, while `neuron-boot` is the freestanding
binary that firmware jumps into. There is no kernel-side Interface Definition
Language (IDL) parsing—only runtime-safe boot and core services live here.

## `source/`
Processes that run as OS services or applications. Daemons live in
`source/services/*d` and remain intentionally thin: they translate IPC messages
into calls to the userspace domain libraries, keeping policy and business logic
out of privileged processes.

## `userspace/`
Host-first libraries that model domains and protocols. These crates compile
with `#![forbid(unsafe_code)]`, are friendly to tools such as Miri, and serve as
the single source of truth for shared business logic across the platform.

## `tools/`
Developer productivity tooling including generators (such as `nexus-idl`),
linters, and other host utilities. Generators live here instead of inside the
kernel tree so they can depend on `std` and integrate with the build system
without polluting runtime crates.

## `recipes/`
Reproducible build and developer environment recipes. Use these scripts to
bootstrap toolchains, set up host dependencies, or spin up deterministic
container images for specific workflows.

## `podman/`
Container definitions that mirror the continuous integration environment. Build
these images locally to ensure the same compiler and system packages as the CI
pipeline.

## `config/`
Shared configuration for linting and quality gates (e.g. `clippy.toml`,
`rustfmt.toml`, `cargo-deny`). Centralizing policies keeps workspace crates in
sync and ensures reproducible builds.

## `scripts/`
Helper scripts for building, running, and testing the system. This includes the
QEMU runners, log trimming helpers, and automation wrappers for self-tests and
toolchain setup.

## `docs/`
All project documentation: high-level overviews, testing guides, architecture
notes, and RFCs. Start here when you need a conceptual map of the system or to
learn the expected engineering workflows.

## How to Navigate the Repository

- **Kernel changes:** begin in `kernel/`, updating `neuron` for core logic and
  `neuron-boot` for the entry binary. Keep cross-cutting helpers in userspace
  libraries when possible.
- **Service or daemon updates:** read the relevant module under
  `source/services/` and locate the shared library under `userspace/` that owns
  the business rules before touching the daemon adapter.
- **Library or API evolution:** update the crate in `userspace/`, extend its
  property and contract tests, and regenerate any tooling outputs via `tools/`.
- **Environment and tooling adjustments:** check `recipes/`, `podman/`, and
  `scripts/` for the existing automation before adding new scripts.
