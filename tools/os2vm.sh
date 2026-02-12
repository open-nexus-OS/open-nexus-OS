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
LOG_DIR=${LOG_DIR:-.}
OS2VM_PCAP=${OS2VM_PCAP:-0}
AGENT_DEBUG_LOG=${AGENT_DEBUG_LOG:-/home/jenning/open-nexus-OS/.cursor/debug.log}
AGENT_RUN_ID=${AGENT_RUN_ID:-os2vm_$(date +%s)}

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

#region agent log
agent_debug_log() {
  local hypothesis_id=$1
  local location=$2
  local message=$3
  local data=$4
  printf '{"id":"log_%s_%s","runId":"%s","hypothesisId":"%s","location":"%s","message":"%s","data":%s,"timestamp":%s}\n' \
    "$(date +%s%N)" "$hypothesis_id" "$AGENT_RUN_ID" "$hypothesis_id" "$location" "$message" "$data" "$(date +%s%3N)" >>"$AGENT_DEBUG_LOG"
}
#endregion

parse_timeout_seconds() {
  local raw=$1
  local value
  local unit
  if [[ "$raw" =~ ^([0-9]+)([smh]?)$ ]]; then
    value=${BASH_REMATCH[1]}
    unit=${BASH_REMATCH[2]}
    case "$unit" in
      m) echo $(( value * 60 )) ;;
      h) echo $(( value * 3600 )) ;;
      *) echo "$value" ;;
    esac
    return 0
  fi
  echo 180
}

RUN_TIMEOUT_SECS=$(parse_timeout_seconds "$RUN_TIMEOUT")
MARKER_TIMEOUT=${MARKER_TIMEOUT:-$RUN_TIMEOUT_SECS}
if ! [[ "$MARKER_TIMEOUT" =~ ^[0-9]+$ ]]; then
  MARKER_TIMEOUT=$RUN_TIMEOUT_SECS
fi
if (( MARKER_TIMEOUT > RUN_TIMEOUT_SECS )); then
  MARKER_TIMEOUT=$RUN_TIMEOUT_SECS
fi

wait_marker() {
  local file=$1
  local pattern=$2
  local terminal_pattern=${3:-}
  local deadline=$(( $(date +%s) + MARKER_TIMEOUT ))
  while (( $(date +%s) < deadline )); do
    if [[ -s "$file" ]] && grep -q "$pattern" "$file" 2>/dev/null; then
      return 0
    fi
    if [[ -n "$terminal_pattern" ]] && [[ -s "$file" ]] && grep -q "$terminal_pattern" "$file" 2>/dev/null; then
      return 2
    fi
    sleep 1
  done
  return 1
}

