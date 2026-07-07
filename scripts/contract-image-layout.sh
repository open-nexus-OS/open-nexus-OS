#!/usr/bin/env bash
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: P0.1 layout-fragility gate — proves the boot survives image-size
# perturbation. Byte-shifts in the kernel/init image once broke DISTANT
# subsystems silently (StackPool cursor corruption → StackExhausted; the
# zero-guards in task/mod.rs and alloc_init_page are scars of it). The boot
# now carries loud tripwires (`KERNEL: layout ok` + headroom, STACK-POOL /
# VMO-POOL value reports); this gate perturbs the image and demands the
# ladder stays green — so a re-emergence is caught in CI, not in a user boot.
#
# Mechanism: the kernel embeds a REFERENCED rodata pad sized by the
# NEURON_LAYOUT_PAD env var (kmain assert_memory_layout — volatile-read, so
# no linker pass can collect it; the banner reports `pad=N`). The earlier
# approach — appending an unreferenced `#[used]` static to a tracked file —
# was a PLACEBO: gc-sections dropped it, which this gate's own landing check
# caught. Env-dependency tracking makes cargo rebuild the kernel per value.
#
# Usage: scripts/contract-image-layout.sh   (expects a completed build)
set -euo pipefail
cd "$(dirname "$0")/.."

BASE_IMAGE_END=""

run_boot() {
  local tag="$1"
  local pad="$2"
  NEURON_LAYOUT_PAD="$pad" RUN_TIMEOUT=40s timeout 200 just start-vnc \
    >"build/logs/perturb-$tag.log" 2>&1 || true
  local log
  log="$(ls -td build/logs/manual--* | head -1)/uart.log"
  if ! grep -q "KERNEL: layout ok" "$log"; then
    echo "contract-image-layout: FAIL ($tag) — no 'KERNEL: layout ok' marker"; exit 1
  fi
  if grep -qE "StackExhausted|STACK-POOL|VMO-POOL exhausted|LAYOUT:" "$log"; then
    echo "contract-image-layout: FAIL ($tag) — layout tripwire fired:"; grep -E "StackExhausted|STACK-POOL|VMO-POOL|LAYOUT:" "$log"; exit 1
  fi
  if ! grep -q "full-window color visible" "$log"; then
    echo "contract-image-layout: FAIL ($tag) — visible chain missing"; exit 1
  fi
  # Landing checks (a pad that is not in the image is a placebo gate):
  # the banner must carry the armed pad size AND the image end must move.
  if ! grep -q "pad=$pad)" "$log"; then
    echo "contract-image-layout: FAIL ($tag) — banner pad mismatch (want pad=$pad):"
    grep -o "KERNEL: layout ok.*" "$log" | head -1
    exit 1
  fi
  local image_end
  image_end="$(grep -o 'image_end=0x[0-9a-f]*' "$log" | head -1)"
  if [[ "$tag" == "baseline" ]]; then
    BASE_IMAGE_END="$image_end"
  elif [[ "$image_end" == "$BASE_IMAGE_END" ]]; then
    echo "contract-image-layout: FAIL ($tag) — pad did not land ($image_end unchanged)"; exit 1
  fi
  echo "contract-image-layout: OK ($tag) — $image_end $(grep -o 'headroom=[0-9]*K' "$log")"
}

echo "== baseline =="
run_boot baseline 0

for pad in 4096 8192 65536; do
  echo "== perturb +${pad}B =="
  run_boot "pad$pad" "$pad"
done

# Restore the unpadded kernel in the build tree (env-dep tracking rebuilds it).
env NEURON_LAYOUT_PAD=0 EMBED_INIT_ELF="$PWD/target/riscv64imac-unknown-none-elf/release/init-lite" \
  RUSTFLAGS='--check-cfg=cfg(nexus_env,values("host","os")) --cfg nexus_env="os"' \
  cargo build -p neuron-boot --target riscv64imac-unknown-none-elf --release >/dev/null 2>&1
echo "contract-image-layout: ALL GREEN (baseline + 3 perturbations)"
