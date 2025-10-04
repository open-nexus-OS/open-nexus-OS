#!/usr/bin/env bash
# Copyright 2024 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
TARGET=${TARGET:-riscv64imac-unknown-none-elf}
KERNEL_ELF=$ROOT/target/$TARGET/release/neuron-boot

if [ ! -f "$KERNEL_ELF" ]; then
  (cd "$ROOT" && cargo build -p neuron-boot --target "$TARGET" --release)
fi

COMMON_ARGS=(
  -machine virt
  -cpu rv64
  -m 256M
  -smp "${SMP:-1}"
  -nographic
  -kernel "$KERNEL_ELF"
  -bios default
)

exec qemu-system-riscv64 "${COMMON_ARGS[@]}" "$@"
