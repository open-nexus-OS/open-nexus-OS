#!/usr/bin/env bash
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# Opt-in 2-VM harness for cross-VM DSoftBus proof (TASK-0005 / RFC-0010).
#
# Key design choice (determinism + speed):
# - Build once (services + init-lite + kernel) to avoid Cargo file-lock contention.
# - Then launch two QEMU instances that share an L2 hub via socket/mcast.
# - Evidence is written to *.txt files (repo ignores *.log).
#
# Usage:
#   RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
if [[ "${RUN_OS2VM:-0}" != "1" ]]; then
  echo "[error] RUN_OS2VM is not set to 1. Refusing to run 2-VM harness." >&2
  echo "[info] Usage: RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh" >&2
  exit 2
fi

TARGET=${TARGET:-riscv64imac-unknown-none-elf}
RUSTFLAGS_OS=${RUSTFLAGS_OS:---check-cfg=cfg(nexus_env,values(\"host\",\"os\")) --cfg nexus_env=\"os\"}
RUN_TIMEOUT=${RUN_TIMEOUT:-180s}
MARKER_TIMEOUT=${MARKER_TIMEOUT:-300}
LOG_DIR=${LOG_DIR:-.}
OS2VM_PCAP=${OS2VM_PCAP:-0}

A_MAC=${A_MAC:-52:54:00:12:34:0a}
B_MAC=${B_MAC:-52:54:00:12:34:0b}

# QEMU socket backend:
# - Default: deterministic point-to-point link (listen/connect) on localhost.
# - Optional: multicast hub (set NETDEV_A=NETDEV_B to the same mcast string).
NETDEV_A=${NETDEV_A:--netdev socket,id=n0,listen=127.0.0.1:37021}
NETDEV_B=${NETDEV_B:--netdev socket,id=n0,connect=127.0.0.1:37021}

UART_A="$LOG_DIR/uart-A.txt"
UART_B="$LOG_DIR/uart-B.txt"
HOST_A="$LOG_DIR/host-A.txt"
HOST_B="$LOG_DIR/host-B.txt"
PCAP_A="$LOG_DIR/os2vm-A.pcap"
PCAP_B="$LOG_DIR/os2vm-B.pcap"

mkdir -p "$LOG_DIR"
: >"$UART_A"
: >"$UART_B"
: >"$HOST_A"
: >"$HOST_B"
if [[ "$OS2VM_PCAP" == "1" ]]; then
  : >"$PCAP_A"
  : >"$PCAP_B"
fi

wait_marker() {
  local file=$1
  local pattern=$2
  local deadline=$(( $(date +%s) + MARKER_TIMEOUT ))
  while (( $(date +%s) < deadline )); do
    if [[ -s "$file" ]] && grep -q "$pattern" "$file" 2>/dev/null; then
      return 0
    fi
    sleep 1
  done
  return 1
}

set_env_var() {
  local name=$1
  local value=$2
  printf -v "$name" '%s' "$value"
  export "$name"
}

build_os_once() {
  export RUSTFLAGS="$RUSTFLAGS_OS"

  local services="keystored,rngd,policyd,samgrd,bundlemgrd,packagefsd,vfsd,execd,netstackd,dsoftbusd,selftest-client"
  export INIT_LITE_SERVICE_LIST="$services"

  IFS=',' read -r -a svcs <<<"$services"
  for raw in "${svcs[@]}"; do
    local svc=${raw//[[:space:]]/}
    [[ -z "$svc" ]] && continue

    (cd "$ROOT" && RUSTFLAGS="$RUSTFLAGS_OS" cargo build -p "$svc" --target "$TARGET" --release --no-default-features --features os-lite)

    local svc_upper
    svc_upper=$(echo "$svc" | tr '[:lower:]' '[:upper:]' | tr '-' '_')
    set_env_var "INIT_LITE_SERVICE_${svc_upper}_ELF" "$ROOT/target/$TARGET/release/$svc"
    local stack_var="INIT_LITE_SERVICE_${svc_upper}_STACK_PAGES"
    if [[ -z "${!stack_var:-}" ]]; then
      set_env_var "$stack_var" "8"
    fi
  done

  (cd "$ROOT" && RUSTFLAGS="$RUSTFLAGS_OS" cargo build -p init-lite --target "$TARGET" --release)

  local INIT_ELF="$ROOT/target/$TARGET/release/init-lite"
  (cd "$ROOT" && EMBED_INIT_ELF="$INIT_ELF" RUSTFLAGS="$RUSTFLAGS_OS" cargo build -p neuron-boot --target "$TARGET" --release)

  local KERNEL_ELF="$ROOT/target/$TARGET/release/neuron-boot"
  local KERNEL_BIN="$ROOT/target/$TARGET/release/neuron-boot.bin"
  if [[ ! -f "$KERNEL_BIN" || "$KERNEL_BIN" -ot "$KERNEL_ELF" ]]; then
    local OBJCOPY
    OBJCOPY=$(find ~/.rustup/toolchains -name llvm-objcopy -type f 2>/dev/null | head -1)
    if [[ -z "$OBJCOPY" ]]; then
      echo "[error] llvm-objcopy not found. Install llvm-tools: rustup component add llvm-tools-preview" >&2
      exit 1
    fi
    "$OBJCOPY" -O binary "$KERNEL_ELF" "$KERNEL_BIN"
  fi
}

launch_qemu() {
  local name=$1
  local mac=$2
  local uart=$3
  local hostlog=$4

  local KERNEL_BIN="$ROOT/target/$TARGET/release/neuron-boot.bin"
  local netdev
  if [[ "$name" == "A" ]]; then
    netdev="$NETDEV_A"
  else
    netdev="$NETDEV_B"
  fi
  local -a args=(
    -machine virt,aclint=on
    -cpu max
    -m 265M
    -smp "${SMP:-1}"
    -nographic
    -serial mon:stdio
    -icount 1,sleep=on
    -bios default
    -kernel "$KERNEL_BIN"
    ${netdev}
    -device "virtio-net-device,netdev=n0,mac=$mac"
  )

  if [[ "$OS2VM_PCAP" == "1" ]]; then
    local pcap
    local filter_id
    if [[ "$name" == "A" ]]; then
      pcap="$PCAP_A"
      filter_id="pcapA"
    else
      pcap="$PCAP_B"
      filter_id="pcapB"
    fi
    # Capture raw packets on netdev=n0 into a PCAP file (Wireshark-readable).
    # This is opt-in to keep default runs fast and deterministic.
    args+=(-object "filter-dump,id=${filter_id},netdev=n0,file=${pcap}")
  fi

  # QEMU runtime is bounded; build is done already.
  timeout --foreground "$RUN_TIMEOUT" stdbuf -oL qemu-system-riscv64 "${args[@]}" \
    | tee "$uart" \
    >"$hostlog" 2>&1 &

  echo $!
}

echo "[info] Building OS artifacts once..."
build_os_once

echo "[info] Launching Node A..."
PID_A=$(launch_qemu A "$A_MAC" "$UART_A" "$HOST_A")
echo "[info] Launching Node B..."
PID_B=$(launch_qemu B "$B_MAC" "$UART_B" "$HOST_B")

cleanup() {
  kill "$PID_A" "$PID_B" 2>/dev/null || true
}
trap cleanup EXIT

echo "[info] Waiting for cross-VM discovery markers..."
wait_marker "$UART_A" "dsoftbusd: discovery cross-vm up" || { echo "[error] Node A missing discovery marker"; exit 1; }
wait_marker "$UART_B" "dsoftbusd: discovery cross-vm up" || { echo "[error] Node B missing discovery marker"; exit 1; }

echo "[info] Waiting for cross-VM session markers..."
wait_marker "$UART_A" "dsoftbusd: cross-vm session ok" || { echo "[error] Node A missing session marker"; exit 1; }
wait_marker "$UART_B" "dsoftbusd: cross-vm session ok" || { echo "[error] Node B missing session marker"; exit 1; }

echo "[info] Waiting for remote proxy markers on Node A..."
wait_marker "$UART_A" "SELFTEST: remote resolve ok" || { echo "[error] Node A missing remote resolve marker"; exit 1; }
wait_marker "$UART_A" "SELFTEST: remote query ok" || { echo "[error] Node A missing remote query marker"; exit 1; }

echo "[info] All required markers observed. Stopping VMs."
cleanup
exit 0
