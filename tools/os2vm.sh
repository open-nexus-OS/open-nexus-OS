#!/usr/bin/env bash
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# Opt-in 2-VM harness for cross-VM DSoftBus proof (TASK-0005 / RFC-0010 / TASK-0016).
#
# Debug-first design:
# - Deterministic modern virtio-mmio (`force-legacy=off`) and bounded phase budgets.
# - Phase-gated execution for fast reruns (`RUN_PHASE`, `OS2VM_SKIP_BUILD`).
# - Structured summaries (`os2vm-summary-<run>.json/.txt`) with first-failure localization.
# - Packet capture modes (`off|on|auto`) with packet-to-marker correlation hints.
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
BUILD_TARGET_DIR=${OS2VM_TARGET_DIR:-"$ROOT/target-os2vm"}
RUSTFLAGS_OS=${RUSTFLAGS_OS:---check-cfg=cfg(nexus_env,values(\"host\",\"os\")) --cfg nexus_env=\"os\"}
RUN_TIMEOUT=${RUN_TIMEOUT:-180s}
AGENT_RUN_ID=${AGENT_RUN_ID:-os2vm_$(date +%s)}
AGENT_DEBUG_LOG=${AGENT_DEBUG_LOG:-/home/jenning/open-nexus-OS/.cursor/debug.log}
DEBUG_SESSION_ID=${DEBUG_SESSION_ID:-17b977}
DEBUG_LOG_PATH=${DEBUG_LOG_PATH:-/home/jenning/open-nexus-OS/.cursor/debug-17b977.log}

set_default_if_unset() {
  local name=$1
  local value=$2
  if [[ -z "${!name+x}" ]]; then
    printf -v "$name" '%s' "$value"
  fi
}

OS2VM_PROFILE=${OS2VM_PROFILE:-debug}
case "$OS2VM_PROFILE" in
  debug)
    set_default_if_unset RUN_PHASE "end"
    set_default_if_unset OS2VM_SKIP_BUILD "0"
    set_default_if_unset OS2VM_EXIT_CODE_MODE "typed"
    set_default_if_unset OS2VM_SUMMARY_STDOUT "1"
    set_default_if_unset OS2VM_PCAP "on"
    ;;
  ci)
    set_default_if_unset RUN_PHASE "end"
    set_default_if_unset OS2VM_SKIP_BUILD "0"
    set_default_if_unset OS2VM_EXIT_CODE_MODE "typed"
    set_default_if_unset OS2VM_SUMMARY_STDOUT "1"
    set_default_if_unset OS2VM_PCAP "auto"
    ;;
  fast-local)
    set_default_if_unset RUN_PHASE "session"
    set_default_if_unset OS2VM_SKIP_BUILD "1"
    set_default_if_unset OS2VM_EXIT_CODE_MODE "typed"
    set_default_if_unset OS2VM_SUMMARY_STDOUT "1"
    set_default_if_unset OS2VM_PCAP "off"
    ;;
  *)
    echo "[warn] Unknown OS2VM_PROFILE='$OS2VM_PROFILE'; using debug defaults." >&2
    set_default_if_unset RUN_PHASE "end"
    set_default_if_unset OS2VM_SKIP_BUILD "0"
    set_default_if_unset OS2VM_EXIT_CODE_MODE "typed"
    set_default_if_unset OS2VM_SUMMARY_STDOUT "1"
    set_default_if_unset OS2VM_PCAP "on"
    ;;
esac

OS2VM_ARTIFACT_ROOT=${OS2VM_ARTIFACT_ROOT:-"$ROOT/artifacts/os2vm"}
OS2VM_RUNS_DIR=${OS2VM_RUNS_DIR:-"$OS2VM_ARTIFACT_ROOT/runs"}
LOG_DIR=${LOG_DIR:-"$OS2VM_RUNS_DIR/$AGENT_RUN_ID"}
OS2VM_PCAP_BASENAME=${OS2VM_PCAP_BASENAME:-packets}
OS2VM_SUMMARY_JSON=${OS2VM_SUMMARY_JSON:-"$LOG_DIR/summary.json"}
OS2VM_SUMMARY_TXT=${OS2VM_SUMMARY_TXT:-"$LOG_DIR/summary.txt"}
OS2VM_RELEASE_BUNDLE_JSON=${OS2VM_RELEASE_BUNDLE_JSON:-"$LOG_DIR/release-evidence.json"}
OS2VM_PID_FILE=${OS2VM_PID_FILE:-"$LOG_DIR/pids.env"}
OS2VM_RESULT_FILE=${OS2VM_RESULT_FILE:-"$LOG_DIR/result.txt"}
OS2VM_RETENTION_ENABLE=${OS2VM_RETENTION_ENABLE:-1}
OS2VM_RETENTION_KEEP_SUCCESS=${OS2VM_RETENTION_KEEP_SUCCESS:-4}
OS2VM_RETENTION_KEEP_FAILURE=${OS2VM_RETENTION_KEEP_FAILURE:-12}
OS2VM_RETENTION_MAX_TOTAL_MB=${OS2VM_RETENTION_MAX_TOTAL_MB:-3072}
OS2VM_RETENTION_MAX_AGE_DAYS=${OS2VM_RETENTION_MAX_AGE_DAYS:-14}
OS2VM_SANDBOX_CACHE_GC=${OS2VM_SANDBOX_CACHE_GC:-auto}
OS2VM_SANDBOX_CACHE_DIR=${OS2VM_SANDBOX_CACHE_DIR:-/tmp/cursor-sandbox-cache}
OS2VM_SANDBOX_CACHE_MAX_MB=${OS2VM_SANDBOX_CACHE_MAX_MB:-1024}
OS2VM_SANDBOX_CACHE_TARGET_FREE_MB=${OS2VM_SANDBOX_CACHE_TARGET_FREE_MB:-1024}
OS2VM_SANDBOX_CACHE_MIN_AGE_SECS=${OS2VM_SANDBOX_CACHE_MIN_AGE_SECS:-1800}

A_MAC=${A_MAC:-52:54:00:12:34:0a}
B_MAC=${B_MAC:-52:54:00:12:34:0b}

# QEMU socket backend:
# - Default: deterministic multicast hub on localhost (more robust under fast relaunches).
# - Optional: point-to-point listen/connect by overriding NETDEV_A / NETDEV_B.
DEFAULT_MCAST_NETDEV="-netdev socket,id=n0,mcast=230.0.0.1:37021,localaddr=127.0.0.1"
NETDEV_A=${NETDEV_A:-$DEFAULT_MCAST_NETDEV}
NETDEV_B=${NETDEV_B:-$DEFAULT_MCAST_NETDEV}

UART_A="$LOG_DIR/uart-A.txt"
UART_B="$LOG_DIR/uart-B.txt"
HOST_A="$LOG_DIR/host-A.txt"
HOST_B="$LOG_DIR/host-B.txt"
PCAP_A="$LOG_DIR/${OS2VM_PCAP_BASENAME}-A.pcap"
PCAP_B="$LOG_DIR/${OS2VM_PCAP_BASENAME}-B.pcap"

mkdir -p "$OS2VM_RUNS_DIR" "$LOG_DIR"
if [[ -z "${TMPDIR:-}" ]]; then
  export TMPDIR="$LOG_DIR/tmp"
fi
mkdir -p "$TMPDIR"
mkdir -p "$(dirname "$DEBUG_LOG_PATH")" "$(dirname "$AGENT_DEBUG_LOG")"
: >"$UART_A"
: >"$UART_B"
: >"$HOST_A"
: >"$HOST_B"
if [[ "$OS2VM_PCAP" == "1" || "$OS2VM_PCAP" == "on" || "$OS2VM_PCAP" == "auto" ]]; then
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

