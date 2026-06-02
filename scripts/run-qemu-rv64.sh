#!/usr/bin/env bash
# Copyright 2024 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0

# Environment knobs:
#   RUN_TIMEOUT      – timeout(1) duration before QEMU is terminated (`0` disables timeout)
#   RUN_UNTIL_MARKER – when "1", stop QEMU once a success UART marker is printed (default: 0)
#   QEMU_SESSION_MODE – `proof` or `interactive` (default: proof)
#   QEMU_MARKER_LEVEL – `proof`, `minimal`, or `full` marker posture
#   NEXUS_SELFTEST_MODE – guest runtime mode override passed through QEMU `fw_cfg`
#   NEXUS_SELFTEST_PROFILE – guest runtime profile override passed through QEMU `fw_cfg`
#   QEMU_LOG_MAX     – maximum size of qemu.log after trimming (default: 52428800 bytes)
#   UART_LOG_MAX     – maximum size of uart.log after trimming (default: 10485760 bytes)
#   QEMU_LOG / UART_LOG – override log file paths.
#   INIT_LITE_LOG_TOPICS – comma separated init-lite log topic list (e.g. "svc-meta") propagated to the build script.
#   NEXUS_DISPLAY_BOOTSTRAP – when "1", boot the visible ramfb scanout bootstrap path.
#   QEMU_DISPLAY_BACKEND – QEMU display backend for the visible bootstrap (default: gtk).
#   SANDBOX_CACHE_GC – sandbox cache GC mode: auto|on|off (default: auto)
#   SANDBOX_CACHE_DIR – sandbox cache root (default: /tmp/cursor-sandbox-cache)
#   SANDBOX_CACHE_MAX_MB – trigger GC when cache grows above this (default: 1024)
#   SANDBOX_CACHE_TARGET_FREE_MB – trigger/continue GC when /tmp free is below this (default: 1024)
#   SANDBOX_CACHE_MIN_AGE_SECS – in auto mode, keep newer entries than this age (default: 1800)
#   BUILD_TMPDIR_DEFAULT – TMPDIR fallback for Rust builds (default: $ROOT/.tmp/build)
#   BUILD_TMP_MIN_FREE_MB – minimum free space required in TMPDIR before fallback (default: 256)

set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
TARGET=${TARGET:-riscv64imac-unknown-none-elf}
# Keep cargo outputs and ELF lookup in the same target root.
# Default to workspace-local target to avoid tmp/cache pressure from inherited environments.
# Set NEXUS_FORCE_WORKSPACE_TARGET=0 to honor incoming CARGO_TARGET_DIR.
NEXUS_FORCE_WORKSPACE_TARGET=${NEXUS_FORCE_WORKSPACE_TARGET:-1}
if [[ "$NEXUS_FORCE_WORKSPACE_TARGET" == "1" ]]; then
  TARGET_ROOT="$ROOT/target"
else
  TARGET_ROOT=${CARGO_TARGET_DIR:-"$ROOT/target"}
fi
export CARGO_TARGET_DIR="$TARGET_ROOT"
KERNEL_ELF=$TARGET_ROOT/$TARGET/release/neuron-boot
KERNEL_BIN=$TARGET_ROOT/$TARGET/release/neuron-boot.bin
INIT_ELF=$TARGET_ROOT/$TARGET/release/init-lite
RUSTFLAGS_OS=${RUSTFLAGS_OS:---check-cfg=cfg(nexus_env,values(\"host\",\"os\")) --cfg nexus_env=\"os\"}
export RUSTFLAGS="$RUSTFLAGS_OS"
RUN_UNTIL_MARKER=${RUN_UNTIL_MARKER:-0}
QEMU_TIMEOUT_SIGNAL=${QEMU_TIMEOUT_SIGNAL:-TERM}
QEMU_LOG_MAX=${QEMU_LOG_MAX:-52428800}
UART_LOG_MAX=${UART_LOG_MAX:-10485760}
# Log directory: qemu-test.sh sets LOG_DIR per run; fallback for manual runs.
LOG_DIR=${LOG_DIR:-$ROOT/build/logs/manual--$(date +%Y-%m-%dT%H-%M-%S)}
mkdir -p "$LOG_DIR"
QEMU_LOG=${QEMU_LOG:-$LOG_DIR/qemu.stderr}
UART_LOG=${UART_LOG:-$LOG_DIR/uart.log}
BUILD_LOG=${BUILD_LOG:-$LOG_DIR/build.stderr}
INTERACTIVE_READY_SENTINEL=${INTERACTIVE_READY_SENTINEL:-$ROOT/build/.interactive-scene-ready}
NEURON_BOOT_FEATURES=${NEURON_BOOT_FEATURES:-}
# Allow overriding the QEMU net backend (default: usernet) for opt-in harnesses.
QEMU_NETDEV=${QEMU_NETDEV:--netdev user,id=n0}
QEMU_NETDEV_DEVICE=${QEMU_NETDEV_DEVICE:--device virtio-net-device,netdev=n0}
QEMU_RNG_OBJECT=${QEMU_RNG_OBJECT:--object rng-random,id=rng0,filename=/dev/urandom}
QEMU_RNG_DEVICE=${QEMU_RNG_DEVICE:--device virtio-rng-device,rng=rng0}
QEMU_GPU_DEVICE=${QEMU_GPU_DEVICE:--device virtio-gpu-device}
QEMU_SESSION_MODE=${QEMU_SESSION_MODE:-proof}
QEMU_MARKER_LEVEL=${QEMU_MARKER_LEVEL:-}
QEMU_INPUT_AUTOINJECT=${QEMU_INPUT_AUTOINJECT:-0}
QEMU_QMP_SOCKET=${QEMU_QMP_SOCKET:-$ROOT/build/qemu.qmp}
QEMU_INPUT_INJECTOR_PY=${QEMU_INPUT_INJECTOR_PY:-$ROOT/tools/qmp_visible_input_inject.py}
QEMU_PROFILE_INPUT_HELPER=${QEMU_PROFILE_INPUT_HELPER:-$ROOT/tools/systemui_profile_qemu_devices.py}
NEXUS_SYSTEMUI_PROFILE=${NEXUS_SYSTEMUI_PROFILE:-desktop}
QEMU_PROOF_POINTER_SOURCE=${QEMU_PROOF_POINTER_SOURCE:-}
NEXUS_PROFILE_INPUT_TOUCH=${NEXUS_PROFILE_INPUT_TOUCH:-0}
NEXUS_PROFILE_INPUT_MOUSE=${NEXUS_PROFILE_INPUT_MOUSE:-0}
NEXUS_PROFILE_INPUT_KBD=${NEXUS_PROFILE_INPUT_KBD:-0}
NEXUS_PROFILE_INPUT_REMOTE=${NEXUS_PROFILE_INPUT_REMOTE:-0}
NEXUS_PROFILE_INPUT_ROTARY=${NEXUS_PROFILE_INPUT_ROTARY:-0}
if [[ -z "$QEMU_MARKER_LEVEL" ]]; then
  if [[ "$QEMU_SESSION_MODE" == "interactive" ]]; then
    QEMU_MARKER_LEVEL=minimal
  else
    QEMU_MARKER_LEVEL=proof
  fi
fi
if [[ -z "${NEXUS_SELFTEST_MODE:-}" ]]; then
  case "$QEMU_SESSION_MODE:$QEMU_MARKER_LEVEL" in
    proof:proof)
      if [[ "${NEXUS_DISPLAY_BOOTSTRAP:-0}" == "1" ]]; then
        NEXUS_SELFTEST_MODE=proof
      fi
      ;;
    interactive:minimal) NEXUS_SELFTEST_MODE=interactive-minimal ;;
    interactive:full) NEXUS_SELFTEST_MODE=interactive-full ;;
  esac
