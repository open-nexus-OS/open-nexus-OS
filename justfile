# Copyright 2024 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0

set shell := ["/usr/bin/env", "bash", "-c"]

default: test

build-kernel:
cargo build -p neuron --target riscv64imac-unknown-none-elf

qemu:
scripts/run-qemu-rv64.sh

test:
cargo test -p neuron
