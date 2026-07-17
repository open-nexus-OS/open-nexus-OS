<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# E2E harnesses and environment parity

Policy E2E and Remote E2E harness notes plus the environment parity & prerequisites contract. Split out of the former `docs/testing/index.md`; see [README.md](README.md) for the entry point and [e2e-coverage-matrix.md](e2e-coverage-matrix.md) for the feature-by-feature coverage map.

## Policy E2E notes

- Host coverage lives in `tests/e2e_policy/`. The crate spins up loopback
  instances of `policyd`, `bundlemgrd`, and `execd`, installs two manifests, and
  asserts both the allow and deny responses along with the returned missing
  capabilities.
- The OS run mirrors this by loading policies at boot and printing
  `SELFTEST: policy allow ok` / `SELFTEST: policy deny ok` markers from
  `selftest-client` once the simulated policy checks succeed.
- Policies are stored under `recipes/policy/`. Merge order is lexical; later
  files override earlier definitions. For development overrides drop a
  `local-*.toml` file so it sorts after `base.toml`.

Cap'n Proto remains a userland concern. Large payloads (e.g. bundle artifacts) are transferred via VMO handles on the OS; on the host these handles are emulated by staging bytes in the bundle manager's artifact store before issuing control-plane requests.

## Remote E2E harness

The remote harness in `tests/remote_e2e` proves that two host nodes can discover
each other, authenticate sessions using Noise XK, and forward Cap'n Proto
traffic over the encrypted stream. Each node hosts real `samgrd` and
`bundlemgrd` loops via in-process IPC, while the identity keys are derived using
the shared `userspace/identity` crate. Artifact transfers are staged over a
dedicated DSoftBus channel before issuing the install request, mirroring the VMO
hand-off used by the OS build today. Execute the tests with
`cargo test -p remote_e2e`—they finish in a few seconds and require no QEMU.

The DSoftBus OS backend is implemented (`TASK-0003` through `TASK-0005`) and the
daemon orchestration is now modularized (`TASK-0015`): `source/services/dsoftbusd/src/main.rs`
is a thin entry/wiring layer, with host seam coverage in
`source/services/dsoftbusd/tests/p0_unit.rs`,
`source/services/dsoftbusd/tests/reject_transport_validation.rs`, and
`source/services/dsoftbusd/tests/session_steps.rs`.

## Environment parity & prerequisites

- Toolchain pinned via `rust-toolchain.toml`; install the listed version before building.
- Targets required: `rustup target add riscv64imac-unknown-none-elf`.
- System dependencies: `qemu-system-misc`, `capnproto`, and supporting build packages. The Podman container image installs the same dependencies for CI parity.
- Do not rely on host-only tools—update `recipes/` or container definitions when new packages are needed.
