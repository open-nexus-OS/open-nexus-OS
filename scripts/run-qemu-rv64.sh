#!/usr/bin/env bash
# Copyright 2024 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0

# Environment knobs:
#   RUN_TIMEOUT      – timeout(1) duration before QEMU is terminated (default: 30s)
#   RUN_UNTIL_MARKER – when "1", stop QEMU once a success UART marker is printed (default: 0)
#   QEMU_LOG_MAX     – maximum size of qemu.log after trimming (default: 52428800 bytes)
#   UART_LOG_MAX     – maximum size of uart.log after trimming (default: 10485760 bytes)
#   QEMU_LOG / UART_LOG – override log file paths.

set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
TARGET=${TARGET:-riscv64imac-unknown-none-elf}
KERNEL_ELF=$ROOT/target/$TARGET/release/neuron-boot
RUN_TIMEOUT=${RUN_TIMEOUT:-30s}
RUN_UNTIL_MARKER=${RUN_UNTIL_MARKER:-0}
QEMU_LOG_MAX=${QEMU_LOG_MAX:-52428800}
UART_LOG_MAX=${UART_LOG_MAX:-10485760}
QEMU_LOG=${QEMU_LOG:-qemu.log}
UART_LOG=${UART_LOG:-uart.log}

# Continuous QEMU tracing can easily balloon into tens of gigabytes; trim the
# tail post-run to keep CI artifacts and local logs manageable.
trim_log() {
  local file=$1 max=$2
  if [[ -f "$file" ]]; then
    local sz
    sz=$(wc -c <"$file" || echo 0)
    if [[ "$sz" -gt "$max" ]]; then
      echo "[info] Trimming $file from ${sz} bytes to last $max bytes" >&2
      tail -c "$max" "$file" >"${file}.tmp" && mv "${file}.tmp" "$file"
    fi
  fi
}

monitor_uart() {
  local line
  while IFS= read -r line; do
    case "$line" in
      *"SELFTEST: end"*|*"samgrd: ready"*|*"bundlemgrd: ready"*)
        echo "[info] Success marker detected – stopping QEMU" >&2
        pkill -f qemu-system-riscv64 >/dev/null 2>&1 || true
        break
        ;;
    esac
  done
}

finish() {
  local status=$1
  trim_log "$QEMU_LOG" "$QEMU_LOG_MAX"
  trim_log "$UART_LOG" "$UART_LOG_MAX"
  if [[ "$status" -eq 143 && "$RUN_UNTIL_MARKER" == "1" ]]; then
    echo "[info] QEMU stopped after success marker" >&2
    status=0
  fi
  if [[ "$status" -eq 124 ]]; then
    echo "[warn] QEMU terminated after exceeding timeout ($RUN_TIMEOUT)" >&2
  fi
  return "$status"
}

if [[ ! -f "$KERNEL_ELF" ]]; then
  (cd "$ROOT" && cargo build -p neuron-boot --target "$TARGET" --release)
fi

rm -f "$QEMU_LOG" "$UART_LOG"

COMMON_ARGS=(
  -machine virt
  -cpu rv64
  -m 256M
  -smp "${SMP:-1}"
  -nographic
  -kernel "$KERNEL_ELF"
  -bios default
  -d int,mmu,unimp
  -D "$QEMU_LOG"
)

status=0
if [[ "$RUN_UNTIL_MARKER" == "1" ]]; then
  set +e
  timeout "$RUN_TIMEOUT" stdbuf -oL qemu-system-riscv64 "${COMMON_ARGS[@]}" "$@" \
    | tee >(monitor_uart) \
    | tee "$UART_LOG"
  status=${PIPESTATUS[0]}
  set -e
else
  set +e
  timeout "$RUN_TIMEOUT" stdbuf -oL qemu-system-riscv64 "${COMMON_ARGS[@]}" "$@" \
    | tee "$UART_LOG"
  status=${PIPESTATUS[0]}
  set -e
fi

finish "$status"
