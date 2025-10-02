# open-nexus-os

Open Nexus OS is a research microkernel targeting RISC-V virt hardware with an OpenHarmony-inspired userland. This repository contains the workspace scaffold, toolchain configuration, and host-first testing flow for building the NEURON kernel and its companion services.

## Build

```sh
make build
```

This invokes the containerized toolchain (Podman rootless) to compile the entire workspace.

## Test

```sh
make test
```

All unit, contract, and headless UI tests execute on the host before any QEMU smoke testing.

## Run

```sh
make run
```

This boots the NEURON kernel inside QEMU (riscv64 virt). Set `DEBUG=1` to enable a GDB stub for debugging.

## Documentation

Design notes and RFC templates live under [`docs/rfcs`](docs/rfcs/README.md). Submit proposals there before landing substantial architectural changes.

## Continuous Integration

CI configuration is not yet checked in. Integrate Podman-based builds, cargo-deny, formatting, and QEMU smoke once infrastructure is available.