wait_dual_markers() {
  local file_a=$1
  local pattern_a=$2
  local file_b=$3
  local pattern_b=$4
  local terminal_pattern=${5:-}
  local deadline=$(( $(date +%s) + MARKER_TIMEOUT ))

  while (( $(date +%s) < deadline )); do
    local a_ok=0
    local b_ok=0

    if [[ -s "$file_a" ]] && grep -q "$pattern_a" "$file_a" 2>/dev/null; then
      a_ok=1
    fi
    if [[ -s "$file_b" ]] && grep -q "$pattern_b" "$file_b" 2>/dev/null; then
      b_ok=1
    fi

    if (( a_ok == 1 && b_ok == 1 )); then
      return 0
    fi

    if [[ -n "$terminal_pattern" ]]; then
      if (( a_ok == 0 )) && [[ -s "$file_a" ]] && grep -q "$terminal_pattern" "$file_a" 2>/dev/null; then
        return 2
      fi
      if (( b_ok == 0 )) && [[ -s "$file_b" ]] && grep -q "$terminal_pattern" "$file_b" 2>/dev/null; then
        return 3
      fi
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

  # Keep this aligned with init-lite expectations (os_payload): include updated + logd + statefsd
  # so policy-gated MMIO grants and persistence bring-up don't fatal during cross-VM runs.
  local services="logd,updated,timed,keystored,rngd,policyd,samgrd,bundlemgrd,packagefsd,vfsd,execd,statefsd,netstackd,dsoftbusd,selftest-client"
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
  local blk_img="$LOG_DIR/blk-${name}.img"
  # Always recreate to avoid stale lock contention between runs.
  rm -f "$blk_img"
  # 64MiB is enough for StateFS + bring-up markers.
  qemu-img create -f raw "$blk_img" 64M >/dev/null

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
    -global virtio-mmio.force-legacy=off
    -bios default
    -kernel "$KERNEL_BIN"
    -drive "file=${blk_img},if=none,format=raw,id=drv0"
    -device "virtio-blk-device,drive=drv0"
    -device virtio-rng-device
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

# Best-effort cleanup: avoid stale socket backend contention between runs.
# (If a previous run crashed, QEMU may still hold the listen port.)
pkill -f "qemu-system-riscv64.*37021" 2>/dev/null || true

echo "[info] Launching Node A..."
PID_A=$(launch_qemu A "$A_MAC" "$UART_A" "$HOST_A")
echo "[info] Launching Node B..."
PID_B=$(launch_qemu B "$B_MAC" "$UART_B" "$HOST_B")
#region agent log
agent_debug_log "H2" "tools/os2vm.sh:launch" "qemu nodes launched" "{\"runTimeout\":\"$RUN_TIMEOUT\",\"markerTimeout\":$MARKER_TIMEOUT,\"pidA\":$PID_A,\"pidB\":$PID_B}"
#endregion

cleanup() {
  kill "$PID_A" "$PID_B" 2>/dev/null || true
}
trap cleanup EXIT

echo "[info] Waiting for cross-VM discovery markers..."
rc=0
wait_dual_markers "$UART_A" "dsoftbusd: discovery cross-vm up" "$UART_B" "dsoftbusd: discovery cross-vm up" "SELFTEST: end" || rc=$?
if [[ $rc -ne 0 ]]; then
  if [[ $rc -eq 2 ]]; then
    #region agent log
    agent_debug_log "H3" "tools/os2vm.sh:discovery_wait_A" "node A reached SELFTEST end before discovery marker" "{\"reason\":\"selftest_end_before_discovery\"}"
    #endregion
    echo "[error] Node A ended before discovery marker"
  elif [[ $rc -eq 3 ]]; then
    #region agent log
    agent_debug_log "H3" "tools/os2vm.sh:discovery_wait_B" "node B reached SELFTEST end before discovery marker" "{\"reason\":\"selftest_end_before_discovery\"}"
    #endregion
    echo "[error] Node B ended before discovery marker"
  else
    echo "[error] Missing discovery marker (timeout)"
  fi
  exit 1
fi

echo "[info] Waiting for cross-VM session markers..."
rc=0
wait_dual_markers "$UART_A" "dsoftbusd: cross-vm session ok" "$UART_B" "dsoftbusd: cross-vm session ok" "SELFTEST: end" || rc=$?
if [[ $rc -ne 0 ]]; then
  a_cross=$(grep -c "cross-vm" "$UART_A" || true)
  a_discovery=$(grep -c "discovery cross-vm up" "$UART_A" || true)
  a_accept=$(grep -c "cross-vm accept wait" "$UART_A" || true)
  a_session=$(grep -c "cross-vm session ok" "$UART_A" || true)
  a_dbg_ip_mismatch=$(grep -c "dbg:dsoftbusd: dial peer ip mismatch" "$UART_A" || true)
  a_dbg_ip_expected=$(grep -c "dbg:dsoftbusd: dial peer ip expected" "$UART_A" || true)
  a_dbg_would_block=$(grep -c "dbg:dsoftbusd: dial status would-block" "$UART_A" || true)
  a_dbg_io=$(grep -c "dbg:dsoftbusd: dial status io" "$UART_A" || true)
  a_dbg_other=$(grep -c "dbg:dsoftbusd: dial status other" "$UART_A" || true)
  a_dbg_dial_rpc_timeout=$(grep -c "dbg:dsoftbusd: dial rpc timeout" "$UART_A" || true)
  a_dbg_connect_slow=$(grep -c "dbg:dsoftbusd: connect rpc slow" "$UART_A" || true)
  a_dbg_connect_timeout=$(grep -c "dbg:dsoftbusd: connect rpc timeout" "$UART_A" || true)
  a_dbg_write_rpc_slow=$(grep -c "dbg:dsoftbusd: write rpc slow" "$UART_A" || true)
  a_dbg_write_rpc_timeout=$(grep -c "dbg:dsoftbusd: write rpc timeout" "$UART_A" || true)
  a_dbg_read_rpc_slow=$(grep -c "dbg:dsoftbusd: read rpc slow" "$UART_A" || true)
  a_dbg_read_rpc_timeout=$(grep -c "dbg:dsoftbusd: read rpc timeout" "$UART_A" || true)
  a_dbg_dial_attempt1_call=$(grep -c "dbg:dsoftbusd: dial attempt1 call" "$UART_A" || true)
  a_dbg_dial_attempt1_ret_ok=$(grep -c "dbg:dsoftbusd: dial attempt1 ret ok" "$UART_A" || true)
  a_dbg_dial_attempt1_ret_err=$(grep -c "dbg:dsoftbusd: dial attempt1 ret err" "$UART_A" || true)
  a_dbg_accept_attempt1_call=$(grep -c "dbg:dsoftbusd: accept attempt1 call" "$UART_A" || true)
  a_dbg_accept_attempt1_ret_ok=$(grep -c "dbg:dsoftbusd: accept attempt1 ret ok" "$UART_A" || true)
  a_dbg_accept_attempt1_ret_err=$(grep -c "dbg:dsoftbusd: accept attempt1 ret err" "$UART_A" || true)
  a_dbg_dial_fallback=$(grep -c "dbg:dsoftbusd: dial fallback no-discovery" "$UART_A" || true)
  a_dbg_identity_fallback=$(grep -c "dbg:dsoftbusd: identity fallback deterministic" "$UART_A" || true)
  a_dbg_dial_pending=$(grep -c "dbg:dsoftbusd: dial still pending" "$UART_A" || true)
  a_dbg_accept_pending=$(grep -c "dbg:dsoftbusd: accept still waiting" "$UART_A" || true)
  a_dbg_hs_sid_close=$(grep -c "dbg:dsoftbusd: hs sid close" "$UART_A" || true)
  a_dbg_local_ip_10=$(grep -c "dbg:dsoftbusd: local ip .10" "$UART_A" || true)
  a_dbg_local_ip_11=$(grep -c "dbg:dsoftbusd: local ip .11" "$UART_A" || true)
  a_dbg_local_ip_other=$(grep -c "dbg:dsoftbusd: local ip other" "$UART_A" || true)
  a_dbg_listen_mode_loopback=$(grep -c "dbg:netstackd: listen mode loopback" "$UART_A" || true)
  a_dbg_listen_mode_tcp=$(grep -c "dbg:netstackd: listen mode tcp" "$UART_A" || true)
  b_cross=$(grep -c "cross-vm" "$UART_B" || true)
  b_discovery=$(grep -c "discovery cross-vm up" "$UART_B" || true)
  b_accept=$(grep -c "cross-vm accept wait" "$UART_B" || true)
  b_session=$(grep -c "cross-vm session ok" "$UART_B" || true)
  b_dbg_ip_mismatch=$(grep -c "dbg:dsoftbusd: dial peer ip mismatch" "$UART_B" || true)
  b_dbg_ip_expected=$(grep -c "dbg:dsoftbusd: dial peer ip expected" "$UART_B" || true)
  b_dbg_would_block=$(grep -c "dbg:dsoftbusd: dial status would-block" "$UART_B" || true)
  b_dbg_io=$(grep -c "dbg:dsoftbusd: dial status io" "$UART_B" || true)
  b_dbg_other=$(grep -c "dbg:dsoftbusd: dial status other" "$UART_B" || true)
  b_dbg_dial_rpc_timeout=$(grep -c "dbg:dsoftbusd: dial rpc timeout" "$UART_B" || true)
  b_dbg_connect_slow=$(grep -c "dbg:dsoftbusd: connect rpc slow" "$UART_B" || true)
  b_dbg_connect_timeout=$(grep -c "dbg:dsoftbusd: connect rpc timeout" "$UART_B" || true)
  b_dbg_write_rpc_slow=$(grep -c "dbg:dsoftbusd: write rpc slow" "$UART_B" || true)
  b_dbg_write_rpc_timeout=$(grep -c "dbg:dsoftbusd: write rpc timeout" "$UART_B" || true)
  b_dbg_read_rpc_slow=$(grep -c "dbg:dsoftbusd: read rpc slow" "$UART_B" || true)
  b_dbg_read_rpc_timeout=$(grep -c "dbg:dsoftbusd: read rpc timeout" "$UART_B" || true)
  b_dbg_dial_attempt1_call=$(grep -c "dbg:dsoftbusd: dial attempt1 call" "$UART_B" || true)
  b_dbg_dial_attempt1_ret_ok=$(grep -c "dbg:dsoftbusd: dial attempt1 ret ok" "$UART_B" || true)
  b_dbg_dial_attempt1_ret_err=$(grep -c "dbg:dsoftbusd: dial attempt1 ret err" "$UART_B" || true)
  b_dbg_accept_attempt1_call=$(grep -c "dbg:dsoftbusd: accept attempt1 call" "$UART_B" || true)
  b_dbg_accept_attempt1_ret_ok=$(grep -c "dbg:dsoftbusd: accept attempt1 ret ok" "$UART_B" || true)
  b_dbg_accept_attempt1_ret_err=$(grep -c "dbg:dsoftbusd: accept attempt1 ret err" "$UART_B" || true)
  b_dbg_dial_fallback=$(grep -c "dbg:dsoftbusd: dial fallback no-discovery" "$UART_B" || true)
  b_dbg_identity_fallback=$(grep -c "dbg:dsoftbusd: identity fallback deterministic" "$UART_B" || true)
  b_dbg_dial_pending=$(grep -c "dbg:dsoftbusd: dial still pending" "$UART_B" || true)
  b_dbg_accept_pending=$(grep -c "dbg:dsoftbusd: accept still waiting" "$UART_B" || true)
  b_dbg_hs_sid_close=$(grep -c "dbg:dsoftbusd: hs sid close" "$UART_B" || true)
  b_dbg_local_ip_10=$(grep -c "dbg:dsoftbusd: local ip .10" "$UART_B" || true)
  b_dbg_local_ip_11=$(grep -c "dbg:dsoftbusd: local ip .11" "$UART_B" || true)
  b_dbg_local_ip_other=$(grep -c "dbg:dsoftbusd: local ip other" "$UART_B" || true)
  b_dbg_listen_mode_loopback=$(grep -c "dbg:netstackd: listen mode loopback" "$UART_B" || true)
  b_dbg_listen_mode_tcp=$(grep -c "dbg:netstackd: listen mode tcp" "$UART_B" || true)
  #region agent log
  if [[ $rc -eq 2 ]]; then
    agent_debug_log "H3" "tools/os2vm.sh:session_wait_A" "node A reached SELFTEST end before cross-vm session marker" "{\"nodeA\":{\"crossCount\":$a_cross,\"discoveryCount\":$a_discovery,\"acceptWaitCount\":$a_accept,\"sessionCount\":$a_session,\"dbgLocalIp10\":$a_dbg_local_ip_10,\"dbgLocalIp11\":$a_dbg_local_ip_11,\"dbgLocalIpOther\":$a_dbg_local_ip_other,\"dbgListenModeLoopback\":$a_dbg_listen_mode_loopback,\"dbgListenModeTcp\":$a_dbg_listen_mode_tcp,\"dbgIpExpected\":$a_dbg_ip_expected,\"dbgIpMismatch\":$a_dbg_ip_mismatch,\"dbgDialWouldBlock\":$a_dbg_would_block,\"dbgDialIo\":$a_dbg_io,\"dbgDialOther\":$a_dbg_other,\"dbgDialRpcTimeout\":$a_dbg_dial_rpc_timeout,\"dbgConnectSlow\":$a_dbg_connect_slow,\"dbgConnectTimeout\":$a_dbg_connect_timeout,\"dbgWriteRpcSlow\":$a_dbg_write_rpc_slow,\"dbgWriteRpcTimeout\":$a_dbg_write_rpc_timeout,\"dbgReadRpcSlow\":$a_dbg_read_rpc_slow,\"dbgReadRpcTimeout\":$a_dbg_read_rpc_timeout,\"dbgDialAttempt1Call\":$a_dbg_dial_attempt1_call,\"dbgDialAttempt1RetOk\":$a_dbg_dial_attempt1_ret_ok,\"dbgDialAttempt1RetErr\":$a_dbg_dial_attempt1_ret_err,\"dbgAcceptAttempt1Call\":$a_dbg_accept_attempt1_call,\"dbgAcceptAttempt1RetOk\":$a_dbg_accept_attempt1_ret_ok,\"dbgAcceptAttempt1RetErr\":$a_dbg_accept_attempt1_ret_err,\"dbgDialFallback\":$a_dbg_dial_fallback,\"dbgIdentityFallback\":$a_dbg_identity_fallback,\"dbgDialPending\":$a_dbg_dial_pending,\"dbgAcceptPending\":$a_dbg_accept_pending,\"dbgHsSidClose\":$a_dbg_hs_sid_close},\"nodeB\":{\"crossCount\":$b_cross,\"discoveryCount\":$b_discovery,\"acceptWaitCount\":$b_accept,\"sessionCount\":$b_session,\"dbgLocalIp10\":$b_dbg_local_ip_10,\"dbgLocalIp11\":$b_dbg_local_ip_11,\"dbgLocalIpOther\":$b_dbg_local_ip_other,\"dbgListenModeLoopback\":$b_dbg_listen_mode_loopback,\"dbgListenModeTcp\":$b_dbg_listen_mode_tcp,\"dbgIpExpected\":$b_dbg_ip_expected,\"dbgIpMismatch\":$b_dbg_ip_mismatch,\"dbgDialWouldBlock\":$b_dbg_would_block,\"dbgDialIo\":$b_dbg_io,\"dbgDialOther\":$b_dbg_other,\"dbgDialRpcTimeout\":$b_dbg_dial_rpc_timeout,\"dbgConnectSlow\":$b_dbg_connect_slow,\"dbgConnectTimeout\":$b_dbg_connect_timeout,\"dbgWriteRpcSlow\":$b_dbg_write_rpc_slow,\"dbgWriteRpcTimeout\":$b_dbg_write_rpc_timeout,\"dbgReadRpcSlow\":$b_dbg_read_rpc_slow,\"dbgReadRpcTimeout\":$b_dbg_read_rpc_timeout,\"dbgDialAttempt1Call\":$b_dbg_dial_attempt1_call,\"dbgDialAttempt1RetOk\":$b_dbg_dial_attempt1_ret_ok,\"dbgDialAttempt1RetErr\":$b_dbg_dial_attempt1_ret_err,\"dbgAcceptAttempt1Call\":$b_dbg_accept_attempt1_call,\"dbgAcceptAttempt1RetOk\":$b_dbg_accept_attempt1_ret_ok,\"dbgAcceptAttempt1RetErr\":$b_dbg_accept_attempt1_ret_err,\"dbgDialFallback\":$b_dbg_dial_fallback,\"dbgIdentityFallback\":$b_dbg_identity_fallback,\"dbgDialPending\":$b_dbg_dial_pending,\"dbgAcceptPending\":$b_dbg_accept_pending,\"dbgHsSidClose\":$b_dbg_hs_sid_close},\"reason\":\"selftest_end_before_session\"}"
  elif [[ $rc -eq 3 ]]; then
    agent_debug_log "H3" "tools/os2vm.sh:session_wait_B" "node B reached SELFTEST end before cross-vm session marker" "{\"nodeA\":{\"crossCount\":$a_cross,\"discoveryCount\":$a_discovery,\"acceptWaitCount\":$a_accept,\"sessionCount\":$a_session,\"dbgLocalIp10\":$a_dbg_local_ip_10,\"dbgLocalIp11\":$a_dbg_local_ip_11,\"dbgLocalIpOther\":$a_dbg_local_ip_other,\"dbgListenModeLoopback\":$a_dbg_listen_mode_loopback,\"dbgListenModeTcp\":$a_dbg_listen_mode_tcp,\"dbgIpExpected\":$a_dbg_ip_expected,\"dbgIpMismatch\":$a_dbg_ip_mismatch,\"dbgDialWouldBlock\":$a_dbg_would_block,\"dbgDialIo\":$a_dbg_io,\"dbgDialOther\":$a_dbg_other,\"dbgDialRpcTimeout\":$a_dbg_dial_rpc_timeout,\"dbgConnectSlow\":$a_dbg_connect_slow,\"dbgConnectTimeout\":$a_dbg_connect_timeout,\"dbgWriteRpcSlow\":$a_dbg_write_rpc_slow,\"dbgWriteRpcTimeout\":$a_dbg_write_rpc_timeout,\"dbgReadRpcSlow\":$a_dbg_read_rpc_slow,\"dbgReadRpcTimeout\":$a_dbg_read_rpc_timeout,\"dbgDialAttempt1Call\":$a_dbg_dial_attempt1_call,\"dbgDialAttempt1RetOk\":$a_dbg_dial_attempt1_ret_ok,\"dbgDialAttempt1RetErr\":$a_dbg_dial_attempt1_ret_err,\"dbgAcceptAttempt1Call\":$a_dbg_accept_attempt1_call,\"dbgAcceptAttempt1RetOk\":$a_dbg_accept_attempt1_ret_ok,\"dbgAcceptAttempt1RetErr\":$a_dbg_accept_attempt1_ret_err,\"dbgDialFallback\":$a_dbg_dial_fallback,\"dbgIdentityFallback\":$a_dbg_identity_fallback,\"dbgDialPending\":$a_dbg_dial_pending,\"dbgAcceptPending\":$a_dbg_accept_pending,\"dbgHsSidClose\":$a_dbg_hs_sid_close},\"nodeB\":{\"crossCount\":$b_cross,\"discoveryCount\":$b_discovery,\"acceptWaitCount\":$b_accept,\"sessionCount\":$b_session,\"dbgLocalIp10\":$b_dbg_local_ip_10,\"dbgLocalIp11\":$b_dbg_local_ip_11,\"dbgLocalIpOther\":$b_dbg_local_ip_other,\"dbgListenModeLoopback\":$b_dbg_listen_mode_loopback,\"dbgListenModeTcp\":$b_dbg_listen_mode_tcp,\"dbgIpExpected\":$b_dbg_ip_expected,\"dbgIpMismatch\":$b_dbg_ip_mismatch,\"dbgDialWouldBlock\":$b_dbg_would_block,\"dbgDialIo\":$b_dbg_io,\"dbgDialOther\":$b_dbg_other,\"dbgDialRpcTimeout\":$b_dbg_dial_rpc_timeout,\"dbgConnectSlow\":$b_dbg_connect_slow,\"dbgConnectTimeout\":$b_dbg_connect_timeout,\"dbgWriteRpcSlow\":$b_dbg_write_rpc_slow,\"dbgWriteRpcTimeout\":$b_dbg_write_rpc_timeout,\"dbgReadRpcSlow\":$b_dbg_read_rpc_slow,\"dbgReadRpcTimeout\":$b_dbg_read_rpc_timeout,\"dbgDialAttempt1Call\":$b_dbg_dial_attempt1_call,\"dbgDialAttempt1RetOk\":$b_dbg_dial_attempt1_ret_ok,\"dbgDialAttempt1RetErr\":$b_dbg_dial_attempt1_ret_err,\"dbgAcceptAttempt1Call\":$b_dbg_accept_attempt1_call,\"dbgAcceptAttempt1RetOk\":$b_dbg_accept_attempt1_ret_ok,\"dbgAcceptAttempt1RetErr\":$b_dbg_accept_attempt1_ret_err,\"dbgDialFallback\":$b_dbg_dial_fallback,\"dbgIdentityFallback\":$b_dbg_identity_fallback,\"dbgDialPending\":$b_dbg_dial_pending,\"dbgAcceptPending\":$b_dbg_accept_pending,\"dbgHsSidClose\":$b_dbg_hs_sid_close},\"reason\":\"selftest_end_before_session\"}"
  else
    agent_debug_log "H1" "tools/os2vm.sh:session_wait_timeout" "cross-vm session marker wait timed out" "{\"nodeA\":{\"crossCount\":$a_cross,\"discoveryCount\":$a_discovery,\"acceptWaitCount\":$a_accept,\"sessionCount\":$a_session,\"dbgLocalIp10\":$a_dbg_local_ip_10,\"dbgLocalIp11\":$a_dbg_local_ip_11,\"dbgLocalIpOther\":$a_dbg_local_ip_other,\"dbgListenModeLoopback\":$a_dbg_listen_mode_loopback,\"dbgListenModeTcp\":$a_dbg_listen_mode_tcp,\"dbgIpExpected\":$a_dbg_ip_expected,\"dbgIpMismatch\":$a_dbg_ip_mismatch,\"dbgDialWouldBlock\":$a_dbg_would_block,\"dbgDialIo\":$a_dbg_io,\"dbgDialOther\":$a_dbg_other,\"dbgDialRpcTimeout\":$a_dbg_dial_rpc_timeout,\"dbgConnectSlow\":$a_dbg_connect_slow,\"dbgConnectTimeout\":$a_dbg_connect_timeout,\"dbgWriteRpcSlow\":$a_dbg_write_rpc_slow,\"dbgWriteRpcTimeout\":$a_dbg_write_rpc_timeout,\"dbgReadRpcSlow\":$a_dbg_read_rpc_slow,\"dbgReadRpcTimeout\":$a_dbg_read_rpc_timeout,\"dbgDialAttempt1Call\":$a_dbg_dial_attempt1_call,\"dbgDialAttempt1RetOk\":$a_dbg_dial_attempt1_ret_ok,\"dbgDialAttempt1RetErr\":$a_dbg_dial_attempt1_ret_err,\"dbgAcceptAttempt1Call\":$a_dbg_accept_attempt1_call,\"dbgAcceptAttempt1RetOk\":$a_dbg_accept_attempt1_ret_ok,\"dbgAcceptAttempt1RetErr\":$a_dbg_accept_attempt1_ret_err,\"dbgDialFallback\":$a_dbg_dial_fallback,\"dbgIdentityFallback\":$a_dbg_identity_fallback,\"dbgDialPending\":$a_dbg_dial_pending,\"dbgAcceptPending\":$a_dbg_accept_pending,\"dbgHsSidClose\":$a_dbg_hs_sid_close},\"nodeB\":{\"crossCount\":$b_cross,\"discoveryCount\":$b_discovery,\"acceptWaitCount\":$b_accept,\"sessionCount\":$b_session,\"dbgLocalIp10\":$b_dbg_local_ip_10,\"dbgLocalIp11\":$b_dbg_local_ip_11,\"dbgLocalIpOther\":$b_dbg_local_ip_other,\"dbgListenModeLoopback\":$b_dbg_listen_mode_loopback,\"dbgListenModeTcp\":$b_dbg_listen_mode_tcp,\"dbgIpExpected\":$b_dbg_ip_expected,\"dbgIpMismatch\":$b_dbg_ip_mismatch,\"dbgDialWouldBlock\":$b_dbg_would_block,\"dbgDialIo\":$b_dbg_io,\"dbgDialOther\":$b_dbg_other,\"dbgDialRpcTimeout\":$b_dbg_dial_rpc_timeout,\"dbgConnectSlow\":$b_dbg_connect_slow,\"dbgConnectTimeout\":$b_dbg_connect_timeout,\"dbgWriteRpcSlow\":$b_dbg_write_rpc_slow,\"dbgWriteRpcTimeout\":$b_dbg_write_rpc_timeout,\"dbgReadRpcSlow\":$b_dbg_read_rpc_slow,\"dbgReadRpcTimeout\":$b_dbg_read_rpc_timeout,\"dbgDialAttempt1Call\":$b_dbg_dial_attempt1_call,\"dbgDialAttempt1RetOk\":$b_dbg_dial_attempt1_ret_ok,\"dbgDialAttempt1RetErr\":$b_dbg_dial_attempt1_ret_err,\"dbgAcceptAttempt1Call\":$b_dbg_accept_attempt1_call,\"dbgAcceptAttempt1RetOk\":$b_dbg_accept_attempt1_ret_ok,\"dbgAcceptAttempt1RetErr\":$b_dbg_accept_attempt1_ret_err,\"dbgDialFallback\":$b_dbg_dial_fallback,\"dbgIdentityFallback\":$b_dbg_identity_fallback,\"dbgDialPending\":$b_dbg_dial_pending,\"dbgAcceptPending\":$b_dbg_accept_pending,\"dbgHsSidClose\":$b_dbg_hs_sid_close},\"reason\":\"timeout\"}"
  fi
  #endregion
  if [[ $rc -eq 2 ]]; then
    echo "[error] Node A ended before session marker"
  elif [[ $rc -eq 3 ]]; then
    echo "[error] Node B ended before session marker"
  else
    echo "[error] Missing session marker (timeout)"
  fi
  exit 1
fi
#region agent log
agent_debug_log "H4" "tools/os2vm.sh:session_wait_done" "cross-vm session markers reached on both nodes" "{\"status\":\"ok\"}"
#endregion

echo "[info] Waiting for remote proxy markers on Node A..."
wait_marker "$UART_A" "SELFTEST: remote resolve ok" "SELFTEST: end" || { echo "[error] Node A missing remote resolve marker"; exit 1; }
wait_marker "$UART_A" "SELFTEST: remote query ok" "SELFTEST: end" || { echo "[error] Node A missing remote query marker"; exit 1; }
#region agent log
agent_debug_log "H4" "tools/os2vm.sh:remote_markers_done" "remote proxy markers reached on node A" "{\"status\":\"ok\"}"
#endregion

echo "[info] All required markers observed. Stopping VMs."
cleanup
exit 0