fi
if [[ -n "${NEXUS_SELFTEST_MODE:-}" && -z "${NEXUS_DISPLAY_BOOTSTRAP:-}" ]]; then
  NEXUS_DISPLAY_BOOTSTRAP=1
fi
NEXUS_DISPLAY_BOOTSTRAP=${NEXUS_DISPLAY_BOOTSTRAP:-0}
if [[ -z "${RUN_TIMEOUT:-}" ]]; then
  if [[ "$QEMU_SESSION_MODE" == "interactive" ]]; then
    RUN_TIMEOUT=0
  else
    RUN_TIMEOUT=90s
  fi
fi
QEMU_DISPLAY_BACKEND=${QEMU_DISPLAY_BACKEND:-gtk}
RESOLVED_QEMU_DISPLAY_BACKEND=${QEMU_DISPLAY_BACKEND}
QEMU_BLK_IMG=${QEMU_BLK_IMG:-$ROOT/build/blk.img}
QEMU_BLK_DRIVE=${QEMU_BLK_DRIVE:--drive if=none,file=$QEMU_BLK_IMG,format=raw,id=drvblk}
QEMU_BLK_DEVICE=${QEMU_BLK_DEVICE:--device virtio-blk-device,drive=drvblk}
QEMU_BLK_LOCK_FILE=${QEMU_BLK_LOCK_FILE:-"$ROOT/build/.qemu-blk.lock"}
QEMU_BLK_LOCK_WAIT=${QEMU_BLK_LOCK_WAIT:-180}
SANDBOX_CACHE_GC=${SANDBOX_CACHE_GC:-auto}
SANDBOX_CACHE_DIR=${SANDBOX_CACHE_DIR:-/tmp/cursor-sandbox-cache}
SANDBOX_CACHE_MAX_MB=${SANDBOX_CACHE_MAX_MB:-1024}
SANDBOX_CACHE_TARGET_FREE_MB=${SANDBOX_CACHE_TARGET_FREE_MB:-1024}
SANDBOX_CACHE_MIN_AGE_SECS=${SANDBOX_CACHE_MIN_AGE_SECS:-1800}
BUILD_TMPDIR_DEFAULT=${BUILD_TMPDIR_DEFAULT:-"$ROOT/.tmp/build"}
BUILD_TMP_MIN_FREE_MB=${BUILD_TMP_MIN_FREE_MB:-256}
HYPOTHESIS_LOG=${HYPOTHESIS_LOG:-$LOG_DIR/hypothesis.json}
RUN_ID=${RUN_ID:-"run-qemu-$(date +%s)-$$"}

# When NEXUS_SKIP_BUILD=1, every per-component `cargo build` below is
# replaced with a "must-already-exist" artifact check. The Makefile sets
# this for `make test` and `make run` so they consume the artifacts that
# `make build` produced (one source of truth for compilation). The
# `just`-driven dev path leaves it unset (=0) and keeps the historical
# lazy-build convenience.
NEXUS_SKIP_BUILD=${NEXUS_SKIP_BUILD:-0}

# require_or_build <artifact-path> <human-name> -- <cargo args...>
# When NEXUS_SKIP_BUILD=1: artifact MUST exist or we fail with a clear
# hint to run `make build`. Otherwise: invoke `cargo` with the trailing
# args (the historical eager-build path) and let cargo's incremental
# cache do the right thing.
require_or_build() {
  local artifact=$1
  local label=$2
  shift 2
  if [[ "$1" == "--" ]]; then
    shift
  fi
  if [[ "$NEXUS_SKIP_BUILD" == "1" ]]; then
    if [[ ! -e "$artifact" ]]; then
      echo "[error] NEXUS_SKIP_BUILD=1 but $label artifact is missing:" >&2
      echo "          $artifact" >&2
      echo "        Run 'make build' (or unset NEXUS_SKIP_BUILD) and retry." >&2
      exit 1
    fi
    echo "[skip-build] $label: $artifact" >&2
    return 0
  fi

  # Run cargo build. Compiler errors go to stderr (visible to user).
  # Capture exit code for hypothesis diagnostics (H4).
  (cd "$ROOT" && "$@")
  local build_rc=$?

  if [[ $build_rc -ne 0 ]]; then
    debug_log "H4" "scripts/run-qemu-rv64.sh:build-errors" "cargo build failure for $label" \
      "{\"label\":\"$label\",\"exit_code\":$build_rc,\"artifact\":\"$artifact\",\"note\":\"see terminal stderr for compiler details\"}"
  fi

  return $build_rc
}

join_by() {
  local IFS="$1"
  shift
  echo "$*"
}

resolve_qemu_display_backend() {
  local backend=${QEMU_DISPLAY_BACKEND}
  if [[ "$backend" == "gtk" ]]; then
    backend="gtk,show-menubar=off"
  elif [[ "$backend" == gtk,* ]]; then
    if [[ ",$backend," != *",show-menubar="* ]]; then
      backend="${backend},show-menubar=off"
    fi
  fi
  if [[ "$QEMU_SESSION_MODE" == "interactive" && "$backend" == gtk,* ]]; then
    if [[ ",$backend," != *",grab-on-hover="* ]]; then
      backend="${backend},grab-on-hover=on"
    fi
  fi
  printf '%s' "$backend"
}

prefer_interactive_absolute_pointer() {
  [[ "$QEMU_SESSION_MODE" == "interactive" \
    && "$NEXUS_PROFILE_INPUT_TOUCH" == "1" \
    && "$NEXUS_PROFILE_INPUT_MOUSE" == "1" ]]
}

load_systemui_input_profile() {
  [[ "${NEXUS_DISPLAY_BOOTSTRAP:-0}" != "1" ]] && return 0
  local env_lines
  if ! env_lines=$(python3 "$QEMU_PROFILE_INPUT_HELPER" --repo-root "$ROOT" --profile "$NEXUS_SYSTEMUI_PROFILE" 2>&1); then
    echo "[error] failed to resolve SystemUI input profile '$NEXUS_SYSTEMUI_PROFILE':" >&2
    echo "$env_lines" >&2
    exit 1
  fi
  while IFS='=' read -r key value; do
    [[ -z "$key" ]] && continue
    export "$key=$value"
  done <<< "$env_lines"
  case "$QEMU_PROOF_POINTER_SOURCE" in
    "")
      ;;
    mouse)
      export NEXUS_PROFILE_INPUT_TOUCH=0
      export NEXUS_PROFILE_INPUT_MOUSE=1
      ;;
    touch)
      export NEXUS_PROFILE_INPUT_TOUCH=1
      export NEXUS_PROFILE_INPUT_MOUSE=0
      ;;
    *)
      echo "[error] unsupported QEMU_PROOF_POINTER_SOURCE='$QEMU_PROOF_POINTER_SOURCE' (expected: mouse|touch)" >&2
      exit 1
      ;;
  esac
}

