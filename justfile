# Copyright 2024 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0

set shell := ["/usr/bin/env", "bash", "-c"]

default: test

build-kernel:
cargo build -p neuron --target riscv64imac-unknown-none-elf

qemu *args:
    scripts/run-qemu-rv64.sh {{args}}

test-os:
    scripts/qemu-test.sh

test:
    cargo test -p neuron

miri:
    cargo miri setup
    cargo miri test -p samgr -p bundlemgr

arch-check:
    cargo run -p arch-check
