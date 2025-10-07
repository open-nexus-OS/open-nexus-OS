# Copyright 2024 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0

set shell := ["/usr/bin/env", "bash", "-c"]

toolchain := "nightly-2025-01-15"

default: test

# Build the bootable NEURON binary crate
build-kernel:
    cargo +{{toolchain}} build -p neuron-boot --target riscv64imac-unknown-none-elf --release

# Build only the kernel library with its own panic handler (no binary)
build-kernel-lib:
    cargo +{{toolchain}} build -p neuron --lib --features panic_handler --target riscv64imac-unknown-none-elf


qemu *args:
    # ensure the binary is built before launching
    just build-kernel
    RUN_TIMEOUT=${RUN_TIMEOUT:-30s} scripts/run-qemu-rv64.sh {{args}}

test-os:
    scripts/qemu-test.sh

test:
    cargo test -p neuron
    env RUSTFLAGS='--cfg nexus_env="host"' cargo test -p samgr -p bundlemgr

miri:
    cargo miri setup
    env MIRIFLAGS='--cfg nexus_env="host"' cargo miri test -p samgr -p bundlemgr

arch-check:
    cargo run -p arch-check
