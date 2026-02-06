#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
QEMU_DIR="$ROOT/tools/qemu-src"
PATCH="$ROOT/tools/qemu/virtio-mmio-force-modern.patch"

if [[ ! -d "$QEMU_DIR" ]]; then
  echo "[qemu] cloning source..."
  git clone --depth 1 https://gitlab.com/qemu-project/qemu.git "$QEMU_DIR"
fi

cd "$QEMU_DIR"

if ! grep -n "force-legacy" hw/virtio/virtio-mmio.c | grep -q "false"; then
  echo "[qemu] applying force-modern patch..."
  git apply "$PATCH"
fi

echo "[qemu] configure..."
./configure --target-list=riscv64-softmmu

echo "[qemu] build..."
ninja -C build

cat <<'EOF'
[qemu] done
Use this QEMU by prepending PATH:
  export PATH="/home/jenning/open-nexus-OS/tools/qemu-src/build:$PATH"
EOF