#region agent session log
agent_session_log() {
  local hypothesis_id=$1
  local location=$2
  local message=$3
  local data_json=$4
  local ts
  ts=$(date +%s%3N 2>/dev/null || date +%s000)
  printf '{"sessionId":"%s","id":"log_%s_%s","timestamp":%s,"location":"%s","message":"%s","data":%s,"runId":"%s","hypothesisId":"%s"}\n' \
    "$DEBUG_SESSION_ID" "$ts" "$hypothesis_id" "$ts" "$location" "$message" "$data_json" "$AGENT_RUN_ID" "$hypothesis_id" >>"$DEBUG_LOG_PATH"
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

now_ms() {
  date +%s%3N 2>/dev/null || echo "$(( $(date +%s) * 1000 ))"
}

json_escape() {
  local s=${1:-}
  s=${s//\\/\\\\}
  s=${s//\"/\\\"}
  s=${s//$'\n'/\\n}
  s=${s//$'\r'/\\r}
  printf '%s' "$s"
}

count_marker() {
  local file=$1
  local pattern=$2
  local c
  c=$(grep -c "$pattern" "$file" 2>/dev/null || true)
  if [[ -z "$c" ]]; then
    c=0
  fi
  echo "$c"
}

file_mtime_or_zero() {
  local p=$1
  if [[ -f "$p" ]]; then
    stat -c %Y "$p" 2>/dev/null || echo 0
  else
    echo 0
  fi
}

marker_line() {
  local file=$1
  local pattern=$2
  grep -n "$pattern" "$file" 2>/dev/null | head -n1 | cut -d: -f1 || true
}

RUN_TIMEOUT_SECS=$(parse_timeout_seconds "$RUN_TIMEOUT")
if (( RUN_TIMEOUT_SECS <= 0 )); then
  RUN_TIMEOUT_SECS=180
fi

resolve_timeout_secs() {
  local raw=$1
  local fallback=$2
  local s
  if [[ -z "$raw" ]]; then
    echo "$fallback"
    return 0
  fi
  s=$(parse_timeout_seconds "$raw")
  if ! [[ "$s" =~ ^[0-9]+$ ]]; then
    echo "$fallback"
    return 0
  fi
  if (( s <= 0 )); then
    s=$fallback
  fi
  if (( s > RUN_TIMEOUT_SECS )); then
    s=$RUN_TIMEOUT_SECS
  fi
  echo "$s"
}

resolve_timeout_secs_uncapped() {
  local raw=$1
  local fallback=$2
  local s
  if [[ -z "$raw" ]]; then
    echo "$fallback"
    return 0
  fi
  s=$(parse_timeout_seconds "$raw")
  if ! [[ "$s" =~ ^[0-9]+$ ]]; then
    echo "$fallback"
    return 0
  fi
  if (( s <= 0 )); then
    s=$fallback
  fi
  echo "$s"
}

df_available_kb() {
  local path=$1
  if ! command -v df >/dev/null 2>&1; then
    echo -1
    return 0
  fi
  df -Pk "$path" 2>/dev/null | awk 'NR==2 {print $4}' || echo -1
}

is_pid_alive() {
  local pid=${1:-}
  if [[ -z "$pid" ]]; then
    return 1
  fi
  kill -0 "$pid" 2>/dev/null
}

sanitize_run_tag() {
  local raw=$1
  local cleaned
  cleaned=$(echo "$raw" | tr -cd '[:alnum:]_-' )
  if [[ -z "$cleaned" ]]; then
    cleaned="os2vmrun"
  fi
  echo "$cleaned"
}

RUN_TAG=$(sanitize_run_tag "$AGENT_RUN_ID")

qemu_name_for_node() {
  local node=$1
  echo "os2vm-${RUN_TAG}-${node}"
}

terminate_pid() {
  local pid=${1:-}
  local grace_secs=${2:-5}
  if [[ -z "$pid" ]]; then
    return 0
  fi
  if ! is_pid_alive "$pid"; then
    return 0
  fi
  kill "$pid" 2>/dev/null || true
  local deadline=$(( $(date +%s) + grace_secs ))
  while is_pid_alive "$pid"; do
    if (( $(date +%s) >= deadline )); then
      break
    fi
    sleep 1
  done
  if is_pid_alive "$pid"; then
    kill -9 "$pid" 2>/dev/null || true
    sleep 1
  fi
}

terminate_named_qemu() {
  local qemu_name=$1
  local pids=""
  pids=$(pgrep -f "qemu-system-riscv64.*-name[[:space:]]${qemu_name}" 2>/dev/null || true)
  if [[ -z "$pids" ]]; then
    return 0
  fi
  local pid
  while IFS= read -r pid; do
    [[ -z "$pid" ]] && continue
    terminate_pid "$pid" 4
  done <<<"$pids"
}

persist_pid_snapshot() {
  cat >"$OS2VM_PID_FILE" <<EOF
RUN_ID=$AGENT_RUN_ID
PID_A=$PID_A
PID_B=$PID_B
EOF
}

cleanup_stale_pid_records() {
  local pid_file
  local run_id
  local pid_a
  local pid_b
  local cmd
  shopt -s nullglob
  for pid_file in "$OS2VM_RUNS_DIR"/*/pids.env; do
    [[ "$pid_file" == "$OS2VM_PID_FILE" ]] && continue
    run_id=""
    pid_a=""
    pid_b=""
    while IFS='=' read -r key value; do
      case "$key" in
        RUN_ID) run_id="$value" ;;
        PID_A) pid_a="$value" ;;
        PID_B) pid_b="$value" ;;
      esac
    done <"$pid_file"
    if [[ "$run_id" == "$AGENT_RUN_ID" ]]; then
      continue
    fi
    for pid in "$pid_a" "$pid_b"; do
      [[ -z "$pid" ]] && continue
      if ! is_pid_alive "$pid"; then
        continue
      fi
      cmd=$(ps -o args= -p "$pid" 2>/dev/null || true)
      if [[ "$cmd" == *"qemu-system-riscv64"* && "$cmd" == *"os2vm-"* ]]; then
        terminate_pid "$pid" 4
      fi
    done
  done
  shopt -u nullglob
}

sandbox_cache_usage_kb() {
  local cache_root=$1
  if [[ ! -d "$cache_root" ]]; then
    echo 0
    return 0
  fi
  du -sk "$cache_root" 2>/dev/null | awk '{print $1}' || echo 0
}

gc_sandbox_cache() {
  local mode=${OS2VM_SANDBOX_CACHE_GC:-auto}
  local cache_root=${OS2VM_SANDBOX_CACHE_DIR:-/tmp/cursor-sandbox-cache}
  case "$mode" in
    off|OFF|0|false|FALSE)
      return 0
      ;;
    on|ON|1|true|TRUE)
      mode="on"
      ;;
    auto|AUTO|*)
      mode="auto"
      ;;
  esac
  if [[ ! -d "$cache_root" ]]; then
    return 0
  fi

  local max_cache_kb=$(( OS2VM_SANDBOX_CACHE_MAX_MB * 1024 ))
  local target_free_kb=$(( OS2VM_SANDBOX_CACHE_TARGET_FREE_MB * 1024 ))
  local min_age_secs=${OS2VM_SANDBOX_CACHE_MIN_AGE_SECS:-1800}
  local cache_kb
  local tmp_free_kb
  cache_kb=$(sandbox_cache_usage_kb "$cache_root")
  tmp_free_kb=$(df_available_kb /tmp)

  local need_gc=0
  if [[ "$mode" == "on" ]]; then
    need_gc=1
  elif (( cache_kb > max_cache_kb )); then
    need_gc=1
  elif [[ "$tmp_free_kb" =~ ^[0-9]+$ ]] && (( tmp_free_kb >= 0 && tmp_free_kb < target_free_kb )); then
    need_gc=1
  fi
  if (( need_gc == 0 )); then
    return 0
  fi

  local now
  now=$(date +%s)
  local -a entries=()
  shopt -s nullglob
  entries=( "$cache_root"/* )
  shopt -u nullglob
  if (( ${#entries[@]} == 0 )); then
    return 0
  fi
  IFS=$'\n' entries=($(ls -1dt "${entries[@]}" 2>/dev/null || true))
  unset IFS

  local idx
  local entry
  local mtime
  local age
  local removed=0
  local before_kb=$cache_kb
  for (( idx=${#entries[@]} - 1; idx>=0; idx-- )); do
    entry=${entries[$idx]}
    [[ -e "$entry" ]] || continue
    mtime=$(stat -c %Y "$entry" 2>/dev/null || echo "$now")
    age=$(( now - mtime ))
    if [[ "$mode" == "auto" ]] && (( age < min_age_secs )); then
      continue
    fi
    rm -rf "$entry" 2>/dev/null || true
    removed=$(( removed + 1 ))
    cache_kb=$(sandbox_cache_usage_kb "$cache_root")
    tmp_free_kb=$(df_available_kb /tmp)
    if (( cache_kb <= max_cache_kb )); then
      if ! [[ "$tmp_free_kb" =~ ^[0-9]+$ ]] || (( tmp_free_kb >= target_free_kb )); then
        break
      fi
    fi
  done

  if (( removed > 0 )); then
    echo "[info] sandbox cache gc: removed=${removed} before_kb=${before_kb} after_kb=${cache_kb} tmp_free_kb=${tmp_free_kb}"
    # #region agent log
    agent_session_log "GC" "tools/os2vm.sh:gc_sandbox_cache" "sandbox cache gc run" "{\"mode\":\"$mode\",\"cacheDir\":\"$cache_root\",\"removed\":$removed,\"beforeKb\":$before_kb,\"afterKb\":$cache_kb,\"tmpFreeKb\":$tmp_free_kb,\"maxCacheKb\":$max_cache_kb,\"targetFreeKb\":$target_free_kb,\"minAgeSecs\":$min_age_secs}"
    # #endregion
  fi
}

MARKER_TIMEOUT_LEGACY=${MARKER_TIMEOUT:-$RUN_TIMEOUT_SECS}
MARKER_TIMEOUT_DEFAULT=$(resolve_timeout_secs "$MARKER_TIMEOUT_LEGACY" "$RUN_TIMEOUT_SECS")
MARKER_TIMEOUT_DISCOVERY_SECS=$(resolve_timeout_secs "${OS2VM_MARKER_TIMEOUT_DISCOVERY:-}" "$MARKER_TIMEOUT_DEFAULT")
MARKER_TIMEOUT_SESSION_SECS=$(resolve_timeout_secs "${OS2VM_MARKER_TIMEOUT_SESSION:-}" "$MARKER_TIMEOUT_DEFAULT")
MARKER_TIMEOUT_MUX_SECS=$(resolve_timeout_secs "${OS2VM_MARKER_TIMEOUT_MUX:-}" "$MARKER_TIMEOUT_DEFAULT")
MARKER_TIMEOUT_REMOTE_SECS=$(resolve_timeout_secs "${OS2VM_MARKER_TIMEOUT_REMOTE:-}" "$MARKER_TIMEOUT_DEFAULT")
# Phase-D style deterministic runtime budgets (opt-out via OS2VM_BUDGET_ENABLE=0).
OS2VM_BUDGET_ENABLE=${OS2VM_BUDGET_ENABLE:-1}
OS2VM_BUDGET_DISCOVERY_MS=${OS2VM_BUDGET_DISCOVERY_MS:-15000}
OS2VM_BUDGET_SESSION_MS=${OS2VM_BUDGET_SESSION_MS:-5000}
OS2VM_BUDGET_MUX_MS=${OS2VM_BUDGET_MUX_MS:-5000}
OS2VM_BUDGET_REMOTE_MS=${OS2VM_BUDGET_REMOTE_MS:-60000}
OS2VM_BUDGET_TOTAL_MS=${OS2VM_BUDGET_TOTAL_MS:-120000}
# Phase-E style bounded soak stability gate (opt-out via OS2VM_SOAK_ENABLE=0).
OS2VM_SOAK_ENABLE=${OS2VM_SOAK_ENABLE:-1}
OS2VM_SOAK_DURATION_SECS=$(resolve_timeout_secs_uncapped "${OS2VM_SOAK_DURATION:-15s}" "15")
if (( OS2VM_SOAK_DURATION_SECS < 0 )); then
  OS2VM_SOAK_DURATION_SECS=0
fi
OS2VM_SOAK_ROUNDS=${OS2VM_SOAK_ROUNDS:-1}
if ! [[ "$OS2VM_SOAK_ROUNDS" =~ ^[0-9]+$ ]]; then
  OS2VM_SOAK_ROUNDS=1
fi
if (( OS2VM_SOAK_ROUNDS < 1 )); then
  OS2VM_SOAK_ROUNDS=1
fi
# QEMU lifetime must outlive phase-gated waits; otherwise nodes can terminate before session/remote markers.
QEMU_TIMEOUT_SECS=$(resolve_timeout_secs_uncapped "${OS2VM_QEMU_TIMEOUT:-}" "$(( RUN_TIMEOUT_SECS * 4 ))")
if (( QEMU_TIMEOUT_SECS < RUN_TIMEOUT_SECS )); then
  QEMU_TIMEOUT_SECS=$RUN_TIMEOUT_SECS
fi

LAST_WAIT_HIT_MS=""
LAST_WAIT_LINE=""
LAST_WAIT_A_LINE=""
LAST_WAIT_B_LINE=""

wait_marker() {
  local file=$1
  local pattern=$2
  local terminal_pattern=${3:-}
  local timeout_secs=$4
  local deadline=$(( $(date +%s) + timeout_secs ))
  LAST_WAIT_HIT_MS=""
  LAST_WAIT_LINE=""
  while (( $(date +%s) < deadline )); do
    if [[ -s "$file" ]] && grep -q "$pattern" "$file" 2>/dev/null; then
      LAST_WAIT_HIT_MS=$(now_ms)
      LAST_WAIT_LINE=$(marker_line "$file" "$pattern")
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
  local timeout_secs=$6
  local pid_a=${7:-}
  local pid_b=${8:-}
  local deadline=$(( $(date +%s) + timeout_secs ))
  local started_at=$(date +%s)
  local next_progress_at=$(( started_at + 5 ))
  LAST_WAIT_A_LINE=""
  LAST_WAIT_B_LINE=""

  while (( $(date +%s) < deadline )); do
    local a_ok=0
    local b_ok=0

    if [[ -s "$file_a" ]] && grep -q "$pattern_a" "$file_a" 2>/dev/null; then
      a_ok=1
      LAST_WAIT_A_LINE=$(marker_line "$file_a" "$pattern_a")
    fi
    if [[ -s "$file_b" ]] && grep -q "$pattern_b" "$file_b" 2>/dev/null; then
      b_ok=1
      LAST_WAIT_B_LINE=$(marker_line "$file_b" "$pattern_b")
    fi

    if (( a_ok == 1 && b_ok == 1 )); then
      LAST_WAIT_HIT_MS=$(now_ms)
      return 0
    fi

    if (( $(date +%s) >= next_progress_at )); then
      local now_ts
      local elapsed
      local remaining
      now_ts=$(date +%s)
      elapsed=$(( now_ts - started_at ))
      remaining=$(( deadline - now_ts ))
      if (( remaining < 0 )); then
        remaining=0
      fi
      echo "[wait] cross-vm markers pending: A=$a_ok B=$b_ok elapsed=${elapsed}s remaining=${remaining}s"
      next_progress_at=$(( now_ts + 5 ))
    fi

    if [[ -n "$terminal_pattern" ]]; then
      if (( a_ok == 0 )) && [[ -s "$file_a" ]] && grep -q "$terminal_pattern" "$file_a" 2>/dev/null; then
        return 2
      fi
      if (( b_ok == 0 )) && [[ -s "$file_b" ]] && grep -q "$terminal_pattern" "$file_b" 2>/dev/null; then
        return 3
      fi
    fi
    if (( a_ok == 0 )) && [[ -n "$pid_a" ]] && ! is_pid_alive "$pid_a"; then
      return 2
    fi
    if (( b_ok == 0 )) && [[ -n "$pid_b" ]] && ! is_pid_alive "$pid_b"; then
      return 3
    fi

    sleep 1
  done

  return 1
}

declare -a PHASE_ORDER=("build" "launch" "discovery" "session" "mux" "remote" "perf" "soak" "end")
declare -A PHASE_START_MS
declare -A PHASE_END_MS
declare -A PHASE_DURATION_MS
declare -A PHASE_STATUS
declare -A MARKER_LINES
declare -A EVIDENCE
declare -A PCAP_STATS

phase_index() {
  local needle=$1
  local i=0
  for p in "${PHASE_ORDER[@]}"; do
    if [[ "$p" == "$needle" ]]; then
      echo "$i"
      return 0
    fi
    i=$(( i + 1 ))
  done
  echo "-1"
}

TARGET_PHASE_INDEX=$(phase_index "$RUN_PHASE")
if (( TARGET_PHASE_INDEX < 0 )); then
  echo "[error] Unknown RUN_PHASE='$RUN_PHASE' (supported: ${PHASE_ORDER[*]})" >&2
  exit 2
fi

phase_should_run() {
  local idx
  idx=$(phase_index "$1")
  (( idx >= 0 && idx <= TARGET_PHASE_INDEX ))
}

phase_begin() {
  local phase=$1
  PHASE_START_MS["$phase"]=$(now_ms)
  PHASE_STATUS["$phase"]="running"
  echo "[info] Phase start: $phase"
}

phase_finish() {
  local phase=$1
  local status=$2
  local end_ms
  local start_ms
  end_ms=$(now_ms)
  start_ms=${PHASE_START_MS["$phase"]:-$end_ms}
  PHASE_END_MS["$phase"]=$end_ms
  PHASE_DURATION_MS["$phase"]=$(( end_ms - start_ms ))
  PHASE_STATUS["$phase"]=$status
  echo "[info] Phase done: $phase status=$status duration_ms=${PHASE_DURATION_MS["$phase"]}"
}

OS2VM_PCAP_MODE="off"
case "$OS2VM_PCAP" in
  0|off|OFF|false|FALSE)
    OS2VM_PCAP_MODE="off"
    ;;
  1|on|ON|true|TRUE)
    OS2VM_PCAP_MODE="on"
    ;;
  auto|AUTO)
    OS2VM_PCAP_MODE="auto"
    ;;
  *)
    echo "[warn] Unknown OS2VM_PCAP='$OS2VM_PCAP', using 'off'." >&2
    OS2VM_PCAP_MODE="off"
    ;;
esac
ENABLE_PCAP_CAPTURE=0
if [[ "$OS2VM_PCAP_MODE" == "on" || "$OS2VM_PCAP_MODE" == "auto" ]]; then
  ENABLE_PCAP_CAPTURE=1
fi

RESULT="running"
FAILED_PHASE=""
MISSING_MARKER=""
ERROR_CODE=""
ERROR_NODE=""
ERROR_SUBSYSTEM=""
ERROR_MESSAGE=""
ERROR_HINT=""
ERROR_CONFIDENCE="0.00"
ERROR_TYPED_EXIT=1
FINAL_EXIT_CODE=0
PID_A=""
PID_B=""
QEMU_EXIT_A="not_started"
QEMU_EXIT_B="not_started"
CLEANUP_DONE=0
TCPDUMP_AVAILABLE=0
if command -v tcpdump >/dev/null 2>&1; then
  TCPDUMP_AVAILABLE=1
fi

apply_error_matrix() {
  case "$ERROR_CODE" in
    OS2VM_E_DISCOVERY_TIMEOUT)
      ERROR_SUBSYSTEM="dsoftbusd/discovery"
      ERROR_HINT="check discovery announce/recv flow and UDP port 37020 packet path"
      ERROR_CONFIDENCE="0.78"
      ERROR_TYPED_EXIT=31
      ;;
    OS2VM_E_DISCOVERY_NODE_A_ENDED)
      ERROR_SUBSYSTEM="selftest-client/node-a"
      ERROR_HINT="node A reached terminal marker before discovery success"
      ERROR_CONFIDENCE="0.88"
      ERROR_TYPED_EXIT=32
      ;;
    OS2VM_E_DISCOVERY_NODE_B_ENDED)
      ERROR_SUBSYSTEM="selftest-client/node-b"
      ERROR_HINT="node B reached terminal marker before discovery success"
      ERROR_CONFIDENCE="0.88"
      ERROR_TYPED_EXIT=33
      ;;
    OS2VM_E_SESSION_TIMEOUT)
      ERROR_SUBSYSTEM="dsoftbusd/session"
      ERROR_HINT="session marker missing; inspect transport connect/accept and handshake steps"
      ERROR_CONFIDENCE="0.72"
      ERROR_TYPED_EXIT=41
      ;;
    OS2VM_E_SESSION_NODE_A_ENDED)
      ERROR_SUBSYSTEM="selftest-client/node-a"
      ERROR_HINT="node A reached terminal marker before cross-vm session success"
      ERROR_CONFIDENCE="0.88"
      ERROR_TYPED_EXIT=42
      ;;
    OS2VM_E_SESSION_NODE_B_ENDED)
      ERROR_SUBSYSTEM="selftest-client/node-b"
      ERROR_HINT="node B reached terminal marker before cross-vm session success"
      ERROR_CONFIDENCE="0.88"
      ERROR_TYPED_EXIT=43
      ;;
    OS2VM_E_SESSION_NO_SYN)
      ERROR_SUBSYSTEM="netstackd/connect"
      ERROR_HINT="no SYN seen in PCAP; inspect dial issuance and netstack connect path"
      ERROR_CONFIDENCE="0.92"
      ERROR_TYPED_EXIT=44
      ;;
    OS2VM_E_SESSION_NO_SYNACK)
      ERROR_SUBSYSTEM="netstackd/accept-or-network"
      ERROR_HINT="SYN seen without SYN-ACK; inspect peer listen readiness and link path"
      ERROR_CONFIDENCE="0.90"
      ERROR_TYPED_EXIT=45
      ;;
    OS2VM_E_MUX_TIMEOUT)
      ERROR_SUBSYSTEM="dsoftbusd/mux-crossvm"
      ERROR_HINT="cross-vm mux marker ladder timed out; inspect mux handshake/data flow in both nodes"
      ERROR_CONFIDENCE="0.84"
      ERROR_TYPED_EXIT=46
      ;;
    OS2VM_E_MUX_NODE_A_ENDED)
      ERROR_SUBSYSTEM="selftest-client/node-a"
      ERROR_HINT="node A ended before cross-vm mux marker ladder completed"
      ERROR_CONFIDENCE="0.90"
      ERROR_TYPED_EXIT=47
      ;;
    OS2VM_E_MUX_NODE_B_ENDED)
      ERROR_SUBSYSTEM="selftest-client/node-b"
      ERROR_HINT="node B ended before cross-vm mux marker ladder completed"
      ERROR_CONFIDENCE="0.90"
      ERROR_TYPED_EXIT=48
      ;;
    OS2VM_E_MUX_NEGATIVE_MARKER)
      ERROR_SUBSYSTEM="dsoftbusd/mux-crossvm"
      ERROR_HINT="mux fail marker was observed despite success ladder; treat as fake-green contradiction"
      ERROR_CONFIDENCE="0.96"
      ERROR_TYPED_EXIT=49
      ;;
    OS2VM_E_PERF_BUDGET_DISCOVERY)
      ERROR_SUBSYSTEM="dsoftbusd/perf-discovery"
      ERROR_HINT="discovery phase exceeded deterministic budget; inspect discovery churn and retries"
      ERROR_CONFIDENCE="0.91"
      ERROR_TYPED_EXIT=81
      ;;
    OS2VM_E_PERF_BUDGET_SESSION)
      ERROR_SUBSYSTEM="dsoftbusd/perf-session"
      ERROR_HINT="session phase exceeded deterministic budget; inspect dial/accept/handshake latency"
      ERROR_CONFIDENCE="0.91"
      ERROR_TYPED_EXIT=82
      ;;
    OS2VM_E_PERF_BUDGET_MUX)
      ERROR_SUBSYSTEM="dsoftbusd/perf-mux"
      ERROR_HINT="cross-vm mux ladder exceeded deterministic budget; inspect mux roundtrip flow"
      ERROR_CONFIDENCE="0.91"
      ERROR_TYPED_EXIT=83
      ;;
    OS2VM_E_PERF_BUDGET_REMOTE)
      ERROR_SUBSYSTEM="dsoftbusd/perf-remote"
      ERROR_HINT="remote proxy phase exceeded deterministic budget; inspect remote fs/rpc path latency"
      ERROR_CONFIDENCE="0.91"
      ERROR_TYPED_EXIT=84
      ;;
    OS2VM_E_PERF_BUDGET_TOTAL)
      ERROR_SUBSYSTEM="dsoftbusd/perf-total"
      ERROR_HINT="total 2-VM runtime exceeded deterministic budget; inspect cross-phase regressions"
      ERROR_CONFIDENCE="0.90"
      ERROR_TYPED_EXIT=85
      ;;
    OS2VM_E_SOAK_NODE_A_ENDED)
      ERROR_SUBSYSTEM="selftest-client/node-a"
      ERROR_HINT="node A terminated during bounded soak window"
      ERROR_CONFIDENCE="0.92"
      ERROR_TYPED_EXIT=86
      ;;
    OS2VM_E_SOAK_NODE_B_ENDED)
      ERROR_SUBSYSTEM="selftest-client/node-b"
      ERROR_HINT="node B terminated during bounded soak window"
      ERROR_CONFIDENCE="0.92"
      ERROR_TYPED_EXIT=87
      ;;
    OS2VM_E_SOAK_FAIL_MARKER)
      ERROR_SUBSYSTEM="dsoftbusd/soak-guard"
      ERROR_HINT="fail/panic marker appeared during bounded soak window; inspect UART logs"
      ERROR_CONFIDENCE="0.95"
      ERROR_TYPED_EXIT=88
      ;;
    OS2VM_E_REMOTE_RESOLVE_MISSING)
      ERROR_SUBSYSTEM="dsoftbusd/remote-proxy-resolve"
      ERROR_HINT="remote resolve marker missing; inspect proxy dependency and resolve RPC path"
      ERROR_CONFIDENCE="0.82"
      ERROR_TYPED_EXIT=51
      ;;
    OS2VM_E_REMOTE_QUERY_MISSING)
      ERROR_SUBSYSTEM="dsoftbusd/remote-proxy-query"
      ERROR_HINT="remote query marker missing; inspect bundle-list proxy request/response path"
      ERROR_CONFIDENCE="0.82"
      ERROR_TYPED_EXIT=52
      ;;
    OS2VM_E_REMOTE_PKGFS_STAT_MISSING)
      ERROR_SUBSYSTEM="dsoftbusd/remote-proxy-packagefs-stat"
      ERROR_HINT="remote pkgfs STAT marker missing; inspect packagefs resolve/status path"
      ERROR_CONFIDENCE="0.86"
      ERROR_TYPED_EXIT=53
      ;;
    OS2VM_E_REMOTE_PKGFS_OPEN_MISSING)
      ERROR_SUBSYSTEM="dsoftbusd/remote-proxy-packagefs-open"
      ERROR_HINT="remote pkgfs OPEN marker missing; inspect open handle allocation/limits"
      ERROR_CONFIDENCE="0.86"
      ERROR_TYPED_EXIT=54
      ;;
    OS2VM_E_REMOTE_PKGFS_READ_MISSING)
      ERROR_SUBSYSTEM="dsoftbusd/remote-proxy-packagefs-read"
      ERROR_HINT="remote pkgfs READ marker missing; inspect read transport and status mapping"
      ERROR_CONFIDENCE="0.86"
      ERROR_TYPED_EXIT=55
      ;;
    OS2VM_E_REMOTE_PKGFS_CLOSE_MISSING)
      ERROR_SUBSYSTEM="dsoftbusd/remote-proxy-packagefs-close"
      ERROR_HINT="remote pkgfs CLOSE marker missing; inspect close status and handle lifecycle"
      ERROR_CONFIDENCE="0.86"
      ERROR_TYPED_EXIT=56
      ;;
    OS2VM_E_REMOTE_PKGFS_FLOW_MISSING)
      ERROR_SUBSYSTEM="dsoftbusd/remote-proxy-packagefs"
      ERROR_HINT="remote packagefs final flow marker missing; inspect step ladder and selftest loop"
      ERROR_CONFIDENCE="0.84"
      ERROR_TYPED_EXIT=57
      ;;
    OS2VM_E_REMOTE_SERVED_MISSING)
      ERROR_SUBSYSTEM="dsoftbusd/remote-proxy-node-b"
      ERROR_HINT="node B did not emit remote packagefs served marker"
      ERROR_CONFIDENCE="0.84"
      ERROR_TYPED_EXIT=58
      ;;
    OS2VM_E_REMOTE_STATEFS_FLOW_MISSING)
      ERROR_SUBSYSTEM="dsoftbusd/remote-proxy-statefs"
      ERROR_HINT="remote statefs final flow marker missing; inspect statefs proxy and backend path"
      ERROR_CONFIDENCE="0.86"
      ERROR_TYPED_EXIT=60
      ;;
    OS2VM_E_REMOTE_STATEFS_SERVED_MISSING)
      ERROR_SUBSYSTEM="dsoftbusd/remote-proxy-statefs-node-b"
      ERROR_HINT="node B did not emit remote statefs served marker"
      ERROR_CONFIDENCE="0.86"
      ERROR_TYPED_EXIT=65
      ;;
    OS2VM_E_REMOTE_NEGATIVE_MARKER)
      ERROR_SUBSYSTEM="selftest-client/remote"
      ERROR_HINT="remote FAIL marker observed despite success ladder; treat as fake-green contradiction"
      ERROR_CONFIDENCE="0.96"
      ERROR_TYPED_EXIT=66
      ;;
    OS2VM_E_BUILD_ARTIFACT_MISSING)
      ERROR_SUBSYSTEM="build/artifacts"
      ERROR_HINT="required build artifacts missing; run build phase or verify target dir"
      ERROR_CONFIDENCE="0.98"
      ERROR_TYPED_EXIT=61
      ;;
    OS2VM_E_HOST_ENOSPC)
      ERROR_SUBSYSTEM="host/storage"
      ERROR_HINT="insufficient free disk space for build/log/tmp paths"
      ERROR_CONFIDENCE="0.99"
      ERROR_TYPED_EXIT=62
      ;;
    OS2VM_E_LAUNCH_NODE_A_FAILED)
      ERROR_SUBSYSTEM="qemu/launch"
      ERROR_HINT="node A did not start; inspect host-A.txt and qemu invocation"
      ERROR_CONFIDENCE="0.99"
      ERROR_TYPED_EXIT=71
      ;;
    OS2VM_E_LAUNCH_NODE_B_FAILED)
      ERROR_SUBSYSTEM="qemu/launch"
      ERROR_HINT="node B did not start; inspect host-B.txt and qemu invocation"
      ERROR_CONFIDENCE="0.99"
      ERROR_TYPED_EXIT=72
      ;;
    *)
      ERROR_SUBSYSTEM="unknown"
      ERROR_HINT="inspect summary + UART + host logs for unexpected error"
      ERROR_CONFIDENCE="0.40"
      ERROR_TYPED_EXIT=99
      ;;
  esac
}

set_failure() {
  local code=$1
  local phase=$2
  local node=$3
  local message=$4
  if [[ "$RESULT" == "failed" ]]; then
    return 0
  fi
  RESULT="failed"
  ERROR_CODE=$code
  FAILED_PHASE=$phase
  ERROR_NODE=$node
  ERROR_MESSAGE=$message
  apply_error_matrix
}

resolve_exit_code() {
  if [[ "$RESULT" == "success" ]]; then
    echo 0
    return 0
  fi
  if [[ "$OS2VM_EXIT_CODE_MODE" == "typed" ]]; then
    echo "$ERROR_TYPED_EXIT"
  else
    echo 1
  fi
}

set_env_var() {
  local name=$1
  local value=$2
  printf -v "$name" '%s' "$value"
  export "$name"
}

build_os_once() {
  export RUSTFLAGS="$RUSTFLAGS_OS"
  export CARGO_TARGET_DIR="$BUILD_TARGET_DIR"

  # Keep this aligned with init-lite expectations (os_payload): include updated + logd + metricsd + statefsd
  # so policy-gated MMIO grants and persistence bring-up don't fatal during cross-VM runs.
  local services="logd,metricsd,updated,timed,keystored,rngd,policyd,samgrd,bundlemgrd,packagefsd,vfsd,execd,statefsd,netstackd,dsoftbusd,selftest-client"
  export INIT_LITE_SERVICE_LIST="$services"

  IFS=',' read -r -a svcs <<<"$services"
  for raw in "${svcs[@]}"; do
    local svc=${raw//[[:space:]]/}
    [[ -z "$svc" ]] && continue

    (cd "$ROOT" && RUSTFLAGS="$RUSTFLAGS_OS" cargo build -p "$svc" --target "$TARGET" --release --no-default-features --features os-lite)

    local svc_upper
    svc_upper=$(echo "$svc" | tr '[:lower:]' '[:upper:]' | tr '-' '_')
    set_env_var "INIT_LITE_SERVICE_${svc_upper}_ELF" "$BUILD_TARGET_DIR/$TARGET/release/$svc"
    local stack_var="INIT_LITE_SERVICE_${svc_upper}_STACK_PAGES"
    if [[ -z "${!stack_var:-}" ]]; then
      set_env_var "$stack_var" "8"
    fi
  done

  (cd "$ROOT" && RUSTFLAGS="$RUSTFLAGS_OS" cargo build -p init-lite --target "$TARGET" --release)

  local INIT_ELF="$BUILD_TARGET_DIR/$TARGET/release/init-lite"
  (cd "$ROOT" && EMBED_INIT_ELF="$INIT_ELF" RUSTFLAGS="$RUSTFLAGS_OS" cargo build -p neuron-boot --target "$TARGET" --release)

  local KERNEL_ELF="$BUILD_TARGET_DIR/$TARGET/release/neuron-boot"
  local KERNEL_BIN="$BUILD_TARGET_DIR/$TARGET/release/neuron-boot.bin"
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
  local qemu_name
  qemu_name=$(qemu_name_for_node "$name")

  local KERNEL_BIN="$BUILD_TARGET_DIR/$TARGET/release/neuron-boot.bin"
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
    -name "$qemu_name"
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

  if (( ENABLE_PCAP_CAPTURE == 1 )); then
    local pcap
    local filter_id
    local run_tag="$RUN_TAG"
    if [[ -z "$run_tag" ]]; then
      run_tag="run"
    fi
    if [[ "$name" == "A" ]]; then
      pcap="$PCAP_A"
      filter_id="pcapA${run_tag}"
    else
      pcap="$PCAP_B"
      filter_id="pcapB${run_tag}"
    fi
    # Capture raw packets on netdev=n0 into a PCAP file (Wireshark-readable).
    # Mode "auto" keeps captures only for failed runs.
    args+=(-object "filter-dump,id=${filter_id},netdev=n0,file=${pcap}")
  fi

  # QEMU runtime is bounded; build is done already.
  timeout --foreground "${QEMU_TIMEOUT_SECS}s" stdbuf -oL qemu-system-riscv64 "${args[@]}" \
    | tee "$uart" \
    >"$hostlog" 2>&1 &

  echo $!
}

verify_required_artifacts() {
  local -a required=(
    "$BUILD_TARGET_DIR/$TARGET/release/netstackd"
    "$BUILD_TARGET_DIR/$TARGET/release/dsoftbusd"
    "$BUILD_TARGET_DIR/$TARGET/release/selftest-client"
    "$BUILD_TARGET_DIR/$TARGET/release/init-lite"
    "$BUILD_TARGET_DIR/$TARGET/release/neuron-boot"
    "$BUILD_TARGET_DIR/$TARGET/release/neuron-boot.bin"
  )
  local missing=0
  for path in "${required[@]}"; do
    if [[ ! -f "$path" ]]; then
      echo "[error] missing artifact: $path" >&2
      missing=1
    fi
  done
  if (( missing == 1 )); then
    set_failure "OS2VM_E_BUILD_ARTIFACT_MISSING" "build" "host" "required OS2VM artifacts are missing"
    return 1
  fi
  return 0
}

phase_build() {
  echo "[info] Build target dir: $BUILD_TARGET_DIR"
  local logdir_free_kb
  local tmp_free_kb
  local tmp_path
  local tmp_min_kb=65536
  if [[ "$OS2VM_SKIP_BUILD" != "1" ]]; then
    tmp_min_kb=524288
  fi
  logdir_free_kb=$(df_available_kb "$LOG_DIR")
  tmp_path="${TMPDIR:-/tmp}"
  gc_sandbox_cache
  tmp_free_kb=$(df_available_kb "$tmp_path")
  # #region agent log
  agent_session_log "I" "tools/os2vm.sh:phase_build" "host disk free preflight" "{\"logDir\":\"$LOG_DIR\",\"tmpDir\":\"$tmp_path\",\"logDirFreeKb\":$logdir_free_kb,\"tmpFreeKb\":$tmp_free_kb,\"tmpMinKb\":$tmp_min_kb}"
  # #endregion
  if [[ "$tmp_free_kb" =~ ^[0-9]+$ ]] && (( tmp_free_kb >= 0 && tmp_free_kb < tmp_min_kb )); then
    local auto_tmp="$LOG_DIR/tmp-fallback"
    mkdir -p "$auto_tmp"
    export TMPDIR="$auto_tmp"
    tmp_path="$TMPDIR"
    tmp_free_kb=$(df_available_kb "$tmp_path")
    echo "[warn] low tmp free space; using TMPDIR=$TMPDIR"
    # #region agent log
    agent_session_log "I" "tools/os2vm.sh:phase_build" "tmp fallback applied" "{\"fallbackTmpDir\":\"$tmp_path\",\"tmpFreeKb\":$tmp_free_kb,\"tmpMinKb\":$tmp_min_kb}"
    # #endregion
  fi
  if [[ "$logdir_free_kb" =~ ^[0-9]+$ ]] && (( logdir_free_kb < 65536 )); then
    set_failure "OS2VM_E_HOST_ENOSPC" "build" "host" "insufficient disk space in LOG_DIR for harness outputs"
    return 1
  fi
  if [[ "$tmp_free_kb" =~ ^[0-9]+$ ]] && (( tmp_free_kb >= 0 && tmp_free_kb < tmp_min_kb )); then
    set_failure "OS2VM_E_HOST_ENOSPC" "build" "host" "insufficient tmp disk space for rust/qemu operations"
    return 1
  fi
  if [[ "$OS2VM_SKIP_BUILD" == "1" ]]; then
    echo "[info] OS2VM_SKIP_BUILD=1 -> validating artifacts only."
    verify_required_artifacts
  else
    echo "[info] Building OS artifacts once..."
    build_os_once
    verify_required_artifacts
  fi

  local netstackd_elf="$BUILD_TARGET_DIR/$TARGET/release/netstackd"
  local dsoftbusd_elf="$BUILD_TARGET_DIR/$TARGET/release/dsoftbusd"
  local crossvm_src="$ROOT/source/services/dsoftbusd/src/os/session/cross_vm.rs"
  local skip_build=false
  local build_executed=false
  local netstackd_exists=false
  local dsoftbusd_exists=false
  local dsoftbusd_mtime=0
  local crossvm_src_mtime=0
  if [[ "$OS2VM_SKIP_BUILD" == "1" ]]; then
    skip_build=true
  fi
  if [[ "$OS2VM_SKIP_BUILD" != "1" ]]; then
    build_executed=true
  fi
  if [[ -f "$netstackd_elf" ]]; then
    netstackd_exists=true
  fi
  if [[ -f "$dsoftbusd_elf" ]]; then
    dsoftbusd_exists=true
  fi
  dsoftbusd_mtime=$(file_mtime_or_zero "$dsoftbusd_elf")
  crossvm_src_mtime=$(file_mtime_or_zero "$crossvm_src")
  agent_session_log "A" "tools/os2vm.sh:build_artifacts" "verify os2vm build artifact paths" "{\"buildTargetDir\":\"$BUILD_TARGET_DIR\",\"target\":\"$TARGET\",\"skipBuild\":$skip_build,\"buildExecuted\":$build_executed,\"netstackdElf\":\"$netstackd_elf\",\"netstackdExists\":$netstackd_exists,\"dsoftbusdElf\":\"$dsoftbusd_elf\",\"dsoftbusdExists\":$dsoftbusd_exists,\"dsoftbusdMtime\":$dsoftbusd_mtime,\"crossVmSrcMtime\":$crossvm_src_mtime}"
}

phase_launch() {
  cleanup_stale_pid_records
  terminate_named_qemu "$(qemu_name_for_node A)"
  terminate_named_qemu "$(qemu_name_for_node B)"
  echo "[info] Launching Node A..."
  PID_A=$(launch_qemu A "$A_MAC" "$UART_A" "$HOST_A")
  echo "[info] Launching Node B..."
  PID_B=$(launch_qemu B "$B_MAC" "$UART_B" "$HOST_B")
  persist_pid_snapshot
  if ! is_pid_alive "$PID_A"; then
    set_failure "OS2VM_E_LAUNCH_NODE_A_FAILED" "launch" "A" "node A process did not stay alive after launch"
    return 1
  fi
  if ! is_pid_alive "$PID_B"; then
    set_failure "OS2VM_E_LAUNCH_NODE_B_FAILED" "launch" "B" "node B process did not stay alive after launch"
    return 1
  fi
  agent_debug_log "H2" "tools/os2vm.sh:launch" "qemu nodes launched" "{\"runTimeout\":\"$RUN_TIMEOUT\",\"qemuTimeoutSecs\":$QEMU_TIMEOUT_SECS,\"markerTimeoutDefault\":$MARKER_TIMEOUT_DEFAULT,\"pidA\":$PID_A,\"pidB\":$PID_B,\"pcapMode\":\"$OS2VM_PCAP_MODE\"}"
  # #region agent log
  agent_session_log "D" "tools/os2vm.sh:launch" "verify socket backend launch parameters" "{\"netdevA\":\"$NETDEV_A\",\"netdevB\":\"$NETDEV_B\",\"pidA\":\"$PID_A\",\"pidB\":\"$PID_B\"}"
  # #endregion
  agent_session_log "F" "tools/os2vm.sh:launch" "verify modern virtio-mmio launch setting" "{\"qemuArg\":\"-global virtio-mmio.force-legacy=off\",\"enabled\":true}"
}

phase_discovery() {
  local marker="dsoftbusd: discovery cross-vm up"
  local rc=0
  echo "[info] Waiting for cross-VM discovery markers..."
  wait_dual_markers "$UART_A" "$marker" "$UART_B" "$marker" "" "$MARKER_TIMEOUT_DISCOVERY_SECS" "$PID_A" "$PID_B" || rc=$?
  if [[ $rc -ne 0 ]]; then
    EVIDENCE["a_discovery"]=$(count_marker "$UART_A" "dsoftbusd: discovery cross-vm up")
    EVIDENCE["b_discovery"]=$(count_marker "$UART_B" "dsoftbusd: discovery cross-vm up")
    EVIDENCE["a_fallback_100215"]=$(count_marker "$UART_A" "fallback static 10.0.2.15/24")
    EVIDENCE["b_fallback_100215"]=$(count_marker "$UART_B" "fallback static 10.0.2.15/24")
    MISSING_MARKER="$marker (both nodes)"
    if [[ $rc -eq 2 ]]; then
      set_failure "OS2VM_E_DISCOVERY_NODE_A_ENDED" "discovery" "A" "node A ended before discovery marker"
      return 1
    elif [[ $rc -eq 3 ]]; then
      set_failure "OS2VM_E_DISCOVERY_NODE_B_ENDED" "discovery" "B" "node B ended before discovery marker"
      return 1
    fi
    set_failure "OS2VM_E_DISCOVERY_TIMEOUT" "discovery" "both" "discovery marker timeout"
    return 1
  fi
  MARKER_LINES["A_discovery"]=$LAST_WAIT_A_LINE
  MARKER_LINES["B_discovery"]=$LAST_WAIT_B_LINE
  return 0
}

phase_session() {
  local marker="dsoftbusd: cross-vm session ok"
  local rc=0
  echo "[info] Waiting for cross-VM session markers..."
  wait_dual_markers "$UART_A" "$marker" "$UART_B" "$marker" "" "$MARKER_TIMEOUT_SESSION_SECS" "$PID_A" "$PID_B" || rc=$?
  if [[ $rc -ne 0 ]]; then
    EVIDENCE["a_dial_would_block"]=$(count_marker "$UART_A" "dbg:dsoftbusd: dial status would_block")
    EVIDENCE["a_dial_io"]=$(count_marker "$UART_A" "dbg:dsoftbusd: dial status io")
    EVIDENCE["a_dial_other"]=$(count_marker "$UART_A" "dbg:dsoftbusd: dial status other")
    EVIDENCE["a_dial_not_found"]=$(count_marker "$UART_A" "dbg:dsoftbusd: dial status not_found")
    EVIDENCE["a_dial_malformed"]=$(count_marker "$UART_A" "dbg:dsoftbusd: dial status malformed")
    EVIDENCE["a_dial_timed_out"]=$(count_marker "$UART_A" "dbg:dsoftbusd: dial status timed_out")
    EVIDENCE["b_accept_pending"]=$(count_marker "$UART_B" "dbg:dsoftbusd: accept pending")
    EVIDENCE["a_selftest_end"]=$(count_marker "$UART_A" "SELFTEST: end")
    EVIDENCE["b_selftest_end"]=$(count_marker "$UART_B" "SELFTEST: end")
    # #region agent log
    agent_session_log "J" "tools/os2vm.sh:session_wait" "session marker wait diagnostics" "{\"waitReturnCode\":$rc,\"nodeA\":{\"dialWouldBlock\":${EVIDENCE["a_dial_would_block"]},\"dialIo\":${EVIDENCE["a_dial_io"]},\"dialOther\":${EVIDENCE["a_dial_other"]},\"dialNotFound\":${EVIDENCE["a_dial_not_found"]},\"dialMalformed\":${EVIDENCE["a_dial_malformed"]},\"dialTimedOut\":${EVIDENCE["a_dial_timed_out"]},\"selftestEnd\":${EVIDENCE["a_selftest_end"]}},\"nodeB\":{\"acceptPending\":${EVIDENCE["b_accept_pending"]},\"selftestEnd\":${EVIDENCE["b_selftest_end"]}}}"
    # #endregion
    MISSING_MARKER="$marker (both nodes)"
    if [[ $rc -eq 2 ]]; then
      set_failure "OS2VM_E_SESSION_NODE_A_ENDED" "session" "A" "node A ended before session marker"
      return 1
    elif [[ $rc -eq 3 ]]; then
      set_failure "OS2VM_E_SESSION_NODE_B_ENDED" "session" "B" "node B ended before session marker"
      return 1
    fi
    set_failure "OS2VM_E_SESSION_TIMEOUT" "session" "both" "session marker timeout"
    return 1
  fi
  MARKER_LINES["A_session"]=$LAST_WAIT_A_LINE
  MARKER_LINES["B_session"]=$LAST_WAIT_B_LINE
  return 0
}

phase_mux() {
  local -a mux_markers=(
    "dsoftbus:mux crossvm session up"
    "dsoftbus:mux crossvm data ok"
    "SELFTEST: mux crossvm pri control ok"
    "SELFTEST: mux crossvm bulk ok"
    "SELFTEST: mux crossvm backpressure ok"
  )
  local marker=""
  local rc=0
  local a_mux_fail_total=0
  local b_mux_fail_total=0
  echo "[info] Waiting for cross-VM mux ladder markers..."
  for marker in "${mux_markers[@]}"; do
    rc=0
    wait_dual_markers "$UART_A" "$marker" "$UART_B" "$marker" "" "$MARKER_TIMEOUT_MUX_SECS" "$PID_A" "$PID_B" || rc=$?
    if [[ $rc -ne 0 ]]; then
      EVIDENCE["a_mux_marker_hits"]=$(count_marker "$UART_A" "$marker")
      EVIDENCE["b_mux_marker_hits"]=$(count_marker "$UART_B" "$marker")
      EVIDENCE["a_mux_fail"]=$(count_marker "$UART_A" "dsoftbus:mux crossvm fail")
      EVIDENCE["b_mux_fail"]=$(count_marker "$UART_B" "dsoftbus:mux crossvm fail")
      MISSING_MARKER="$marker (both nodes)"
      if [[ $rc -eq 2 ]]; then
        set_failure "OS2VM_E_MUX_NODE_A_ENDED" "mux" "A" "node A ended before mux marker"
        return 1
      elif [[ $rc -eq 3 ]]; then
        set_failure "OS2VM_E_MUX_NODE_B_ENDED" "mux" "B" "node B ended before mux marker"
        return 1
      fi
      set_failure "OS2VM_E_MUX_TIMEOUT" "mux" "both" "mux marker timeout"
      return 1
    fi
    case "$marker" in
      "dsoftbus:mux crossvm session up")
        MARKER_LINES["A_mux_session"]=$LAST_WAIT_A_LINE
        MARKER_LINES["B_mux_session"]=$LAST_WAIT_B_LINE
        ;;
      "dsoftbus:mux crossvm data ok")
        MARKER_LINES["A_mux_data"]=$LAST_WAIT_A_LINE
        MARKER_LINES["B_mux_data"]=$LAST_WAIT_B_LINE
        ;;
      "SELFTEST: mux crossvm pri control ok")
        MARKER_LINES["A_mux_pri"]=$LAST_WAIT_A_LINE
        MARKER_LINES["B_mux_pri"]=$LAST_WAIT_B_LINE
        ;;
      "SELFTEST: mux crossvm bulk ok")
        MARKER_LINES["A_mux_bulk"]=$LAST_WAIT_A_LINE
        MARKER_LINES["B_mux_bulk"]=$LAST_WAIT_B_LINE
        ;;
      "SELFTEST: mux crossvm backpressure ok")
        MARKER_LINES["A_mux_backpressure"]=$LAST_WAIT_A_LINE
        MARKER_LINES["B_mux_backpressure"]=$LAST_WAIT_B_LINE
        ;;
    esac
  done

  # Fake-green guard: any explicit mux fail marker makes the run fail even if success markers exist.
  a_mux_fail_total=$(count_marker "$UART_A" "dsoftbus:mux crossvm fail")
  b_mux_fail_total=$(count_marker "$UART_B" "dsoftbus:mux crossvm fail")
  EVIDENCE["a_mux_fail_total"]=$a_mux_fail_total
  EVIDENCE["b_mux_fail_total"]=$b_mux_fail_total
  if (( a_mux_fail_total > 0 || b_mux_fail_total > 0 )); then
    MISSING_MARKER="mux-fail-marker-absent"
    set_failure "OS2VM_E_MUX_NEGATIVE_MARKER" "mux" "both" "mux fail marker observed in successful marker ladder"
    return 1
  fi

  return 0
}

phase_remote() {
  local rc=0
  echo "[info] Waiting for remote proxy markers on Node A..."

  wait_marker "$UART_A" "SELFTEST: remote resolve ok" "SELFTEST: end" "$MARKER_TIMEOUT_REMOTE_SECS" || rc=$?
  if [[ $rc -ne 0 ]]; then
    EVIDENCE["a_remote_resolve_fail"]=$(count_marker "$UART_A" "SELFTEST: remote resolve FAIL")
    EVIDENCE["a_remote_rpc_fail_resolve"]=$(count_marker "$UART_A" "dbg:dsoftbusd: remote rpc fail resolve")
    EVIDENCE["b_remote_proxy_up"]=$(count_marker "$UART_B" "dsoftbusd: remote proxy up")
    EVIDENCE["b_remote_proxy_rx"]=$(count_marker "$UART_B" "dsoftbusd: remote proxy rx")
    MISSING_MARKER="SELFTEST: remote resolve ok"
    set_failure "OS2VM_E_REMOTE_RESOLVE_MISSING" "remote" "A" "remote resolve marker missing"
    return 1
  fi
  MARKER_LINES["A_remote_resolve"]=$LAST_WAIT_LINE

  rc=0
  wait_marker "$UART_A" "SELFTEST: remote query ok" "SELFTEST: end" "$MARKER_TIMEOUT_REMOTE_SECS" || rc=$?
  if [[ $rc -ne 0 ]]; then
    EVIDENCE["a_remote_query_fail"]=$(count_marker "$UART_A" "SELFTEST: remote query FAIL")
    EVIDENCE["a_remote_rpc_fail_bundle_list"]=$(count_marker "$UART_A" "dbg:dsoftbusd: remote rpc fail bundle-list")
    MISSING_MARKER="SELFTEST: remote query ok"
    set_failure "OS2VM_E_REMOTE_QUERY_MISSING" "remote" "A" "remote query marker missing"
    return 1
  fi
  MARKER_LINES["A_remote_query"]=$LAST_WAIT_LINE

  rc=0
  wait_marker "$UART_A" "SELFTEST: remote statefs rw ok" "SELFTEST: end" "$MARKER_TIMEOUT_REMOTE_SECS" || rc=$?
  if [[ $rc -ne 0 ]]; then
    EVIDENCE["a_remote_statefs_fail"]=$(count_marker "$UART_A" "SELFTEST: remote statefs rw FAIL")
    EVIDENCE["a_remote_rpc_fail_statefs"]=$(count_marker "$UART_A" "dbg:dsoftbusd: remote rpc fail statefs")
    EVIDENCE["b_remote_statefs_served"]=$(count_marker "$UART_B" "dsoftbusd: remote statefs served")
    MISSING_MARKER="SELFTEST: remote statefs rw ok"
    set_failure "OS2VM_E_REMOTE_STATEFS_FLOW_MISSING" "remote" "A" "remote statefs final marker missing"
    return 1
  fi
  MARKER_LINES["A_remote_statefs_flow"]=$LAST_WAIT_LINE

  rc=0
  wait_marker "$UART_B" "dsoftbusd: remote statefs served" "SELFTEST: end" "$MARKER_TIMEOUT_REMOTE_SECS" || rc=$?
  if [[ $rc -ne 0 ]]; then
    EVIDENCE["b_remote_statefs_served"]=$(count_marker "$UART_B" "dsoftbusd: remote statefs served")
    MISSING_MARKER="dsoftbusd: remote statefs served"
    set_failure "OS2VM_E_REMOTE_STATEFS_SERVED_MISSING" "remote" "B" "node B missing remote statefs served marker"
    return 1
  fi
  MARKER_LINES["B_remote_statefs_served"]=$LAST_WAIT_LINE

  rc=0
  wait_marker "$UART_A" "SELFTEST: remote pkgfs stat ok" "SELFTEST: end" "$MARKER_TIMEOUT_REMOTE_SECS" || rc=$?
  if [[ $rc -ne 0 ]]; then
    EVIDENCE["a_remote_pkgfs_fail"]=$(count_marker "$UART_A" "SELFTEST: remote pkgfs read FAIL")
    EVIDENCE["a_rpc_fail_pkgfs_stat_open"]=$(count_marker "$UART_A" "dbg:dsoftbusd: remote rpc fail pkgfs-stat-open")
    EVIDENCE["a_rpc_fail_pkgfs_read"]=$(count_marker "$UART_A" "dbg:dsoftbusd: remote rpc fail pkgfs-read")
    EVIDENCE["a_rpc_fail_pkgfs_close"]=$(count_marker "$UART_A" "dbg:dsoftbusd: remote rpc fail pkgfs-close")
    MISSING_MARKER="SELFTEST: remote pkgfs stat ok"
    set_failure "OS2VM_E_REMOTE_PKGFS_STAT_MISSING" "remote" "A" "remote packagefs stat marker missing"
    return 1
  fi
  MARKER_LINES["A_remote_pkgfs_stat"]=$LAST_WAIT_LINE

  rc=0
  wait_marker "$UART_A" "SELFTEST: remote pkgfs open ok" "SELFTEST: end" "$MARKER_TIMEOUT_REMOTE_SECS" || rc=$?
  if [[ $rc -ne 0 ]]; then
    EVIDENCE["a_remote_pkgfs_fail"]=$(count_marker "$UART_A" "SELFTEST: remote pkgfs read FAIL")
    EVIDENCE["a_rpc_fail_pkgfs_stat_open"]=$(count_marker "$UART_A" "dbg:dsoftbusd: remote rpc fail pkgfs-stat-open")
    MISSING_MARKER="SELFTEST: remote pkgfs open ok"
    set_failure "OS2VM_E_REMOTE_PKGFS_OPEN_MISSING" "remote" "A" "remote packagefs open marker missing"
    return 1
  fi
  MARKER_LINES["A_remote_pkgfs_open"]=$LAST_WAIT_LINE

  rc=0
  wait_marker "$UART_A" "SELFTEST: remote pkgfs read step ok" "SELFTEST: end" "$MARKER_TIMEOUT_REMOTE_SECS" || rc=$?
  if [[ $rc -ne 0 ]]; then
    EVIDENCE["a_remote_pkgfs_fail"]=$(count_marker "$UART_A" "SELFTEST: remote pkgfs read FAIL")
    EVIDENCE["a_rpc_fail_pkgfs_read"]=$(count_marker "$UART_A" "dbg:dsoftbusd: remote rpc fail pkgfs-read")
    MISSING_MARKER="SELFTEST: remote pkgfs read step ok"
    set_failure "OS2VM_E_REMOTE_PKGFS_READ_MISSING" "remote" "A" "remote packagefs read-step marker missing"
    return 1
  fi
  MARKER_LINES["A_remote_pkgfs_read"]=$LAST_WAIT_LINE

  rc=0
  wait_marker "$UART_A" "SELFTEST: remote pkgfs close ok" "SELFTEST: end" "$MARKER_TIMEOUT_REMOTE_SECS" || rc=$?
  if [[ $rc -ne 0 ]]; then
    EVIDENCE["a_remote_pkgfs_fail"]=$(count_marker "$UART_A" "SELFTEST: remote pkgfs read FAIL")
    EVIDENCE["a_rpc_fail_pkgfs_close"]=$(count_marker "$UART_A" "dbg:dsoftbusd: remote rpc fail pkgfs-close")
    MISSING_MARKER="SELFTEST: remote pkgfs close ok"
    set_failure "OS2VM_E_REMOTE_PKGFS_CLOSE_MISSING" "remote" "A" "remote packagefs close marker missing"
    return 1
  fi
  MARKER_LINES["A_remote_pkgfs_close"]=$LAST_WAIT_LINE

  rc=0
  wait_marker "$UART_A" "SELFTEST: remote pkgfs read ok" "SELFTEST: end" "$MARKER_TIMEOUT_REMOTE_SECS" || rc=$?
  if [[ $rc -ne 0 ]]; then
    EVIDENCE["a_remote_pkgfs_fail"]=$(count_marker "$UART_A" "SELFTEST: remote pkgfs read FAIL")
    MISSING_MARKER="SELFTEST: remote pkgfs read ok"
    set_failure "OS2VM_E_REMOTE_PKGFS_FLOW_MISSING" "remote" "A" "remote packagefs final marker missing"
    return 1
  fi
  MARKER_LINES["A_remote_pkgfs_flow"]=$LAST_WAIT_LINE

  rc=0
  wait_marker "$UART_B" "dsoftbusd: remote packagefs served" "SELFTEST: end" "$MARKER_TIMEOUT_REMOTE_SECS" || rc=$?
  if [[ $rc -ne 0 ]]; then
    EVIDENCE["b_remote_pkgfs_served"]=$(count_marker "$UART_B" "dsoftbusd: remote packagefs served")
    MISSING_MARKER="dsoftbusd: remote packagefs served"
    set_failure "OS2VM_E_REMOTE_SERVED_MISSING" "remote" "B" "node B missing remote packagefs served marker"
    return 1
  fi
  MARKER_LINES["B_remote_served"]=$LAST_WAIT_LINE
  # Fake-green guard: if any explicit remote FAIL marker appeared, classify run as failure
  # even when later success markers are present.
  local remote_resolve_fail
  local remote_query_fail
  local remote_statefs_fail
  local remote_pkgfs_fail
  remote_resolve_fail=$(count_marker "$UART_A" "SELFTEST: remote resolve FAIL")
  remote_query_fail=$(count_marker "$UART_A" "SELFTEST: remote query FAIL")
  remote_statefs_fail=$(count_marker "$UART_A" "SELFTEST: remote statefs rw FAIL")
  remote_pkgfs_fail=$(count_marker "$UART_A" "SELFTEST: remote pkgfs read FAIL")
  # #region agent log
  agent_session_log "REMOTE_GUARD" "tools/os2vm.sh:phase_remote" "remote fail-marker guard snapshot" "{\"resolveFail\":$remote_resolve_fail,\"queryFail\":$remote_query_fail,\"statefsFail\":$remote_statefs_fail,\"pkgfsFail\":$remote_pkgfs_fail}"
  # #endregion
  if (( remote_resolve_fail > 0 || remote_query_fail > 0 || remote_statefs_fail > 0 || remote_pkgfs_fail > 0 )); then
    MISSING_MARKER="remote-fail-marker-absent"
    set_failure "OS2VM_E_REMOTE_NEGATIVE_MARKER" "remote" "A" "remote FAIL marker observed in successful marker ladder"
    return 1
  fi
  agent_debug_log "H4" "tools/os2vm.sh:remote_markers_done" "remote proxy markers reached on node A" "{\"status\":\"ok\"}"
  return 0
}

phase_perf() {
  if [[ "$OS2VM_BUDGET_ENABLE" != "1" ]]; then
    echo "[info] Phase-D perf budgets disabled (OS2VM_BUDGET_ENABLE=$OS2VM_BUDGET_ENABLE)."
    return 0
  fi

  local discovery_ms=${PHASE_DURATION_MS["discovery"]:-0}
  local session_ms=${PHASE_DURATION_MS["session"]:-0}
  local mux_ms=${PHASE_DURATION_MS["mux"]:-0}
  local remote_ms=${PHASE_DURATION_MS["remote"]:-0}
  local now_ms_value
  local build_start_ms
  local total_ms
  now_ms_value=$(now_ms)
  build_start_ms=${PHASE_START_MS["build"]:-$now_ms_value}
  total_ms=$(( now_ms_value - build_start_ms ))

  EVIDENCE["perf_discovery_ms"]=$discovery_ms
  EVIDENCE["perf_session_ms"]=$session_ms
  EVIDENCE["perf_mux_ms"]=$mux_ms
  EVIDENCE["perf_remote_ms"]=$remote_ms
  EVIDENCE["perf_total_ms"]=$total_ms

  if (( discovery_ms > OS2VM_BUDGET_DISCOVERY_MS )); then
    set_failure "OS2VM_E_PERF_BUDGET_DISCOVERY" "perf" "both" "discovery_ms=${discovery_ms} exceeds budget=${OS2VM_BUDGET_DISCOVERY_MS}"
    return 1
  fi
  if (( session_ms > OS2VM_BUDGET_SESSION_MS )); then
    set_failure "OS2VM_E_PERF_BUDGET_SESSION" "perf" "both" "session_ms=${session_ms} exceeds budget=${OS2VM_BUDGET_SESSION_MS}"
    return 1
  fi
  if (( mux_ms > OS2VM_BUDGET_MUX_MS )); then
    set_failure "OS2VM_E_PERF_BUDGET_MUX" "perf" "both" "mux_ms=${mux_ms} exceeds budget=${OS2VM_BUDGET_MUX_MS}"
    return 1
  fi
  if (( remote_ms > OS2VM_BUDGET_REMOTE_MS )); then
    set_failure "OS2VM_E_PERF_BUDGET_REMOTE" "perf" "both" "remote_ms=${remote_ms} exceeds budget=${OS2VM_BUDGET_REMOTE_MS}"
    return 1
  fi
  if (( total_ms > OS2VM_BUDGET_TOTAL_MS )); then
    set_failure "OS2VM_E_PERF_BUDGET_TOTAL" "perf" "both" "total_ms=${total_ms} exceeds budget=${OS2VM_BUDGET_TOTAL_MS}"
    return 1
  fi

  return 0
}

phase_soak() {
  if [[ "$OS2VM_SOAK_ENABLE" != "1" ]]; then
    echo "[info] Phase-E soak gate disabled (OS2VM_SOAK_ENABLE=$OS2VM_SOAK_ENABLE)."
    return 0
  fi
  if (( OS2VM_SOAK_DURATION_SECS <= 0 )); then
    echo "[info] Phase-E soak duration is 0s; skipping soak checks."
    return 0
  fi

  local rounds_target=$OS2VM_SOAK_ROUNDS
  local rounds_completed=0
  local round=0
  local round_started=0
  local round_deadline=0
  local next_progress_at=0
  local a_mux_fail_hits=0
  local b_mux_fail_hits=0
  local a_remote_fail_hits=0
  local b_remote_fail_hits=0
  local a_panic_hits=0
  local b_panic_hits=0
  local now_ts
  local remaining

  echo "[info] Running bounded soak stability checks (${OS2VM_SOAK_DURATION_SECS}s x ${rounds_target} round(s))..."
  for (( round=1; round<=rounds_target; round++ )); do
    round_started=$(date +%s)
    round_deadline=$(( round_started + OS2VM_SOAK_DURATION_SECS ))
    next_progress_at=$(( round_started + 5 ))
    while (( $(date +%s) < round_deadline )); do
      if ! is_pid_alive "$PID_A"; then
        set_failure "OS2VM_E_SOAK_NODE_A_ENDED" "soak" "A" "node A terminated during soak window"
        return 1
      fi
      if ! is_pid_alive "$PID_B"; then
        set_failure "OS2VM_E_SOAK_NODE_B_ENDED" "soak" "B" "node B terminated during soak window"
        return 1
      fi

      a_mux_fail_hits=$(count_marker "$UART_A" "dsoftbus:mux crossvm fail")
      b_mux_fail_hits=$(count_marker "$UART_B" "dsoftbus:mux crossvm fail")
      a_remote_fail_hits=$(count_marker "$UART_A" "SELFTEST: remote .* FAIL")
      b_remote_fail_hits=$(count_marker "$UART_B" "SELFTEST: remote .* FAIL")
      a_panic_hits=$(count_marker "$UART_A" "panic")
      b_panic_hits=$(count_marker "$UART_B" "panic")
      if (( a_mux_fail_hits > 0 || b_mux_fail_hits > 0 || a_remote_fail_hits > 0 || b_remote_fail_hits > 0 || a_panic_hits > 0 || b_panic_hits > 0 )); then
        EVIDENCE["soak_a_mux_fail_hits"]=$a_mux_fail_hits
        EVIDENCE["soak_b_mux_fail_hits"]=$b_mux_fail_hits
        EVIDENCE["soak_a_remote_fail_hits"]=$a_remote_fail_hits
        EVIDENCE["soak_b_remote_fail_hits"]=$b_remote_fail_hits
        EVIDENCE["soak_a_panic_hits"]=$a_panic_hits
        EVIDENCE["soak_b_panic_hits"]=$b_panic_hits
        EVIDENCE["soak_rounds_completed"]=$rounds_completed
        EVIDENCE["soak_rounds_target"]=$rounds_target
        MISSING_MARKER="soak-fail-marker-absent"
        set_failure "OS2VM_E_SOAK_FAIL_MARKER" "soak" "both" "fail/panic marker observed during soak window"
        return 1
      fi

      now_ts=$(date +%s)
      if (( now_ts >= next_progress_at )); then
        remaining=$(( round_deadline - now_ts ))
        if (( remaining < 0 )); then
          remaining=0
        fi
        echo "[wait] soak round ${round}/${rounds_target}: remaining=${remaining}s"
        next_progress_at=$(( now_ts + 5 ))
      fi
      sleep 1
    done

    rounds_completed=$round
  done

  EVIDENCE["soak_duration_secs"]=$OS2VM_SOAK_DURATION_SECS
  EVIDENCE["soak_rounds_target"]=$rounds_target
  EVIDENCE["soak_rounds_completed"]=$rounds_completed
  EVIDENCE["soak_a_mux_fail_hits"]=$a_mux_fail_hits
  EVIDENCE["soak_b_mux_fail_hits"]=$b_mux_fail_hits
  EVIDENCE["soak_a_remote_fail_hits"]=$a_remote_fail_hits
  EVIDENCE["soak_b_remote_fail_hits"]=$b_remote_fail_hits
  EVIDENCE["soak_a_panic_hits"]=$a_panic_hits
  EVIDENCE["soak_b_panic_hits"]=$b_panic_hits
  return 0
}

phase_end() {
  echo "[info] All required markers observed."
}

run_phase() {
  local phase=$1
  local fn=$2
  phase_begin "$phase"
  if "$fn"; then
    phase_finish "$phase" "ok"
    return 0
  fi
  phase_finish "$phase" "failed"
  if [[ "$RESULT" == "running" ]]; then
    set_failure "OS2VM_E_UNEXPECTED" "$phase" "host" "phase returned non-zero without classification"
  fi
  return 1
}

collect_qemu_exit() {
  local pid=$1
  if [[ -z "$pid" ]]; then
    echo "not_started"
    return 0
  fi
  if wait "$pid" 2>/dev/null; then
    echo 0
  else
    local rc=$?
    if (( rc == 127 )); then
      # Detached/non-child process (can happen with pipeline/background wrapping).
      if is_pid_alive "$pid"; then
        echo "detached_running"
      else
        echo "detached"
      fi
      return 0
    fi
    echo "$rc"
  fi
}

cleanup_qemu() {
  if (( CLEANUP_DONE == 1 )); then
    return 0
  fi
  CLEANUP_DONE=1
  terminate_pid "$PID_A" 5
  terminate_pid "$PID_B" 5
  terminate_named_qemu "$(qemu_name_for_node A)"
  terminate_named_qemu "$(qemu_name_for_node B)"
  QEMU_EXIT_A=$(collect_qemu_exit "$PID_A")
  QEMU_EXIT_B=$(collect_qemu_exit "$PID_B")
}

run_dir_result() {
  local run_dir=$1
  local result_file="$run_dir/result.txt"
  if [[ -f "$result_file" ]]; then
    tr -d '\n' <"$result_file" 2>/dev/null || echo "unknown"
  else
    echo "unknown"
  fi
}

run_dir_size_bytes() {
  local run_dir=$1
  du -sb "$run_dir" 2>/dev/null | awk '{print $1}' || echo 0
}

gc_run_artifacts() {
  if [[ "$OS2VM_RETENTION_ENABLE" != "1" ]]; then
    return 0
  fi
  if [[ ! -d "$OS2VM_RUNS_DIR" ]]; then
    return 0
  fi
  local keep_success=$OS2VM_RETENTION_KEEP_SUCCESS
  local keep_failure=$OS2VM_RETENTION_KEEP_FAILURE
  local max_age_days=$OS2VM_RETENTION_MAX_AGE_DAYS
  local max_total_bytes=$(( OS2VM_RETENTION_MAX_TOTAL_MB * 1024 * 1024 ))
  local now_secs
  now_secs=$(date +%s)
  local -a runs=()
  local run
  shopt -s nullglob
  runs=( "$OS2VM_RUNS_DIR"/* )
  shopt -u nullglob
  if (( ${#runs[@]} == 0 )); then
    return 0
  fi
  IFS=$'\n' runs=($(ls -1dt "${runs[@]}" 2>/dev/null || true))
  unset IFS

  local success_seen=0
  local failure_seen=0
  local result
  local mtime
  local age_days
  local delete=0
  for run in "${runs[@]}"; do
    [[ -d "$run" ]] || continue
    [[ "$run" == "$LOG_DIR" ]] && continue
    result=$(run_dir_result "$run")
    mtime=$(stat -c %Y "$run" 2>/dev/null || echo 0)
    age_days=$(( (now_secs - mtime) / 86400 ))
    delete=0
    if [[ "$result" == "success" ]]; then
      success_seen=$(( success_seen + 1 ))
      if (( success_seen > keep_success )); then
        delete=1
      fi
    else
      failure_seen=$(( failure_seen + 1 ))
      if (( failure_seen > keep_failure )); then
        delete=1
      fi
    fi
    if (( age_days > max_age_days )); then
      delete=1
    fi
    if (( delete == 1 )); then
      rm -rf "$run"
    fi
  done

  shopt -s nullglob
  runs=( "$OS2VM_RUNS_DIR"/* )
  shopt -u nullglob
  if (( ${#runs[@]} == 0 )); then
    return 0
  fi
  local total_bytes=0
  local size_bytes=0
  for run in "${runs[@]}"; do
    [[ -d "$run" ]] || continue
    size_bytes=$(run_dir_size_bytes "$run")
    total_bytes=$(( total_bytes + size_bytes ))
  done
  if (( total_bytes <= max_total_bytes )); then
    return 0
  fi
  # Size budget enforcement: trim oldest non-current runs first.
  IFS=$'\n' runs=($(ls -1dt "${runs[@]}" 2>/dev/null || true))
  unset IFS
  local idx
  for (( idx=${#runs[@]}-1; idx>=0 && total_bytes>max_total_bytes; idx-- )); do
    run=${runs[$idx]}
    [[ -d "$run" ]] || continue
    [[ "$run" == "$LOG_DIR" ]] && continue
    size_bytes=$(run_dir_size_bytes "$run")
    rm -rf "$run"
    total_bytes=$(( total_bytes - size_bytes ))
  done
}

pcap_count() {
  local pcap=$1
  local filter=$2
  if [[ ! -f "$pcap" ]]; then
    echo -1
    return 0
  fi
  if (( TCPDUMP_AVAILABLE == 0 )); then
    echo -1
    return 0
  fi
  tcpdump -nn -r "$pcap" "$filter" 2>/dev/null | wc -l | tr -d ' '
}

collect_pcap_summary() {
  local node=$1
  local pcap=$2
  local exists=false
  local size=0
  if [[ -f "$pcap" ]]; then
    exists=true
    size=$(wc -c <"$pcap" 2>/dev/null || echo 0)
  fi
  PCAP_STATS["${node}_exists"]=$exists
  PCAP_STATS["${node}_path"]=$pcap
  PCAP_STATS["${node}_size"]=$size
  PCAP_STATS["${node}_arp"]=$(pcap_count "$pcap" "arp")
  PCAP_STATS["${node}_udp_disc"]=$(pcap_count "$pcap" "udp and port 37020")
  PCAP_STATS["${node}_tcp_session"]=$(pcap_count "$pcap" "tcp and (port 34567 or port 34568)")
  PCAP_STATS["${node}_syn"]=$(pcap_count "$pcap" "tcp[tcpflags] & tcp-syn != 0 and tcp[tcpflags] & tcp-ack == 0")
  PCAP_STATS["${node}_synack"]=$(pcap_count "$pcap" "tcp[tcpflags] & (tcp-syn|tcp-ack) == (tcp-syn|tcp-ack)")
  PCAP_STATS["${node}_rst"]=$(pcap_count "$pcap" "tcp[tcpflags] & tcp-rst != 0")
}

nz() {
  local v=${1:-0}
  if [[ -z "$v" || "$v" == "-1" ]]; then
    echo 0
  else
    echo "$v"
  fi
}

refine_failure_by_packets() {
  if [[ "$ERROR_CODE" != "OS2VM_E_SESSION_TIMEOUT" ]]; then
    return 0
  fi
  if (( TCPDUMP_AVAILABLE == 0 )); then
    return 0
  fi
  local syn_total=$(( $(nz "${PCAP_STATS[A_syn]:-0}") + $(nz "${PCAP_STATS[B_syn]:-0}") ))
  local synack_total=$(( $(nz "${PCAP_STATS[A_synack]:-0}") + $(nz "${PCAP_STATS[B_synack]:-0}") ))
  if (( syn_total == 0 )); then
    ERROR_CODE="OS2VM_E_SESSION_NO_SYN"
    apply_error_matrix
  elif (( synack_total == 0 )); then
    ERROR_CODE="OS2VM_E_SESSION_NO_SYNACK"
    apply_error_matrix
  fi
}

collect_marker_lines() {
  MARKER_LINES["A_discovery"]=${MARKER_LINES["A_discovery"]:-$(marker_line "$UART_A" "dsoftbusd: discovery cross-vm up")}
  MARKER_LINES["B_discovery"]=${MARKER_LINES["B_discovery"]:-$(marker_line "$UART_B" "dsoftbusd: discovery cross-vm up")}
  MARKER_LINES["A_session"]=${MARKER_LINES["A_session"]:-$(marker_line "$UART_A" "dsoftbusd: cross-vm session ok")}
  MARKER_LINES["B_session"]=${MARKER_LINES["B_session"]:-$(marker_line "$UART_B" "dsoftbusd: cross-vm session ok")}
  MARKER_LINES["A_mux_session"]=${MARKER_LINES["A_mux_session"]:-$(marker_line "$UART_A" "dsoftbus:mux crossvm session up")}
  MARKER_LINES["B_mux_session"]=${MARKER_LINES["B_mux_session"]:-$(marker_line "$UART_B" "dsoftbus:mux crossvm session up")}
  MARKER_LINES["A_mux_data"]=${MARKER_LINES["A_mux_data"]:-$(marker_line "$UART_A" "dsoftbus:mux crossvm data ok")}
  MARKER_LINES["B_mux_data"]=${MARKER_LINES["B_mux_data"]:-$(marker_line "$UART_B" "dsoftbus:mux crossvm data ok")}
  MARKER_LINES["A_mux_pri"]=${MARKER_LINES["A_mux_pri"]:-$(marker_line "$UART_A" "SELFTEST: mux crossvm pri control ok")}
  MARKER_LINES["B_mux_pri"]=${MARKER_LINES["B_mux_pri"]:-$(marker_line "$UART_B" "SELFTEST: mux crossvm pri control ok")}
  MARKER_LINES["A_mux_bulk"]=${MARKER_LINES["A_mux_bulk"]:-$(marker_line "$UART_A" "SELFTEST: mux crossvm bulk ok")}
  MARKER_LINES["B_mux_bulk"]=${MARKER_LINES["B_mux_bulk"]:-$(marker_line "$UART_B" "SELFTEST: mux crossvm bulk ok")}
  MARKER_LINES["A_mux_backpressure"]=${MARKER_LINES["A_mux_backpressure"]:-$(marker_line "$UART_A" "SELFTEST: mux crossvm backpressure ok")}
  MARKER_LINES["B_mux_backpressure"]=${MARKER_LINES["B_mux_backpressure"]:-$(marker_line "$UART_B" "SELFTEST: mux crossvm backpressure ok")}
  MARKER_LINES["A_remote_resolve"]=${MARKER_LINES["A_remote_resolve"]:-$(marker_line "$UART_A" "SELFTEST: remote resolve ok")}
  MARKER_LINES["A_remote_query"]=${MARKER_LINES["A_remote_query"]:-$(marker_line "$UART_A" "SELFTEST: remote query ok")}
  MARKER_LINES["A_remote_statefs_flow"]=${MARKER_LINES["A_remote_statefs_flow"]:-$(marker_line "$UART_A" "SELFTEST: remote statefs rw ok")}
  MARKER_LINES["A_remote_pkgfs_stat"]=${MARKER_LINES["A_remote_pkgfs_stat"]:-$(marker_line "$UART_A" "SELFTEST: remote pkgfs stat ok")}
  MARKER_LINES["A_remote_pkgfs_open"]=${MARKER_LINES["A_remote_pkgfs_open"]:-$(marker_line "$UART_A" "SELFTEST: remote pkgfs open ok")}
  MARKER_LINES["A_remote_pkgfs_read"]=${MARKER_LINES["A_remote_pkgfs_read"]:-$(marker_line "$UART_A" "SELFTEST: remote pkgfs read step ok")}
  MARKER_LINES["A_remote_pkgfs_close"]=${MARKER_LINES["A_remote_pkgfs_close"]:-$(marker_line "$UART_A" "SELFTEST: remote pkgfs close ok")}
  MARKER_LINES["A_remote_pkgfs_flow"]=${MARKER_LINES["A_remote_pkgfs_flow"]:-$(marker_line "$UART_A" "SELFTEST: remote pkgfs read ok")}
  MARKER_LINES["B_remote_statefs_served"]=${MARKER_LINES["B_remote_statefs_served"]:-$(marker_line "$UART_B" "dsoftbusd: remote statefs served")}
  MARKER_LINES["B_remote_served"]=${MARKER_LINES["B_remote_served"]:-$(marker_line "$UART_B" "dsoftbusd: remote packagefs served")}
}

write_summary_json() {
  local end_ms
  local start_ms
  local total_ms
  end_ms=$(now_ms)
  start_ms=${PHASE_START_MS["build"]:-$end_ms}
  total_ms=$(( end_ms - start_ms ))
  cat >"$OS2VM_SUMMARY_JSON" <<EOF
{
  "runId": "$(json_escape "$AGENT_RUN_ID")",
  "result": "$(json_escape "$RESULT")",
  "phaseTarget": "$(json_escape "$RUN_PHASE")",
  "classification": {
    "errorCode": "$(json_escape "$ERROR_CODE")",
    "phase": "$(json_escape "$FAILED_PHASE")",
    "node": "$(json_escape "$ERROR_NODE")",
    "subsystem": "$(json_escape "$ERROR_SUBSYSTEM")",
    "message": "$(json_escape "$ERROR_MESSAGE")",
    "missingMarker": "$(json_escape "$MISSING_MARKER")",
    "hint": "$(json_escape "$ERROR_HINT")",
    "confidence": "$ERROR_CONFIDENCE"
  },
  "timing": {
    "runTimeoutSecs": $RUN_TIMEOUT_SECS,
    "discoveryTimeoutSecs": $MARKER_TIMEOUT_DISCOVERY_SECS,
    "sessionTimeoutSecs": $MARKER_TIMEOUT_SESSION_SECS,
    "muxTimeoutSecs": $MARKER_TIMEOUT_MUX_SECS,
    "remoteTimeoutSecs": $MARKER_TIMEOUT_REMOTE_SECS,
    "totalDurationMs": $total_ms
  },
  "performanceBudgetsMs": {
    "enabled": $OS2VM_BUDGET_ENABLE,
    "discovery": $OS2VM_BUDGET_DISCOVERY_MS,
    "session": $OS2VM_BUDGET_SESSION_MS,
    "mux": $OS2VM_BUDGET_MUX_MS,
    "remote": $OS2VM_BUDGET_REMOTE_MS,
    "total": $OS2VM_BUDGET_TOTAL_MS
  },
  "performanceObservedMs": {
    "discovery": ${EVIDENCE["perf_discovery_ms"]:-0},
    "session": ${EVIDENCE["perf_session_ms"]:-0},
    "mux": ${EVIDENCE["perf_mux_ms"]:-0},
    "remote": ${EVIDENCE["perf_remote_ms"]:-0},
    "total": ${EVIDENCE["perf_total_ms"]:-0}
  },
  "soakGate": {
    "enabled": $OS2VM_SOAK_ENABLE,
    "durationSecs": $OS2VM_SOAK_DURATION_SECS,
    "rounds": $OS2VM_SOAK_ROUNDS,
    "observed": {
      "durationSecs": ${EVIDENCE["soak_duration_secs"]:-0},
      "roundsTarget": ${EVIDENCE["soak_rounds_target"]:-0},
      "roundsCompleted": ${EVIDENCE["soak_rounds_completed"]:-0},
      "aMuxFailHits": ${EVIDENCE["soak_a_mux_fail_hits"]:-0},
      "bMuxFailHits": ${EVIDENCE["soak_b_mux_fail_hits"]:-0},
      "aRemoteFailHits": ${EVIDENCE["soak_a_remote_fail_hits"]:-0},
      "bRemoteFailHits": ${EVIDENCE["soak_b_remote_fail_hits"]:-0},
      "aPanicHits": ${EVIDENCE["soak_a_panic_hits"]:-0},
      "bPanicHits": ${EVIDENCE["soak_b_panic_hits"]:-0}
    }
  },
  "paths": {
    "runDir": "$(json_escape "$LOG_DIR")",
    "uartA": "$(json_escape "$UART_A")",
    "uartB": "$(json_escape "$UART_B")",
    "hostA": "$(json_escape "$HOST_A")",
    "hostB": "$(json_escape "$HOST_B")",
    "summaryTxt": "$(json_escape "$OS2VM_SUMMARY_TXT")",
    "summaryJson": "$(json_escape "$OS2VM_SUMMARY_JSON")",
    "releaseBundleJson": "$(json_escape "$OS2VM_RELEASE_BUNDLE_JSON")"
  },
  "phaseDurationsMs": {
    "build": ${PHASE_DURATION_MS["build"]:-0},
    "launch": ${PHASE_DURATION_MS["launch"]:-0},
    "discovery": ${PHASE_DURATION_MS["discovery"]:-0},
    "session": ${PHASE_DURATION_MS["session"]:-0},
    "mux": ${PHASE_DURATION_MS["mux"]:-0},
    "remote": ${PHASE_DURATION_MS["remote"]:-0},
    "perf": ${PHASE_DURATION_MS["perf"]:-0},
    "soak": ${PHASE_DURATION_MS["soak"]:-0},
    "end": ${PHASE_DURATION_MS["end"]:-0}
  },
  "markerLines": {
    "A_discovery": ${MARKER_LINES["A_discovery"]:-0},
    "B_discovery": ${MARKER_LINES["B_discovery"]:-0},
    "A_session": ${MARKER_LINES["A_session"]:-0},
    "B_session": ${MARKER_LINES["B_session"]:-0},
    "A_mux_session": ${MARKER_LINES["A_mux_session"]:-0},
    "B_mux_session": ${MARKER_LINES["B_mux_session"]:-0},
    "A_mux_data": ${MARKER_LINES["A_mux_data"]:-0},
    "B_mux_data": ${MARKER_LINES["B_mux_data"]:-0},
    "A_mux_pri": ${MARKER_LINES["A_mux_pri"]:-0},
    "B_mux_pri": ${MARKER_LINES["B_mux_pri"]:-0},
    "A_mux_bulk": ${MARKER_LINES["A_mux_bulk"]:-0},
    "B_mux_bulk": ${MARKER_LINES["B_mux_bulk"]:-0},
    "A_mux_backpressure": ${MARKER_LINES["A_mux_backpressure"]:-0},
    "B_mux_backpressure": ${MARKER_LINES["B_mux_backpressure"]:-0},
    "A_remote_resolve": ${MARKER_LINES["A_remote_resolve"]:-0},
    "A_remote_query": ${MARKER_LINES["A_remote_query"]:-0},
    "A_remote_statefs_flow": ${MARKER_LINES["A_remote_statefs_flow"]:-0},
    "A_remote_pkgfs_stat": ${MARKER_LINES["A_remote_pkgfs_stat"]:-0},
    "A_remote_pkgfs_open": ${MARKER_LINES["A_remote_pkgfs_open"]:-0},
    "A_remote_pkgfs_read": ${MARKER_LINES["A_remote_pkgfs_read"]:-0},
    "A_remote_pkgfs_close": ${MARKER_LINES["A_remote_pkgfs_close"]:-0},
    "A_remote_pkgfs_flow": ${MARKER_LINES["A_remote_pkgfs_flow"]:-0},
    "B_remote_statefs_served": ${MARKER_LINES["B_remote_statefs_served"]:-0},
    "B_remote_served": ${MARKER_LINES["B_remote_served"]:-0}
  },
  "qemu": {
    "pidA": "$(json_escape "$PID_A")",
    "pidB": "$(json_escape "$PID_B")",
    "exitA": "$(json_escape "$QEMU_EXIT_A")",
    "exitB": "$(json_escape "$QEMU_EXIT_B")"
  },
  "pcap": {
    "mode": "$(json_escape "$OS2VM_PCAP_MODE")",
    "tcpdumpAvailable": $TCPDUMP_AVAILABLE,
    "A": {
      "path": "$(json_escape "${PCAP_STATS[A_path]:-$PCAP_A}")",
      "exists": ${PCAP_STATS[A_exists]:-false},
      "sizeBytes": ${PCAP_STATS[A_size]:-0},
      "arp": ${PCAP_STATS[A_arp]:--1},
      "udpDiscovery": ${PCAP_STATS[A_udp_disc]:--1},
      "tcpSession": ${PCAP_STATS[A_tcp_session]:--1},
      "syn": ${PCAP_STATS[A_syn]:--1},
      "synAck": ${PCAP_STATS[A_synack]:--1},
      "rst": ${PCAP_STATS[A_rst]:--1}
    },
    "B": {
      "path": "$(json_escape "${PCAP_STATS[B_path]:-$PCAP_B}")",
      "exists": ${PCAP_STATS[B_exists]:-false},
      "sizeBytes": ${PCAP_STATS[B_size]:-0},
      "arp": ${PCAP_STATS[B_arp]:--1},
      "udpDiscovery": ${PCAP_STATS[B_udp_disc]:--1},
      "tcpSession": ${PCAP_STATS[B_tcp_session]:--1},
      "syn": ${PCAP_STATS[B_syn]:--1},
      "synAck": ${PCAP_STATS[B_synack]:--1},
      "rst": ${PCAP_STATS[B_rst]:--1}
    }
  }
}
EOF
}

write_summary_txt() {
  cat >"$OS2VM_SUMMARY_TXT" <<EOF
runId: $AGENT_RUN_ID
profile: $OS2VM_PROFILE
result: $RESULT
phaseTarget: $RUN_PHASE
failureCode: ${ERROR_CODE:-none}
failurePhase: ${FAILED_PHASE:-none}
failureNode: ${ERROR_NODE:-none}
missingMarker: ${MISSING_MARKER:-none}
subsystem: ${ERROR_SUBSYSTEM:-none}
hint: ${ERROR_HINT:-none}
qemuExitA: ${QEMU_EXIT_A}
qemuExitB: ${QEMU_EXIT_B}
pcapMode: $OS2VM_PCAP_MODE
perfBudgetEnabled: $OS2VM_BUDGET_ENABLE
perfBudgetDiscoveryMs: $OS2VM_BUDGET_DISCOVERY_MS
perfBudgetSessionMs: $OS2VM_BUDGET_SESSION_MS
perfBudgetMuxMs: $OS2VM_BUDGET_MUX_MS
perfBudgetRemoteMs: $OS2VM_BUDGET_REMOTE_MS
perfBudgetTotalMs: $OS2VM_BUDGET_TOTAL_MS
perfObservedDiscoveryMs: ${EVIDENCE["perf_discovery_ms"]:-0}
perfObservedSessionMs: ${EVIDENCE["perf_session_ms"]:-0}
perfObservedMuxMs: ${EVIDENCE["perf_mux_ms"]:-0}
perfObservedRemoteMs: ${EVIDENCE["perf_remote_ms"]:-0}
perfObservedTotalMs: ${EVIDENCE["perf_total_ms"]:-0}
soakEnabled: $OS2VM_SOAK_ENABLE
soakDurationSecs: $OS2VM_SOAK_DURATION_SECS
soakRounds: $OS2VM_SOAK_ROUNDS
soakObservedDurationSecs: ${EVIDENCE["soak_duration_secs"]:-0}
soakObservedRoundsTarget: ${EVIDENCE["soak_rounds_target"]:-0}
soakObservedRoundsCompleted: ${EVIDENCE["soak_rounds_completed"]:-0}
soakObservedMuxFailHitsA: ${EVIDENCE["soak_a_mux_fail_hits"]:-0}
soakObservedMuxFailHitsB: ${EVIDENCE["soak_b_mux_fail_hits"]:-0}
soakObservedRemoteFailHitsA: ${EVIDENCE["soak_a_remote_fail_hits"]:-0}
soakObservedRemoteFailHitsB: ${EVIDENCE["soak_b_remote_fail_hits"]:-0}
soakObservedPanicHitsA: ${EVIDENCE["soak_a_panic_hits"]:-0}
soakObservedPanicHitsB: ${EVIDENCE["soak_b_panic_hits"]:-0}
runDir: $LOG_DIR
summaryJson: $OS2VM_SUMMARY_JSON
releaseBundleJson: $OS2VM_RELEASE_BUNDLE_JSON
EOF
}

write_release_bundle_json() {
  cat >"$OS2VM_RELEASE_BUNDLE_JSON" <<EOF
{
  "runId": "$(json_escape "$AGENT_RUN_ID")",
  "generatedAtEpochMs": $(now_ms),
  "result": "$(json_escape "$RESULT")",
  "commands": {
    "distributed": "RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh",
    "singleVm": "REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s ./scripts/qemu-test.sh",
    "regression": "just test-e2e && just test-os-dhcp"
  },
  "gates": {
    "muxLadder": {
      "passed": $( [[ "${MARKER_LINES["A_mux_backpressure"]:-0}" -gt 0 && "${MARKER_LINES["B_mux_backpressure"]:-0}" -gt 0 ]] && echo "true" || echo "false" ),
      "aBackpressureLine": ${MARKER_LINES["A_mux_backpressure"]:-0},
      "bBackpressureLine": ${MARKER_LINES["B_mux_backpressure"]:-0}
    },
    "perf": {
      "enabled": $OS2VM_BUDGET_ENABLE,
      "passed": $( [[ "$RESULT" == "success" ]] && echo "true" || echo "false" ),
      "observedMs": {
        "discovery": ${EVIDENCE["perf_discovery_ms"]:-0},
        "session": ${EVIDENCE["perf_session_ms"]:-0},
        "mux": ${EVIDENCE["perf_mux_ms"]:-0},
        "remote": ${EVIDENCE["perf_remote_ms"]:-0},
        "total": ${EVIDENCE["perf_total_ms"]:-0}
      }
    },
    "soak": {
      "enabled": $OS2VM_SOAK_ENABLE,
      "durationSecs": $OS2VM_SOAK_DURATION_SECS,
      "rounds": $OS2VM_SOAK_ROUNDS,
      "observedRoundsCompleted": ${EVIDENCE["soak_rounds_completed"]:-0},
      "failHits": {
        "aMux": ${EVIDENCE["soak_a_mux_fail_hits"]:-0},
        "bMux": ${EVIDENCE["soak_b_mux_fail_hits"]:-0},
        "aRemote": ${EVIDENCE["soak_a_remote_fail_hits"]:-0},
        "bRemote": ${EVIDENCE["soak_b_remote_fail_hits"]:-0},
        "aPanic": ${EVIDENCE["soak_a_panic_hits"]:-0},
        "bPanic": ${EVIDENCE["soak_b_panic_hits"]:-0}
      }
    }
  },
  "artifacts": {
    "summaryJson": "$(json_escape "$OS2VM_SUMMARY_JSON")",
    "summaryTxt": "$(json_escape "$OS2VM_SUMMARY_TXT")",
    "uartA": "$(json_escape "$UART_A")",
    "uartB": "$(json_escape "$UART_B")",
    "hostA": "$(json_escape "$HOST_A")",
    "hostB": "$(json_escape "$HOST_B")"
  }
}
EOF
}

on_exit() {
  local shell_exit=$1
  set +e
  if [[ "$RESULT" == "running" && "$shell_exit" -ne 0 ]]; then
    set_failure "OS2VM_E_UNEXPECTED" "unknown" "host" "unexpected shell failure before classified result"
  fi
  cleanup_qemu
  collect_marker_lines
  collect_pcap_summary "A" "$PCAP_A"
  collect_pcap_summary "B" "$PCAP_B"
  refine_failure_by_packets
  if [[ "$RESULT" == "running" ]]; then
    RESULT="success"
  fi
  printf '%s\n' "$RESULT" >"$OS2VM_RESULT_FILE"
  FINAL_EXIT_CODE=$(resolve_exit_code)
  write_summary_json
  write_summary_txt
  write_release_bundle_json
  if [[ "$OS2VM_PCAP_MODE" == "auto" && "$RESULT" == "success" ]]; then
    rm -f "$PCAP_A" "$PCAP_B"
  fi
  gc_run_artifacts
  agent_session_log "SUM" "tools/os2vm.sh:summary" "os2vm run summary" "{\"result\":\"$RESULT\",\"errorCode\":\"$ERROR_CODE\",\"phase\":\"$FAILED_PHASE\",\"summaryJson\":\"$OS2VM_SUMMARY_JSON\"}"
  if [[ "$OS2VM_SUMMARY_STDOUT" == "1" ]]; then
    echo "[summary] result=$RESULT code=${ERROR_CODE:-none} phase=${FAILED_PHASE:-none} node=${ERROR_NODE:-none} summary=$OS2VM_SUMMARY_JSON"
  fi
}

trap 'on_exit $?' EXIT

if phase_should_run "build"; then
  if ! run_phase "build" phase_build; then
    FINAL_EXIT_CODE=$(resolve_exit_code)
    exit "$FINAL_EXIT_CODE"
  fi
fi
if phase_should_run "launch"; then
  if ! run_phase "launch" phase_launch; then
    FINAL_EXIT_CODE=$(resolve_exit_code)
    exit "$FINAL_EXIT_CODE"
  fi
fi
if phase_should_run "discovery"; then
  if ! run_phase "discovery" phase_discovery; then
    FINAL_EXIT_CODE=$(resolve_exit_code)
    exit "$FINAL_EXIT_CODE"
  fi
fi
if phase_should_run "session"; then
  if ! run_phase "session" phase_session; then
    FINAL_EXIT_CODE=$(resolve_exit_code)
    exit "$FINAL_EXIT_CODE"
  fi
fi
if phase_should_run "mux"; then
  if ! run_phase "mux" phase_mux; then
    FINAL_EXIT_CODE=$(resolve_exit_code)
    exit "$FINAL_EXIT_CODE"
  fi
fi
if phase_should_run "remote"; then
  if ! run_phase "remote" phase_remote; then
    FINAL_EXIT_CODE=$(resolve_exit_code)
    exit "$FINAL_EXIT_CODE"
  fi
fi
if phase_should_run "perf"; then
  if ! run_phase "perf" phase_perf; then
    FINAL_EXIT_CODE=$(resolve_exit_code)
    exit "$FINAL_EXIT_CODE"
  fi
fi
if phase_should_run "soak"; then
  if ! run_phase "soak" phase_soak; then
    FINAL_EXIT_CODE=$(resolve_exit_code)
    exit "$FINAL_EXIT_CODE"
  fi
fi
if phase_should_run "end"; then
  if ! run_phase "end" phase_end; then
    FINAL_EXIT_CODE=$(resolve_exit_code)
    exit "$FINAL_EXIT_CODE"
  fi
fi

if [[ "$RESULT" == "running" ]]; then
  RESULT="success"
fi
if [[ "$RUN_PHASE" != "end" ]]; then
  echo "[info] RUN_PHASE target '$RUN_PHASE' reached."
fi
FINAL_EXIT_CODE=$(resolve_exit_code)
exit "$FINAL_EXIT_CODE"

: <<'OS2VM_LEGACY_DISABLED'
echo "[info] Building OS artifacts once..."
build_os_once
#region agent log
netstackd_elf="$BUILD_TARGET_DIR/$TARGET/release/netstackd"
netstackd_exists=false
if [[ -f "$netstackd_elf" ]]; then
  netstackd_exists=true
fi
agent_session_log "A" "tools/os2vm.sh:build_artifacts" "verify os2vm build artifact paths" "{\"buildTargetDir\":\"$BUILD_TARGET_DIR\",\"target\":\"$TARGET\",\"netstackdElf\":\"$netstackd_elf\",\"netstackdExists\":$netstackd_exists}"
#endregion

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
#region agent log
agent_session_log "D" "tools/os2vm.sh:launch" "verify socket backend launch parameters" "{\"netdevA\":\"$NETDEV_A\",\"netdevB\":\"$NETDEV_B\",\"pidA\":$PID_A,\"pidB\":$PID_B}"
#endregion
#region agent log
agent_session_log "F" "tools/os2vm.sh:launch" "verify modern virtio-mmio launch setting" "{\"qemuArg\":\"-global virtio-mmio.force-legacy=off\",\"enabled\":true}"
#endregion

cleanup() {
  kill "$PID_A" "$PID_B" 2>/dev/null || true
}
trap cleanup EXIT

echo "[info] Waiting for cross-VM discovery markers..."
rc=0
wait_dual_markers "$UART_A" "dsoftbusd: discovery cross-vm up" "$UART_B" "dsoftbusd: discovery cross-vm up" "SELFTEST: end" || rc=$?
if [[ $rc -ne 0 ]]; then
  #region agent log
  a_fallback_100215=$(grep -c "fallback static 10.0.2.15/24" "$UART_A" || true)
  b_fallback_100215=$(grep -c "fallback static 10.0.2.15/24" "$UART_B" || true)
  a_fallback_1042=$(grep -c "fallback static 10.42." "$UART_A" || true)
  b_fallback_1042=$(grep -c "fallback static 10.42." "$UART_B" || true)
  a_listen_loopback=$(grep -c "dbg:netstackd: listen mode loopback" "$UART_A" || true)
  b_listen_loopback=$(grep -c "dbg:netstackd: listen mode loopback" "$UART_B" || true)
  a_listen_tcp=$(grep -c "dbg:netstackd: listen mode tcp" "$UART_A" || true)
  b_listen_tcp=$(grep -c "dbg:netstackd: listen mode tcp" "$UART_B" || true)
  agent_session_log "B" "tools/os2vm.sh:discovery_wait" "check whether qemu-smoke fallback path is active" "{\"nodeA\":{\"fallback100215\":$a_fallback_100215,\"fallback1042\":$a_fallback_1042,\"listenLoopback\":$a_listen_loopback,\"listenTcp\":$a_listen_tcp},\"nodeB\":{\"fallback100215\":$b_fallback_100215,\"fallback1042\":$b_fallback_1042,\"listenLoopback\":$b_listen_loopback,\"listenTcp\":$b_listen_tcp}}"
  #endregion
  #region agent log
  a_cross_vm_up=$(grep -c "dsoftbusd: discovery cross-vm up" "$UART_A" || true)
  b_cross_vm_up=$(grep -c "dsoftbusd: discovery cross-vm up" "$UART_B" || true)
  a_loopback_up=$(grep -c "dsoftbusd: discovery up (udp loopback)" "$UART_A" || true)
  b_loopback_up=$(grep -c "dsoftbusd: discovery up (udp loopback)" "$UART_B" || true)
  a_local_ip_ok=$(grep -c "dsoftbusd: local ip ok" "$UART_A" || true)
  b_local_ip_ok=$(grep -c "dsoftbusd: local ip ok" "$UART_B" || true)
  agent_session_log "C" "tools/os2vm.sh:discovery_wait" "check local-ip branch selection before discovery mode" "{\"nodeA\":{\"crossVmUp\":$a_cross_vm_up,\"loopbackUp\":$a_loopback_up,\"localIpOk\":$a_local_ip_ok},\"nodeB\":{\"crossVmUp\":$b_cross_vm_up,\"loopbackUp\":$b_loopback_up,\"localIpOk\":$b_local_ip_ok}}"
  #endregion
  #region agent log
  a_selftest_end=$(grep -c "SELFTEST: end" "$UART_A" || true)
  b_selftest_end=$(grep -c "SELFTEST: end" "$UART_B" || true)
  agent_session_log "E" "tools/os2vm.sh:discovery_wait" "check early selftest termination relative to discovery expectation" "{\"waitReturnCode\":$rc,\"nodeA\":{\"selftestEnd\":$a_selftest_end},\"nodeB\":{\"selftestEnd\":$b_selftest_end}}"
  #endregion
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
wait_dual_markers "$UART_A" "dsoftbusd: cross-vm session ok" "$UART_B" "dsoftbusd: cross-vm session ok" || rc=$?
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
rc=0
wait_marker "$UART_A" "SELFTEST: remote resolve ok" "SELFTEST: end" || rc=$?
if [[ $rc -ne 0 ]]; then
  #region agent log
  a_remote_resolve_fail=$(grep -c "SELFTEST: remote resolve FAIL" "$UART_A" || true)
  a_remote_query_fail=$(grep -c "SELFTEST: remote query FAIL" "$UART_A" || true)
  a_remote_pkgfs_fail=$(grep -c "SELFTEST: remote pkgfs read FAIL" "$UART_A" || true)
  a_remote_rpc_fail_resolve=$(grep -c "dbg:dsoftbusd: remote rpc fail resolve" "$UART_A" || true)
  a_remote_rpc_fail_bundle_list=$(grep -c "dbg:dsoftbusd: remote rpc fail bundle-list" "$UART_A" || true)
  b_dep_samgrd_ok=$(grep -c "dbg:dsoftbusd: remote proxy dep samgrd ok" "$UART_B" || true)
  b_dep_samgrd_fail=$(grep -c "dbg:dsoftbusd: remote proxy dep samgrd fail" "$UART_B" || true)
  b_dep_bundle_ok=$(grep -c "dbg:dsoftbusd: remote proxy dep bundlemgrd ok" "$UART_B" || true)
  b_dep_bundle_fail=$(grep -c "dbg:dsoftbusd: remote proxy dep bundlemgrd fail" "$UART_B" || true)
  b_dep_pkgfs_ok=$(grep -c "dbg:dsoftbusd: remote proxy dep packagefsd ok" "$UART_B" || true)
  b_dep_pkgfs_fail=$(grep -c "dbg:dsoftbusd: remote proxy dep packagefsd fail" "$UART_B" || true)
  b_remote_proxy_up=$(grep -c "dsoftbusd: remote proxy up" "$UART_B" || true)
  b_remote_proxy_rx=$(grep -c "dsoftbusd: remote proxy rx" "$UART_B" || true)
  agent_session_log "G" "tools/os2vm.sh:remote_wait" "remote resolve marker failure diagnostics" "{\"waitReturnCode\":$rc,\"nodeA\":{\"remoteResolveFail\":$a_remote_resolve_fail,\"remoteQueryFail\":$a_remote_query_fail,\"remotePkgfsFail\":$a_remote_pkgfs_fail,\"remoteRpcFailResolve\":$a_remote_rpc_fail_resolve,\"remoteRpcFailBundleList\":$a_remote_rpc_fail_bundle_list},\"nodeB\":{\"depSamgrdOk\":$b_dep_samgrd_ok,\"depSamgrdFail\":$b_dep_samgrd_fail,\"depBundleOk\":$b_dep_bundle_ok,\"depBundleFail\":$b_dep_bundle_fail,\"depPkgfsOk\":$b_dep_pkgfs_ok,\"depPkgfsFail\":$b_dep_pkgfs_fail,\"remoteProxyUp\":$b_remote_proxy_up,\"remoteProxyRx\":$b_remote_proxy_rx}}"
  #endregion
  echo "[error] Node A missing remote resolve marker"
  exit 1
fi
wait_marker "$UART_A" "SELFTEST: remote query ok" "SELFTEST: end" || { echo "[error] Node A missing remote query marker"; exit 1; }
rc=0
wait_marker "$UART_A" "SELFTEST: remote pkgfs read ok" "SELFTEST: end" || rc=$?
if [[ $rc -ne 0 ]]; then
  #region agent log
  a_pkgfs_fail=$(grep -c "SELFTEST: remote pkgfs read FAIL" "$UART_A" || true)
  a_rpc_fail_pkgfs_stat_open=$(grep -c "dbg:dsoftbusd: remote rpc fail pkgfs-stat-open" "$UART_A" || true)
  a_rpc_fail_pkgfs_read=$(grep -c "dbg:dsoftbusd: remote rpc fail pkgfs-read" "$UART_A" || true)
  a_rpc_fail_pkgfs_close=$(grep -c "dbg:dsoftbusd: remote rpc fail pkgfs-close" "$UART_A" || true)
  b_dep_pkgfs_ok=$(grep -c "dbg:dsoftbusd: remote proxy dep packagefsd ok" "$UART_B" || true)
  b_dep_pkgfs_fail=$(grep -c "dbg:dsoftbusd: remote proxy dep packagefsd fail" "$UART_B" || true)
  b_client_new_ok=$(grep -c "dbg:dsoftbusd: packagefsd client new_for ok" "$UART_B" || true)
  b_client_new_fail=$(grep -c "dbg:dsoftbusd: packagefsd client new_for fail" "$UART_B" || true)
  b_client_bounded_fail=$(grep -c "dbg:dsoftbusd: packagefsd client bounded fail" "$UART_B" || true)
  b_remote_pkgfs_served=$(grep -c "dsoftbusd: remote packagefs served" "$UART_B" || true)
  agent_session_log "H" "tools/os2vm.sh:remote_wait" "remote pkgfs marker failure diagnostics" "{\"waitReturnCode\":$rc,\"nodeA\":{\"pkgfsFail\":$a_pkgfs_fail,\"rpcFailPkgfsStatOpen\":$a_rpc_fail_pkgfs_stat_open,\"rpcFailPkgfsRead\":$a_rpc_fail_pkgfs_read,\"rpcFailPkgfsClose\":$a_rpc_fail_pkgfs_close},\"nodeB\":{\"depPkgfsOk\":$b_dep_pkgfs_ok,\"depPkgfsFail\":$b_dep_pkgfs_fail,\"clientNewForOk\":$b_client_new_ok,\"clientNewForFail\":$b_client_new_fail,\"clientBoundedFail\":$b_client_bounded_fail,\"remotePkgfsServed\":$b_remote_pkgfs_served}}"
  #endregion
  echo "[error] Node A missing remote pkgfs marker"
  exit 1
fi
wait_marker "$UART_B" "dsoftbusd: remote packagefs served" "SELFTEST: end" || { echo "[error] Node B missing remote packagefs served marker"; exit 1; }
#region agent log
agent_debug_log "H4" "tools/os2vm.sh:remote_markers_done" "remote proxy markers reached on node A" "{\"status\":\"ok\"}"
#endregion

echo "[info] All required markers observed. Stopping VMs."
cleanup
exit 0
OS2VM_LEGACY_DISABLED