set_env_var() {
  local name=$1
  local value=$2
  printf -v "$name" '%s' "$value"
  export "$name"
}

# #region agent log
debug_log() {
  local hypothesis_id=$1
  local location=$2
  local message=$3
  local data_json=${4:-"{}"}
  local ts
  ts=$(date +%s%3N 2>/dev/null || date +%s000)
  data_json=$(printf '%s' "$data_json" | tr -d '\n\r')
  printf '{"runId":"%s","hypothesisId":"%s","location":"%s","message":"%s","data":%s,"timestamp":%s}\n' \
    "$RUN_ID" "$hypothesis_id" "$location" "$message" "$data_json" "$ts" >>"$HYPOTHESIS_LOG" 2>/dev/null || true
}
# #endregion

# #region agent log
json_escape() {
  local value=${1:-}
  value=${value//\\/\\\\}
  value=${value//\"/\\\"}
  value=${value//$'\n'/ }
  value=${value//$'\r'/ }
  printf '%s' "$value"
}
# #endregion

df_available_kb() {
  local path=$1
  if ! command -v df >/dev/null 2>&1; then
    echo -1
    return 0
  fi
  df -Pk "$path" 2>/dev/null | awk 'NR==2 {print $4}' || echo -1
}

mem_available_kb() {
  if [[ -r /proc/meminfo ]]; then
    awk '/^MemAvailable:/ {print $2; found=1} END {if (!found) print -1}' /proc/meminfo
  else
    echo -1
  fi
}

sandbox_cache_usage_kb() {
  local cache_root=$1
  if [[ ! -d "$cache_root" ]]; then
    echo 0
    return 0
  fi
  du -sk "$cache_root" 2>/dev/null | awk '{print $1}' || echo 0
}

path_usage_kb() {
  local path=$1
  if [[ ! -e "$path" ]]; then
    echo 0
    return 0
  fi
  du -sk "$path" 2>/dev/null | awk '{print $1}' || echo -1
}

gc_sandbox_cache() {
  local mode=${SANDBOX_CACHE_GC:-auto}
  case "$mode" in
    off|OFF|0|false|FALSE) return 0 ;;
    on|ON|1|true|TRUE) mode="on" ;;
    auto|AUTO|*) mode="auto" ;;
  esac
  if [[ ! -d "$SANDBOX_CACHE_DIR" ]]; then
    return 0
  fi

  local max_cache_kb=$(( SANDBOX_CACHE_MAX_MB * 1024 ))
  local target_free_kb=$(( SANDBOX_CACHE_TARGET_FREE_MB * 1024 ))
  local min_age_secs=${SANDBOX_CACHE_MIN_AGE_SECS:-1800}
  local cache_kb
  local tmp_free_kb
  local emergency_gc=0
  cache_kb=$(sandbox_cache_usage_kb "$SANDBOX_CACHE_DIR")
  tmp_free_kb=$(df_available_kb /tmp)
  if [[ "$tmp_free_kb" =~ ^[0-9]+$ ]] && (( tmp_free_kb >= 0 && tmp_free_kb < target_free_kb )); then
    emergency_gc=1
  fi
  # #region agent log
  debug_log "H2" "scripts/run-qemu-rv64.sh:gc-pre" "sandbox cache + tmp free before gc decision" \
    "{\"cache_dir\":\"$SANDBOX_CACHE_DIR\",\"cache_kb\":$cache_kb,\"tmp_free_kb\":$tmp_free_kb,\"mode\":\"$mode\",\"max_cache_mb\":$SANDBOX_CACHE_MAX_MB,\"target_free_mb\":$SANDBOX_CACHE_TARGET_FREE_MB,\"emergency_gc\":$emergency_gc}"
  # #endregion

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
  entries=( "$SANDBOX_CACHE_DIR"/* )
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
  local skipped_young=0
  local before_kb=$cache_kb
  for (( idx=${#entries[@]} - 1; idx>=0; idx-- )); do
    entry=${entries[$idx]}
    [[ -e "$entry" ]] || continue
    mtime=$(stat -c %Y "$entry" 2>/dev/null || echo "$now")
    age=$(( now - mtime ))
    if [[ "$mode" == "auto" && "$emergency_gc" != "1" ]] && (( age < min_age_secs )); then
      skipped_young=$(( skipped_young + 1 ))
      continue
    fi
    rm -rf "$entry" 2>/dev/null || true
    removed=$(( removed + 1 ))
    cache_kb=$(sandbox_cache_usage_kb "$SANDBOX_CACHE_DIR")
    tmp_free_kb=$(df_available_kb /tmp)
    if (( cache_kb <= max_cache_kb )); then
      if ! [[ "$tmp_free_kb" =~ ^[0-9]+$ ]] || (( tmp_free_kb >= target_free_kb )); then
        break
      fi
    fi
  done
  if (( removed > 0 )); then
    echo "[info] sandbox cache gc: removed=${removed} before_kb=${before_kb} after_kb=${cache_kb} tmp_free_kb=${tmp_free_kb}" >&2
  fi
  # #region agent log
  debug_log "H2" "scripts/run-qemu-rv64.sh:gc-post" "sandbox cache state after gc pass" \
    "{\"removed\":$removed,\"before_kb\":$before_kb,\"after_kb\":$cache_kb,\"tmp_free_kb\":$tmp_free_kb,\"skipped_young\":$skipped_young,\"emergency_gc\":$emergency_gc}"
  # #endregion
}

prepare_build_tmpdir() {
  if [[ -z "${TMPDIR:-}" ]]; then
    export TMPDIR="$BUILD_TMPDIR_DEFAULT"
  fi
  mkdir -p "$TMPDIR"
  local min_kb=$(( BUILD_TMP_MIN_FREE_MB * 1024 ))
  local tmp_free_kb
  tmp_free_kb=$(df_available_kb "$TMPDIR")
  if [[ "$tmp_free_kb" =~ ^[0-9]+$ ]] && (( tmp_free_kb >= 0 && tmp_free_kb < min_kb )); then
    local fallback="$ROOT/.tmp/build-fallback"
    mkdir -p "$fallback"
    export TMPDIR="$fallback"
    echo "[warn] low tmp free space; switching TMPDIR=$TMPDIR" >&2
  fi
  echo "[info] Build TMPDIR=$TMPDIR" >&2
  echo "[info] Build target dir=$TARGET_ROOT" >&2
  # #region agent log
  debug_log "H1" "scripts/run-qemu-rv64.sh:build-paths" "effective build output and tmp directories" \
    "{\"cargo_target_dir\":\"$CARGO_TARGET_DIR\",\"target_root\":\"$TARGET_ROOT\",\"tmpdir\":\"$TMPDIR\",\"tmp_free_kb\":$tmp_free_kb}"
  # #endregion
}

declare -a SERVICES=()

DEFAULT_SERVICE_LIST=""

prepare_service_payloads() {
  if [[ -z "${INIT_LITE_SERVICE_LIST:-}" ]]; then
    INIT_LITE_SERVICE_LIST="$(scripts/discover-services.sh --list | paste -sd, -)"
    export INIT_LITE_SERVICE_LIST
  fi

  if [[ -z "${INIT_LITE_SERVICE_LIST:-}" ]]; then
    SERVICES=()
  else
    IFS=',' read -r -a SERVICES <<<"$INIT_LITE_SERVICE_LIST"
  fi

  if [[ "${#SERVICES[@]}" -eq 0 ]]; then
    return
  fi

  for raw in "${SERVICES[@]}"; do
    local svc=${raw//[[:space:]]/}
    [[ -z "$svc" ]] && continue
    local svc_upper
    svc_upper=$(echo "$svc" | tr '[:lower:]' '[:upper:]' | tr '-' '_')

    local cargo_flags_var="INIT_LITE_SERVICE_${svc_upper}_CARGO_FLAGS"
    local -a cargo_args=(build -p "$svc" --target "$TARGET" --release)
    if [[ -n "${!cargo_flags_var:-}" ]]; then
      # shellcheck disable=SC2206 # intentionally split user-provided flags
      local extra_flags=(${!cargo_flags_var})
      cargo_args+=("${extra_flags[@]}")
    else
      cargo_args+=(--no-default-features --features os-lite)
    fi
    local elf_path="$TARGET_ROOT/$TARGET/release/$svc"
    require_or_build "$elf_path" "service:$svc" -- env RUSTFLAGS="$RUSTFLAGS_OS" cargo "${cargo_args[@]}"
    set_env_var "INIT_LITE_SERVICE_${svc_upper}_ELF" "$elf_path"
    local stack_var="INIT_LITE_SERVICE_${svc_upper}_STACK_PAGES"
    if [[ -z "${!stack_var:-}" ]]; then
      case "$svc" in
        hidrawd|touchd|inputd)
          # TASK-0253 proof services run bounded init-only payload entries.
          set_env_var "$stack_var" "1"
          ;;
        *)
          set_env_var "$stack_var" "8"
          ;;
      esac
    fi
  done
}

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

# UART stream monitor that enforces the os-lite init readiness sequence before
# allowing RUN_UNTIL_MARKER=1 to stop QEMU. Expected order:
#   init: start
#   init: start <service>
#   init: up <service>
#   init: ready
#   packagefsd: ready
#   vfsd: ready
#   execd/selftest markers: elf load -> hello-elf -> e2e exec-elf -> exit0 start ->
#     child exited -> child exit ok
#   policy allow/deny probes
#   VFS stat/read/ebadf checks
monitor_uart() {
  local line
  local saw_init_start=0
  local saw_init_start_keystored=0
  local saw_init_start_rngd=0
  local saw_init_start_policyd=0
  local saw_init_start_logd=0
  local saw_init_start_metricsd=0
  local saw_init_start_samgrd=0
  local saw_init_start_bundlemgrd=0
  local saw_init_start_packagefsd=0
  local saw_init_start_vfsd=0
  local saw_init_start_execd=0
  local saw_ready=0
  local saw_elf_ok=0
  local saw_exec_selftest=0
  local saw_child=0
  local saw_child_exit_start=0
  local saw_exit_log=0
  local saw_child_exit_ok=0
  local saw_execd_malformed=0
  local saw_exec_denied=0
  local saw_ipc_payload_roundtrip=0
  local saw_ipc_deadline_timeout=0
  local saw_nexus_ipc_kernel_loopback=0
  local saw_ipc_cap_move_reply=0
  local saw_ipc_sender_pid=0
  local saw_ipc_sender_service_id=0
  local saw_keystored_capmove=0
  local saw_kselftest_ipc_queue_full=0
  local saw_kselftest_ipc_bytes_full=0
  local saw_kselftest_ipc_global_bytes_budget=0
  local saw_kselftest_ipc_owner_bytes_budget=0
  local saw_kselftest_ipc_recv_waiter_fifo=0
  local saw_kselftest_ipc_send_waiter_fifo=0
  local saw_kselftest_ipc_send_unblock=0
  local saw_kselftest_ipc_endpoint_quota=0
  local saw_kselftest_ipc_close_wakes=0
  local saw_kselftest_ipc_owner_exit_wakes=0
  local saw_ipc_routing=0
  local saw_ipc_routing_pkg=0
  local saw_ipc_routing_policyd=0
  local saw_ipc_routing_bundlemgrd=0
  local saw_ipc_routing_samgrd=0
  local saw_samgrd_lookup=0
  local saw_samgrd_unknown=0
  local saw_samgrd_malformed=0
  local saw_samgrd_register=0
  local saw_ipc_routing_execd=0
  local saw_ipc_routing_keystored=0
  local saw_keystored_v1=0
  local saw_bundlemgrd_list=0
  local saw_bundlemgrd_malformed=0
  local saw_bundlemgrd_route_execd_denied=0
  local saw_pkgfs_ready=0
  local saw_vfsd_ready=0
  local saw_metricsd_ready=0
  local saw_init_up_keystored=0
  local saw_init_up_rngd=0
  local saw_init_up_policyd=0
  local saw_init_up_logd=0
  local saw_init_up_metricsd=0
  local saw_init_up_samgrd=0
  local saw_init_up_bundlemgrd=0
  local saw_init_up_packagefsd=0
  local saw_init_up_vfsd=0
  local saw_init_up_execd=0
  local saw_policy_allow=0
  local saw_policy_deny=0
  local saw_policy_spoof_denied=0
  local saw_policy_malformed=0
  local saw_vfs_stat=0
  local saw_vfs_read=0
  local saw_vfs_ebadf=0
  local saw_selftest_end=0
  while IFS= read -r line; do
    # Generic single-marker short-circuit: if RUN_UNTIL_MARKER is a non-zero,
    # non-"1" string, stop as soon as the line contains it.
    if [[ "$RUN_UNTIL_MARKER" != "0" && "$RUN_UNTIL_MARKER" != "1" ]]; then
      if [[ "$line" == *"$RUN_UNTIL_MARKER"* ]]; then
        echo "[info] Marker \"$RUN_UNTIL_MARKER\" seen – stopping QEMU" >&2
        pkill -f qemu-system-riscv64 >/dev/null 2>&1 || true
        break
      fi
    fi
    case "$line" in
      *"init: start keystored"*)
        saw_init_start_keystored=1
        ;;
      *"init: start rngd"*)
        saw_init_start_rngd=1
        ;;
      *"init: start policyd"*)
        saw_init_start_policyd=1
        ;;
      *"init: start logd"*)
        saw_init_start_logd=1
        ;;
      *"init: start metricsd"*)
        saw_init_start_metricsd=1
        ;;
      *"init: start samgrd"*)
        saw_init_start_samgrd=1
        ;;
      *"init: start bundlemgrd"*)
        saw_init_start_bundlemgrd=1
        ;;
      *"init: start packagefsd"*)
        saw_init_start_packagefsd=1
        ;;
      *"init: start vfsd"*)
        saw_init_start_vfsd=1
        ;;
      *"init: start execd"*)
        saw_init_start_execd=1
        ;;
      *"init: start"*)
        saw_init_start=1
        ;;
      *"init: ready"*)
        saw_ready=1
        ;;
      *"init: up keystored"*)
        saw_init_up_keystored=1
        ;;
      *"init: up rngd"*)
        saw_init_up_rngd=1
        ;;
      *"init: up policyd"*)
        saw_init_up_policyd=1
        ;;
      *"init: up logd"*)
        saw_init_up_logd=1
        ;;
      *"init: up metricsd"*)
        saw_init_up_metricsd=1
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
      *"metricsd: ready"*)
        saw_metricsd_ready=1
        ;;
      *"execd: elf load ok"*)
        saw_elf_ok=1
        ;;
      *"SELFTEST: e2e exec-elf ok"*)
        saw_exec_selftest=1
        ;;
      *"child: hello-elf"*)
        saw_child=1
        ;;
      *"child: exit0 start"*)
        saw_child_exit_start=1
        ;;
      *"execd: child exited pid="*)
        saw_exit_log=1
        ;;
      *"SELFTEST: child exit ok"*)
        saw_child_exit_ok=1
        ;;
      *"SELFTEST: execd malformed ok"*)
        saw_execd_malformed=1
        ;;
      *"SELFTEST: exec denied ok"*)
        saw_exec_denied=1
        ;;
      *"SELFTEST: ipc payload roundtrip ok"*)
        saw_ipc_payload_roundtrip=1
        ;;
      *"SELFTEST: ipc deadline timeout ok"*)
        saw_ipc_deadline_timeout=1
        ;;
      *"SELFTEST: nexus-ipc kernel loopback ok"*)
        saw_nexus_ipc_kernel_loopback=1
        ;;
      *"SELFTEST: ipc cap move reply ok"*)
        saw_ipc_cap_move_reply=1
        ;;
      *"SELFTEST: ipc sender pid ok"*)
        saw_ipc_sender_pid=1
        ;;
      *"SELFTEST: ipc sender service_id ok"*)
        saw_ipc_sender_service_id=1
        ;;
      *"SELFTEST: ipc routing policyd ok"*)
        saw_ipc_routing_policyd=1
        ;;
      *"SELFTEST: ipc routing bundlemgrd ok"*)
        saw_ipc_routing_bundlemgrd=1
        ;;
      *"SELFTEST: ipc routing samgrd ok"*)
        saw_ipc_routing_samgrd=1
        ;;
      *"SELFTEST: samgrd v1 lookup ok"*)
        saw_samgrd_lookup=1
        ;;
      *"SELFTEST: samgrd v1 register ok"*)
        saw_samgrd_register=1
        ;;
      *"SELFTEST: samgrd v1 unknown ok"*)
        saw_samgrd_unknown=1
        ;;
      *"SELFTEST: samgrd v1 malformed ok"*)
        saw_samgrd_malformed=1
        ;;
      *"SELFTEST: bundlemgrd v1 list ok"*)
        saw_bundlemgrd_list=1
        ;;
      *"SELFTEST: bundlemgrd v1 malformed ok"*)
        saw_bundlemgrd_malformed=1
        ;;
      *"SELFTEST: bundlemgrd route execd denied ok"*)
        saw_bundlemgrd_route_execd_denied=1
        ;;
      *"SELFTEST: ipc routing execd ok"*)
        saw_ipc_routing_execd=1
        ;;
      *"SELFTEST: ipc routing keystored ok"*)
        saw_ipc_routing_keystored=1
        ;;
      *"SELFTEST: keystored v1 ok"*)
        saw_keystored_v1=1
        ;;
      *"SELFTEST: keystored capmove ok"*)
        saw_keystored_capmove=1
        ;;
      *"KSELFTEST: ipc queue full ok"*)
        saw_kselftest_ipc_queue_full=1
        ;;
      *"KSELFTEST: ipc bytes full ok"*)
        saw_kselftest_ipc_bytes_full=1
        ;;
      *"KSELFTEST: ipc global bytes budget ok"*)
        saw_kselftest_ipc_global_bytes_budget=1
        ;;
      *"KSELFTEST: ipc owner bytes budget ok"*)
        saw_kselftest_ipc_owner_bytes_budget=1
        ;;
      *"KSELFTEST: ipc recv waiter fifo ok"*)
        saw_kselftest_ipc_recv_waiter_fifo=1
        ;;
      *"KSELFTEST: ipc send waiter fifo ok"*)
        saw_kselftest_ipc_send_waiter_fifo=1
        ;;
      *"KSELFTEST: ipc send unblock ok"*)
        saw_kselftest_ipc_send_unblock=1
        ;;
      *"KSELFTEST: ipc endpoint quota ok"*)
        saw_kselftest_ipc_endpoint_quota=1
        ;;
      *"KSELFTEST: ipc close wakes ok"*)
        saw_kselftest_ipc_close_wakes=1
        ;;
      *"KSELFTEST: ipc owner-exit wakes ok"*)
        saw_kselftest_ipc_owner_exit_wakes=1
        ;;
      *"SELFTEST: ipc routing ok"*)
        saw_ipc_routing=1
        ;;
      *"SELFTEST: ipc routing packagefsd ok"*)
        saw_ipc_routing_pkg=1
        ;;
      *"SELFTEST: policy allow ok"*)
        saw_policy_allow=1
        ;;
      *"SELFTEST: policy deny ok"*)
        saw_policy_deny=1
        ;;
      *"SELFTEST: policyd requester spoof denied ok"*)
        saw_policy_spoof_denied=1
        ;;
      *"SELFTEST: policy malformed ok"*)
        saw_policy_malformed=1
        ;;
      *"SELFTEST: vfs stat ok"*)
        saw_vfs_stat=1
        ;;
      *"SELFTEST: vfs read ok"*)
        saw_vfs_read=1
        ;;
      *"SELFTEST: vfs ebadf ok"*)
        saw_vfs_ebadf=1
        ;;
      *"SELFTEST: end"*)
        saw_selftest_end=1
        ;;
      *"windowd: interactive scene ready"*)
        printf 'ready\n' >"$INTERACTIVE_READY_SENTINEL"
        ;;
      *"I: after selftest"*|*"KSELFTEST: spawn ok"*|*"SELFTEST: ipc ok"*|*"SELFTEST: end"*)
        # When RUN_UNTIL_MARKER is a specific marker string, we only stop once that string is seen
        # (handled above). When RUN_UNTIL_MARKER=1, we stop once the full readiness/selftest set is
        # satisfied (handled below). Avoid stopping on these generic success markers, as it can
        # prevent userspace bring-up from running during phase-gated runs.
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

    if [[ "$RUN_UNTIL_MARKER" == "1" \
        && "$saw_init_start" -eq 1 && "$saw_init_start_keystored" -eq 1 && "$saw_init_start_policyd" -eq 1 && "$saw_init_start_samgrd" -eq 1 \
        && "$saw_init_start_bundlemgrd" -eq 1 && "$saw_init_start_packagefsd" -eq 1 && "$saw_init_start_vfsd" -eq 1 && "$saw_init_start_execd" -eq 1 \
        && "$saw_init_up_keystored" -eq 1 && "$saw_init_up_policyd" -eq 1 && "$saw_init_up_samgrd" -eq 1 && "$saw_init_up_bundlemgrd" -eq 1 \
        && "$saw_init_up_packagefsd" -eq 1 && "$saw_init_up_vfsd" -eq 1 && "$saw_init_up_execd" -eq 1 \
        && "$saw_ready" -eq 1 && "$saw_pkgfs_ready" -eq 1 && "$saw_vfsd_ready" -eq 1 \
        && "$saw_elf_ok" -eq 1 && "$saw_exec_selftest" -eq 1 && "$saw_child" -eq 1 && "$saw_child_exit_start" -eq 1 \
        && "$saw_exit_log" -eq 1 && "$saw_child_exit_ok" -eq 1 && "$saw_exec_denied" -eq 1 && "$saw_execd_malformed" -eq 1 && "$saw_policy_allow" -eq 1 && "$saw_policy_deny" -eq 1 && "$saw_policy_spoof_denied" -eq 1 && "$saw_policy_malformed" -eq 1 \
        && "$saw_ipc_payload_roundtrip" -eq 1 \
        && "$saw_ipc_deadline_timeout" -eq 1 \
        && "$saw_nexus_ipc_kernel_loopback" -eq 1 \
        && "$saw_ipc_cap_move_reply" -eq 1 \
        && "$saw_ipc_sender_pid" -eq 1 \
        && "$saw_ipc_sender_service_id" -eq 1 \
        && "$saw_keystored_capmove" -eq 1 \
        && "$saw_kselftest_ipc_queue_full" -eq 1 \
        && "$saw_kselftest_ipc_bytes_full" -eq 1 \
        && "$saw_kselftest_ipc_global_bytes_budget" -eq 1 \
        && "$saw_kselftest_ipc_owner_bytes_budget" -eq 1 \
        && "$saw_kselftest_ipc_recv_waiter_fifo" -eq 1 \
        && "$saw_kselftest_ipc_send_waiter_fifo" -eq 1 \
        && "$saw_kselftest_ipc_send_unblock" -eq 1 \
        && "$saw_kselftest_ipc_endpoint_quota" -eq 1 \
        && "$saw_kselftest_ipc_close_wakes" -eq 1 \
        && "$saw_kselftest_ipc_owner_exit_wakes" -eq 1 \
        && "$saw_ipc_routing_policyd" -eq 1 \
        && "$saw_ipc_routing_bundlemgrd" -eq 1 \
        && "$saw_bundlemgrd_list" -eq 1 \
        && "$saw_bundlemgrd_malformed" -eq 1 \
        && "$saw_bundlemgrd_route_execd_denied" -eq 1 \
        && "$saw_ipc_routing_keystored" -eq 1 \
        && "$saw_keystored_v1" -eq 1 \
        && "$saw_ipc_routing_samgrd" -eq 1 \
        && "$saw_samgrd_register" -eq 1 \
        && "$saw_samgrd_lookup" -eq 1 \
        && "$saw_samgrd_unknown" -eq 1 \
        && "$saw_samgrd_malformed" -eq 1 \
        && "$saw_ipc_routing_execd" -eq 1 \
        && "$saw_ipc_routing" -eq 1 \
        && "$saw_ipc_routing_pkg" -eq 1 \
        && "$saw_vfs_stat" -eq 1 && "$saw_vfs_read" -eq 1 && "$saw_vfs_ebadf" -eq 1 \
        && "$saw_selftest_end" -eq 1 ]]; then
      echo "[info] Success marker detected – stopping QEMU" >&2
      pkill -f qemu-system-riscv64 >/dev/null 2>&1 || true
      break
    fi
  done
}

# #region agent log
monitor_agent_uart() {
  while IFS= read -r line; do
    case "$line" in
      agent8cde1d\|*)
        local hypothesis_id location message data
        IFS='|' read -r _ hypothesis_id location message data <<<"$line"
        debug_log "$hypothesis_id" "$location" "$message" \
          "{\"marker\":\"$(json_escape "$data")\"}"
        ;;
    esac
  done
}
# #endregion

finish() {
  local status=$1
  trim_log "$QEMU_LOG" "$QEMU_LOG_MAX"
  trim_log "$UART_LOG" "$UART_LOG_MAX"
  if [[ "$status" -eq 143 && "$RUN_UNTIL_MARKER" != "0" ]]; then
    echo "[info] QEMU stopped after success marker" >&2
    status=0
  fi
  if [[ "$status" -eq 124 \
      && "$QEMU_SESSION_MODE" == "interactive" \
      && "$QEMU_MARKER_LEVEL" == "minimal" \
      && -f "$INTERACTIVE_READY_SENTINEL" ]]; then
    echo "[info] QEMU reached interactive scene readiness before timeout; accepting time-capped make run" >&2
    status=0
  fi
  if [[ "$status" -eq 124 && "$RUN_TIMEOUT" != "0" ]]; then
    echo "[warn] QEMU terminated after exceeding timeout ($RUN_TIMEOUT)" >&2
  fi
  rm -f "$INTERACTIVE_READY_SENTINEL"
  return "$status"
}

run_qemu_stream() {
  if [[ "$RUN_TIMEOUT" == "0" ]]; then
    stdbuf -oL -eL qemu-system-riscv64 "${COMMON_ARGS[@]}" "$@"
  else
    timeout --signal="$QEMU_TIMEOUT_SIGNAL" --foreground "$RUN_TIMEOUT" \
      stdbuf -oL -eL qemu-system-riscv64 "${COMMON_ARGS[@]}" "$@"
  fi
}

gc_sandbox_cache
prepare_build_tmpdir
prepare_service_payloads

# Ensure a deterministic virtio-blk backing image exists for QEMU.
mkdir -p "$ROOT/build"
# Serialize access to blk image across concurrent make/qemu runs.
exec 9>"$QEMU_BLK_LOCK_FILE"
if ! flock -w "$QEMU_BLK_LOCK_WAIT" 9; then
  echo "[error] Timed out waiting for blk image lock: $QEMU_BLK_LOCK_FILE" >&2
  echo "[error] Another QEMU run is still active. Wait or stop it, then retry." >&2
  exit 1
fi
cleanup_blk_lock() {
  flock -u 9 2>/dev/null || true
  exec 9>&- 2>/dev/null || true
}
cleanup_qmp_input() {
  if [[ -n "${QEMU_INPUT_INJECT_PID:-}" ]]; then
    kill "$QEMU_INPUT_INJECT_PID" >/dev/null 2>&1 || true
  fi
  rm -f "$QEMU_QMP_SOCKET"
}
trap 'cleanup_qmp_input; cleanup_blk_lock' EXIT
# Always recreate the image to avoid stale QEMU "write lock" issues after aborted runs.
rm -f "$QEMU_BLK_IMG"
truncate -s 64M "$QEMU_BLK_IMG"

# Always rebuild init-lite and kernel to pick up changes (unless
# NEXUS_SKIP_BUILD=1, in which case the artifacts MUST already exist).
require_or_build "$INIT_ELF" "init-lite" -- \
  env RUSTFLAGS="$RUSTFLAGS_OS" cargo build -p init-lite --target "$TARGET" --release
kernel_build() {
  local -a cargo_args=(build -p neuron-boot --target "$TARGET" --release)
  if [[ -n "$NEURON_BOOT_FEATURES" ]]; then
    cargo_args+=(--features "$NEURON_BOOT_FEATURES")
  fi
  require_or_build "$KERNEL_ELF" "neuron-boot" -- \
    env EMBED_INIT_ELF="$INIT_ELF" RUSTFLAGS="$RUSTFLAGS_OS" cargo "${cargo_args[@]}"
}

kernel_build

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
  -bios default
  -kernel "$KERNEL_BIN"
)

if [[ "$NEXUS_DISPLAY_BOOTSTRAP" == "1" ]]; then
  load_systemui_input_profile
  RESOLVED_QEMU_DISPLAY_BACKEND=$(resolve_qemu_display_backend)
  # Production-grade GPU-only path: virtio-gpu as the sole display device.
  # gpud renders the boot splash and calls SET_SCANOUT immediately, so the
  # GTK window shows the splash within ~300ms of boot. windowd later sends
  # composited frames via OP_SET_FRAMEBUFFER_VMO which overwrites the splash.
  COMMON_ARGS+=( -display "$RESOLVED_QEMU_DISPLAY_BACKEND" -serial mon:stdio )
  COMMON_ARGS+=( -device virtio-gpu-device,max_outputs=1 )
  QEMU_GPU_DEVICE_PLACED=1
  if [[ "$NEXUS_PROFILE_INPUT_KBD" == "1" ]]; then
    COMMON_ARGS+=( -device virtio-keyboard-device )
  fi
  # In the interactive GTK lane, prefer the absolute tablet pointer when the
  # profile exposes both mouse and touch. This gives `just start` a stable
  # host-pointer stream without discarding mixed-source support in proof mode.
  if prefer_interactive_absolute_pointer; then
    COMMON_ARGS+=( -device virtio-tablet-device )
  else
    # QEMU's tablet device is the closest bounded absolute-pointer stand-in for
    # touch/tablet profiles.
    if [[ "$NEXUS_PROFILE_INPUT_TOUCH" == "1" ]]; then
      COMMON_ARGS+=( -device virtio-tablet-device )
    fi
    if [[ "$NEXUS_PROFILE_INPUT_MOUSE" == "1" ]]; then
      COMMON_ARGS+=( -device virtio-mouse-device )
    fi
  fi
else
  COMMON_ARGS+=( -nographic -serial mon:stdio )
fi
if [[ "$QEMU_INPUT_AUTOINJECT" == "1" ]]; then
  rm -f "$QEMU_QMP_SOCKET"
  COMMON_ARGS+=( -qmp "unix:$QEMU_QMP_SOCKET,server=on,wait=off" )
fi

if [[ -n "${NEXUS_SELFTEST_MODE:-}" ]]; then
  COMMON_ARGS+=( -fw_cfg "name=opt/org.open-nexus/selftest-mode,string=${NEXUS_SELFTEST_MODE}" )
fi
if [[ -n "${NEXUS_SELFTEST_PROFILE:-}" ]]; then
  COMMON_ARGS+=( -fw_cfg "name=opt/org.open-nexus/selftest-profile,string=${NEXUS_SELFTEST_PROFILE}" )
fi

# Default to modern virtio-mmio for determinism (legacy virtio-mmio has known virtio-blk issues).
# Legacy is still available for opt-in debugging/bisecting via QEMU_FORCE_LEGACY=1.
if [[ "${QEMU_FORCE_LEGACY:-0}" == "1" ]]; then
  COMMON_ARGS+=( -global virtio-mmio.force-legacy=on )
else
  COMMON_ARGS+=( -global virtio-mmio.force-legacy=off )
fi

# icount mode for deterministic execution (can be disabled for debugging)
if [[ "${QEMU_NO_ICOUNT:-0}" != "1" ]]; then
  QEMU_ICOUNT_ARGS=${QEMU_ICOUNT_ARGS:-"1,sleep=on"}
  COMMON_ARGS+=( -icount "$QEMU_ICOUNT_ARGS" )
fi

COMMON_ARGS+=(
  # Networking: attach a virtio-net device on the virtio-mmio bus.
  # This is self-contained (user-mode net), requires no host TAP and remains deterministic enough
  # for marker-driven bring-up.
  ${QEMU_NETDEV}
  ${QEMU_NETDEV_DEVICE}
  ${QEMU_RNG_OBJECT}
  ${QEMU_RNG_DEVICE}
  ${QEMU_BLK_DRIVE}
  ${QEMU_BLK_DEVICE}
)
# virtio-gpu may already have been placed as primary display in the bootstrap path.
if [[ -z "${QEMU_GPU_DEVICE_PLACED:-}" ]]; then
  COMMON_ARGS+=( ${QEMU_GPU_DEVICE} )
fi

# #region agent log
has_usb_kbd=0
has_usb_tablet=0
has_virtio_keyboard=0
has_virtio_mouse=0
has_virtio_tablet=0
for qemu_arg in "${COMMON_ARGS[@]}" "$@"; do
  case "$qemu_arg" in
    *usb-kbd*) has_usb_kbd=1 ;;
    *usb-tablet*) has_usb_tablet=1 ;;
    *virtio-keyboard*) has_virtio_keyboard=1 ;;
    *virtio-mouse*) has_virtio_mouse=1 ;;
    *virtio-tablet*) has_virtio_tablet=1 ;;
  esac
done
debug_log "H7,H11" "scripts/run-qemu-rv64.sh:qemu-input-devices" "resolved qemu input device exposure before launch" \
  "{\"usb_kbd\":$has_usb_kbd,\"usb_tablet\":$has_usb_tablet,\"virtio_keyboard\":$has_virtio_keyboard,\"virtio_mouse\":$has_virtio_mouse,\"virtio_tablet\":$has_virtio_tablet,\"forwarded_arg_count\":$#}"
# #endregion

# Debug aid: print the resolved QEMU arguments (bounded).
echo "[info] QEMU_NETDEV=${QEMU_NETDEV}" >&2
echo "[info] QEMU_NETDEV_DEVICE=${QEMU_NETDEV_DEVICE}" >&2
echo "[info] QEMU session mode: ${QEMU_SESSION_MODE}" >&2
echo "[info] QEMU marker level: ${QEMU_MARKER_LEVEL}" >&2
if [[ -n "${NEXUS_SELFTEST_MODE:-}" ]]; then
  echo "[info] guest selftest mode: ${NEXUS_SELFTEST_MODE}" >&2
fi
if [[ -n "${NEXUS_SELFTEST_PROFILE:-}" ]]; then
  echo "[info] guest selftest profile: ${NEXUS_SELFTEST_PROFILE}" >&2
fi
echo "[info] NEXUS_DISPLAY_BOOTSTRAP=${NEXUS_DISPLAY_BOOTSTRAP}" >&2
if [[ "$NEXUS_DISPLAY_BOOTSTRAP" == "1" ]]; then
  echo "[info] QEMU display backend: ${QEMU_DISPLAY_BACKEND}" >&2
  echo "[info] QEMU resolved display backend: ${RESOLVED_QEMU_DISPLAY_BACKEND}" >&2
fi
if [[ "${QEMU_NO_ICOUNT:-0}" == "1" ]]; then
  echo "[info] QEMU icount: disabled" >&2
else
  echo "[info] QEMU icount: enabled" >&2
  echo "[info] QEMU icount args: ${QEMU_ICOUNT_ARGS:-}" >&2
fi
if [[ -n "${QEMU_TRACE_EVENTS:-}" ]]; then
  echo "[info] QEMU trace events: ${QEMU_TRACE_EVENTS}" >&2
  echo "[info] QEMU trace file: ${QEMU_TRACE_FILE:-qemu.trace}" >&2
fi

# #region agent log
debug_log "H1,H2,H4,H5" "scripts/run-qemu-rv64.sh:resolved-live-config" "resolved qemu live-start config before launch" \
  "{\"session_mode\":\"$QEMU_SESSION_MODE\",\"marker_level\":\"$QEMU_MARKER_LEVEL\",\"selftest_mode\":\"${NEXUS_SELFTEST_MODE:-}\",\"selftest_profile\":\"${NEXUS_SELFTEST_PROFILE:-}\",\"display_bootstrap\":\"$NEXUS_DISPLAY_BOOTSTRAP\",\"display_backend\":\"${QEMU_DISPLAY_BACKEND:-}\",\"resolved_display_backend\":\"${RESOLVED_QEMU_DISPLAY_BACKEND:-}\",\"run_timeout\":\"$RUN_TIMEOUT\",\"run_until_marker\":\"$RUN_UNTIL_MARKER\",\"skip_build\":\"$NEXUS_SKIP_BUILD\",\"kernel_bin\":\"$KERNEL_BIN\",\"forwarded_arg_count\":$#,\"forwarded_first_arg\":\"$(json_escape "${1:-}")\"}"
# #endregion

# Enable heavy QEMU tracing only when explicitly requested
if [[ "${QEMU_TRACE:-0}" == "1" ]]; then
  TRACE_FLAGS=${QEMU_TRACE_FLAGS:-int,mmu,unimp}
  COMMON_ARGS+=( -d "$TRACE_FLAGS" -D "$QEMU_LOG" )
fi

# Enable QEMU trace events (separate from `-d` logging).
#
# Example:
#   QEMU_TRACE_EVENTS="net_rx_pkt_parsed" QEMU_TRACE_FILE="qemu.trace" scripts/run-qemu-rv64.sh
#
# NOTE: This is used for debugging RX/TX delivery issues (e.g. slirp DHCP under `-icount`).
if [[ -n "${QEMU_TRACE_EVENTS:-}" ]]; then
  QEMU_TRACE_FILE=${QEMU_TRACE_FILE:-qemu.trace}
  COMMON_ARGS+=( -trace "enable=${QEMU_TRACE_EVENTS},file=${QEMU_TRACE_FILE}" )
fi

# Optional GDB stub for interactive debugging
if [[ "${QEMU_GDB:-0}" == "1" ]]; then
  COMMON_ARGS+=( -S -s -monitor none )
fi

status=0
rm -f "$INTERACTIVE_READY_SENTINEL"
# #region agent log
target_usage_kb_before=$(path_usage_kb "$TARGET_ROOT")
root_free_kb_before=$(df_available_kb "$ROOT")
tmp_free_kb_before=$(df_available_kb /tmp)
mem_avail_kb_before=$(mem_available_kb)
debug_log "H14" "scripts/run-qemu-rv64.sh:host-resources-pre" "host resource snapshot before qemu launch" \
  "{\"target_root\":\"$TARGET_ROOT\",\"target_usage_kb\":$target_usage_kb_before,\"root_free_kb\":$root_free_kb_before,\"tmp_free_kb\":$tmp_free_kb_before,\"mem_available_kb\":$mem_avail_kb_before}"
# #endregion
if [[ "$RUN_UNTIL_MARKER" != "0" ]]; then
  if [[ "$QEMU_INPUT_AUTOINJECT" == "1" ]]; then
    # #region agent log
    debug_log "H4" "scripts/run-qemu-rv64.sh:autoinject-start" "starting qmp visible-input injector" \
      "{\"enabled\":1,\"socket\":\"$QEMU_QMP_SOCKET\",\"injector\":\"$QEMU_INPUT_INJECTOR_PY\"}"
    # #endregion
    python3 "$QEMU_INPUT_INJECTOR_PY" "$QEMU_QMP_SOCKET" >&2 &
    QEMU_INPUT_INJECT_PID=$!
  else
    # #region agent log
    debug_log "H4" "scripts/run-qemu-rv64.sh:autoinject-disabled" "qmp visible-input injector disabled" \
      "{\"enabled\":0}"
    # #endregion
  fi
  set +e
  run_qemu_stream "$@" \
    2> >(tee "$QEMU_LOG" >&2) \
    | tee >(monitor_uart) >(monitor_agent_uart) \
    | tee "$UART_LOG"
  status=${PIPESTATUS[0]}
  set -e
else
  if [[ "$QEMU_INPUT_AUTOINJECT" == "1" ]]; then
    # #region agent log
    debug_log "H4" "scripts/run-qemu-rv64.sh:autoinject-start" "starting qmp visible-input injector" \
      "{\"enabled\":1,\"socket\":\"$QEMU_QMP_SOCKET\",\"injector\":\"$QEMU_INPUT_INJECTOR_PY\"}"
    # #endregion
    python3 "$QEMU_INPUT_INJECTOR_PY" "$QEMU_QMP_SOCKET" >&2 &
    QEMU_INPUT_INJECT_PID=$!
  else
    # #region agent log
    debug_log "H4" "scripts/run-qemu-rv64.sh:autoinject-disabled" "qmp visible-input injector disabled" \
      "{\"enabled\":0}"
    # #endregion
  fi
  set +e
  run_qemu_stream "$@" \
    2> >(tee "$QEMU_LOG" >&2) \
    | tee >(monitor_uart) >(monitor_agent_uart) \
    | tee "$UART_LOG"
  status=${PIPESTATUS[0]}
  set -e
fi

if [[ -n "${QEMU_INPUT_INJECT_PID:-}" ]]; then
  set +e
  wait "$QEMU_INPUT_INJECT_PID"
  injector_status=$?
  set -e
  # #region agent log
  debug_log "H4" "scripts/run-qemu-rv64.sh:autoinject-exit" "qmp visible-input injector exited" \
    "{\"injector_status\":$injector_status}"
  # #endregion
  if [[ "$injector_status" -ne 0 && "$status" -eq 0 ]]; then
    status=$injector_status
  fi
fi

# #region agent log
target_usage_kb_after=$(path_usage_kb "$TARGET_ROOT")
root_free_kb_after=$(df_available_kb "$ROOT")
tmp_free_kb_after=$(df_available_kb /tmp)
mem_avail_kb_after=$(mem_available_kb)
debug_log "H14" "scripts/run-qemu-rv64.sh:host-resources-post" "host resource snapshot after qemu launch" \
  "{\"status\":$status,\"target_root\":\"$TARGET_ROOT\",\"target_usage_kb\":$target_usage_kb_after,\"root_free_kb\":$root_free_kb_after,\"tmp_free_kb\":$tmp_free_kb_after,\"mem_available_kb\":$mem_avail_kb_after}"
# #endregion

finish "$status"
