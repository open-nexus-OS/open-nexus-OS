#!/usr/bin/env bash
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# Builds `rasterbench` three ways with the repo's pinned toolchain (TASK-0362 P0):
#   soft   riscv64imac  — the image's userspace target today
#   hard   riscv64gc    — F and D
#   hardv  riscv64gc+v  — F, D and the vector extension (the compiler's own vectorisation)
# The flags are the OS image's (`.cargo/config.toml`), only the float and vector features differ.
#
#   docs/board/measurements/2026-11-12-float-and-vectors/rasterbench/build.sh [out-dir]
#   for b in soft hard hardv; do adb push <out-dir>/rasterbench-$b /tmp/; adb shell /tmp/rasterbench-$b; done
#
# The two riscv64gc builds need the target's core library once:
#   rustup target add riscv64gc-unknown-none-elf
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
out="${1:-${TMPDIR:-/tmp}/rasterbench}"
target_dir="${CARGO_TARGET_DIR:-$here/target}"
mkdir -p "$out"

build() {
  local name="$1" target="$2" features="$3"
  RUSTFLAGS="--check-cfg=cfg(nexus_env,values(\"host\",\"os\")) --cfg nexus_env=\"os\" \
-C relocation-model=static -C target-feature=$features" \
    cargo build --release --manifest-path "$here/Cargo.toml" --target "$target" \
    --target-dir "$target_dir"
  /bin/cp "$target_dir/$target/release/rasterbench" "$out/rasterbench-$name"
  echo "built $out/rasterbench-$name ($target, $features)"
}

build soft riscv64imac-unknown-none-elf +zbb
build hard riscv64gc-unknown-none-elf +zbb
build hardv riscv64gc-unknown-none-elf +zbb,+v
