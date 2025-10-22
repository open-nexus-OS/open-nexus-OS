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
KERNEL_BIN=$ROOT/target/$TARGET/release/neuron-boot.bin
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
  local saw_ready=0
  local saw_elf_ok=0
  local saw_child=0
  local saw_exit_log=0
  local saw_child_exit_ok=0
  local saw_pkgfs_ready=0
  local saw_vfsd_ready=0
  local saw_init_up_keystored=0
  local saw_init_up_policyd=0
  local saw_init_up_samgrd=0
  local saw_init_up_bundlemgrd=0
  local saw_init_up_packagefsd=0
  local saw_init_up_vfsd=0
  local saw_init_up_execd=0
  while IFS= read -r line; do
    case "$line" in
      *"init: ready"*)
        saw_ready=1
        ;;
      *"init: up keystored"*)
        saw_init_up_keystored=1
        ;;
      *"init: up policyd"*)
        saw_init_up_policyd=1
        ;;
      *"init: up samgrd"*)
        saw_init_up_samgrd=1
        ;;
      *"init: up bundlemgrd"*)
        saw_init_up_bundlemgrd=1
        ;;
      *"init: up packagefsd"*)
        saw_init_up_packagefsd=1
        ;;
      *"init: up vfsd"*)
        saw_init_up_vfsd=1
        ;;
      *"init: up execd"*)
        saw_init_up_execd=1
        ;;
      *"packagefsd: ready"*)
        saw_pkgfs_ready=1
        ;;
      *"vfsd: ready"*)
        saw_vfsd_ready=1
        ;;
      *"execd: elf load ok"*)
        saw_elf_ok=1
        ;;
      *"child: hello-elf"*)
        saw_child=1
        ;;
      *"execd: child exited pid="*)
        saw_exit_log=1
        ;;
      *"SELFTEST: child exit ok"*)
        saw_child_exit_ok=1
        ;;
      *"SELFTEST: e2e exec-elf ok"*)
        if [[ "$saw_ready" -eq 1 && "$saw_pkgfs_ready" -eq 1 && "$saw_vfsd_ready" -eq 1 && "$saw_elf_ok" -eq 1 && "$saw_child" -eq 1 && "$saw_exit_log" -eq 1 && "$saw_child_exit_ok" -eq 1 \
          && "$saw_init_up_keystored" -eq 1 && "$saw_init_up_policyd" -eq 1 && "$saw_init_up_samgrd" -eq 1 && "$saw_init_up_bundlemgrd" -eq 1 && "$saw_init_up_packagefsd" -eq 1 && "$saw_init_up_vfsd" -eq 1 && "$saw_init_up_execd" -eq 1 ]]; then
          echo "[info] Success marker detected – stopping QEMU" >&2
          pkill -f qemu-system-riscv64 >/dev/null 2>&1 || true
          break
        fi
        ;;
      *"I: after selftest"*|*"KSELFTEST: spawn ok"*|*"SELFTEST: ipc ok"*|*"SELFTEST: end"*)
        if [[ "$RUN_UNTIL_MARKER" != "1" ]]; then
          echo "[info] Success marker detected – stopping QEMU" >&2
          pkill -f qemu-system-riscv64 >/dev/null 2>&1 || true
          break
        fi
        ;;
      *"EXC: scause="*|*"PANIC "*|*"SELFTEST: fail"*|*"ILLEGAL"*|*"rx guard:"*)
        if [[ "$RUN_UNTIL_MARKER" != "1" ]]; then
          echo "[warn] Exception/Panic marker detected – stopping QEMU early for triage" >&2
          pkill -f qemu-system-riscv64 >/dev/null 2>&1 || true
          break
        fi
        ;;
      *"LOCKDEP:"*|*"PT-VERIFY:"*|*"HEAP: "*)
        if [[ "$RUN_UNTIL_MARKER" != "1" ]]; then
          echo "[warn] Kernel debug-hardening marker detected – stopping QEMU for triage" >&2
          pkill -f qemu-system-riscv64 >/dev/null 2>&1 || true
          break
        fi
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

if [[ ! -f "$KERNEL_BIN" || "$KERNEL_BIN" -ot "$KERNEL_ELF" ]]; then
  # Use Rust's llvm-objcopy (works with all targets)
  OBJCOPY=$(find ~/.rustup/toolchains -name llvm-objcopy -type f 2>/dev/null | head -1)
  if [[ -z "$OBJCOPY" ]]; then
    echo "[error] llvm-objcopy not found. Install llvm-tools: rustup component add llvm-tools-preview" >&2
    exit 1
  fi
  "$OBJCOPY" -O binary "$KERNEL_ELF" "$KERNEL_BIN"
fi

rm -f "$QEMU_LOG" "$UART_LOG"

COMMON_ARGS=(
  -machine virt,aclint=on
  -cpu max
  -m 265M
  -smp "${SMP:-1}"
  -nographic
  -serial mon:stdio
  -icount 1,sleep=on
  -bios default
  -kernel "$KERNEL_BIN"
)

# Enable heavy QEMU tracing only when explicitly requested
if [[ "${QEMU_TRACE:-0}" == "1" ]]; then
  TRACE_FLAGS=${QEMU_TRACE_FLAGS:-int,mmu,unimp}
  COMMON_ARGS+=( -d "$TRACE_FLAGS" -D "$QEMU_LOG" )
fi

# Optional GDB stub for interactive debugging
if [[ "${QEMU_GDB:-0}" == "1" ]]; then
  COMMON_ARGS+=( -S -s -monitor none )
fi

status=0
if [[ "$RUN_UNTIL_MARKER" == "1" ]]; then
  set +e
  timeout --foreground "$RUN_TIMEOUT" stdbuf -oL qemu-system-riscv64 "${COMMON_ARGS[@]}" "$@" \
    | tee >(monitor_uart) \
    | tee "$UART_LOG"
  status=${PIPESTATUS[0]}
  set -e
else
  set +e
  timeout --foreground "$RUN_TIMEOUT" stdbuf -oL qemu-system-riscv64 "${COMMON_ARGS[@]}" "$@" \
    | tee "$UART_LOG"
  status=${PIPESTATUS[0]}
  set -e
fi

finish "$status"
