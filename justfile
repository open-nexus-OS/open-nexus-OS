# Copyright 2024 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0

set shell := ["/usr/bin/env", "bash", "-c"]

default: test

# Build the bootable NEURON binary crate
build-kernel:
    cargo build -p neuron-boot --target riscv64imac-unknown-none-elf --release

# Build only the kernel library with its own panic handler (no binary)
build-kernel-lib:
    cargo build -p neuron --lib --features panic_handler --target riscv64imac-unknown-none-elf
 

qemu *args:
    # ensure the binary is built before launching
    just build-kernel
    scripts/run-qemu-rv64.sh {{args}}

test-os:
    scripts/qemu-test.sh

test:
    cargo test -p neuron
    cargo test -p samgr -p bundlemgr --features backend-host

miri:
    cargo miri setup
    cargo miri test -p samgr -p bundlemgr --features backend-host

arch-check:
    cargo run -p arch-check
