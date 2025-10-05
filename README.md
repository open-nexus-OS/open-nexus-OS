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

This boots the NEURON kernel inside QEMU (riscv64 virt). Runs are wrapped with `timeout(1)` (default `RUN_TIMEOUT=30s`) and capture both diagnostics (`qemu.log`) and UART output (`uart.log`). Set `RUN_UNTIL_MARKER=1` for early exit on success markers or adjust `QEMU_LOG_MAX` / `UART_LOG_MAX` to retain a larger tail of each log. Set `DEBUG=1` to enable a GDB stub for debugging.

## Documentation

Start with the project overview in [`docs/overview.md`](docs/overview.md) and the testing guide in [`docs/testing/index.md`](docs/testing/index.md). Design notes and RFC templates live under [`docs/rfcs`](docs/rfcs/README.md). Submit proposals there before landing substantial architectural changes.

## Continuous Integration

CI configuration is not yet checked in. Integrate Podman-based builds, cargo-deny, formatting, and QEMU smoke once infrastructure is available.
