#!/usr/bin/env bash
# Copyright 2024 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# build.sh — single source of truth for cross-compilation.
#
# Called by: qemu-launcher.sh (and indirectly qemu-test.sh via launcher).
# Not called by: Makefile (Makefile owns its own build step for container CI).
#
# Environment:
#   NEXUS_SKIP_BUILD   – when "1", skip cargo build; artifacts MUST pre-exist
#   TARGET             – Rust target triple (default: riscv64imac-unknown-none-elf)
#   TARGET_ROOT        – cargo target directory (default: $ROOT/target)
#   INIT_LITE_SERVICE_LIST – comma-separated service names to cross-compile
#   RUSTFLAGS_OS       – RUSTFLAGS for OS target
#   BUILD_TMPDIR_DEFAULT – fallback for TMPDIR (default: $ROOT/.tmp/build)
#   BUILD_TMP_MIN_FREE_MB – min free space in TMPDIR before fallback (default: 256)
#   HYPOTHESIS_LOG     – path for build hypothesis entries (H4)
#   NEURON_BOOT_FEATURES – extra cargo features for neuron-boot

set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
TARGET=${TARGET:-riscv64imac-unknown-none-elf}
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

NEXUS_SKIP_BUILD=${NEXUS_SKIP_BUILD:-0}
BUILD_TMPDIR_DEFAULT=${BUILD_TMPDIR_DEFAULT:-"$ROOT/.tmp/build"}
BUILD_TMP_MIN_FREE_MB=${BUILD_TMP_MIN_FREE_MB:-256}
HYPOTHESIS_LOG=${HYPOTHESIS_LOG:-/dev/null}
RUN_ID=${RUN_ID:-"build-$(date +%s)-$$"}

NEURON_BOOT_FEATURES=${NEURON_BOOT_FEATURES:-}

# ---------------------------------------------------------------------------
# debug_log — append a structured hypothesis entry
# ---------------------------------------------------------------------------
debug_log() {
  if [[ "$HYPOTHESIS_LOG" == "/dev/null" ]]; then return 0; fi
  local hypothesis_id=$1
  local location=$2
  local message=$3
  local data=$4
  local ts
  ts=$(date +%s%3N 2>/dev/null || echo 0)
  printf '{"runId":"%s","hypothesisId":"%s","location":"%s","message":"%s","data":%s,"timestamp":%s}\n' \
    "$RUN_ID" "$hypothesis_id" "$location" "$message" "$data" "$ts" >>"$HYPOTHESIS_LOG" 2>/dev/null || true
}

# ---------------------------------------------------------------------------
# Helper: available free space in KB for a directory
# ---------------------------------------------------------------------------
df_available_kb() {
  df --output=avail -k "$1" 2>/dev/null | tail -1 | tr -d ' ' || echo 0
}

# ---------------------------------------------------------------------------
# Helper: set an environment variable (export) safely
# ---------------------------------------------------------------------------
set_env_var() {
  local name=$1 value=$2
  printf -v "$name" '%s' "$value"
  export "$name"
}

# ---------------------------------------------------------------------------
# prepare_build_tmpdir — ensure a build TMPDIR with enough free space
# ---------------------------------------------------------------------------
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
  debug_log "H1" "scripts/build.sh:build-paths" "effective build output and tmp directories" \
    "{\"cargo_target_dir\":\"$TARGET_ROOT\",\"target_root\":\"$TARGET_ROOT\",\"tmpdir\":\"$TMPDIR\",\"tmp_free_kb\":$tmp_free_kb}"
}

# ---------------------------------------------------------------------------
# require_or_build <artifact-path> <human-name> -- <cargo args...>
#
# When NEXUS_SKIP_BUILD=1: artifact MUST exist.
# Otherwise: invoke cargo build.
# ---------------------------------------------------------------------------
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

  # Capture stderr for diagnostics while also showing it to the user.
  # Use a tmp file in TMPDIR (already created by prepare_build_tmpdir)
  # to avoid mktemp races.
  local stderr_file="$TMPDIR/build-$$-${label//[^a-zA-Z0-9]/-}.stderr"
  (cd "$ROOT" && "$@" 2> >(tee "$stderr_file" >&2))
  local build_rc=$?

  # Extract error[E…] and warning[…] lines into hypothesis log.
  if [[ $build_rc -ne 0 ]]; then
    local err_lines err_json
    err_lines=$(grep -E '^error(\[[A-Z][0-9]+\])?' "$stderr_file" 2>/dev/null | tr '\n' '|' | sed 's/["\]/\\&/g; s/|$//')
    err_json="\"${err_lines:-}\""
    debug_log "H4" "scripts/build.sh:build-errors" "cargo build failure for $label" \
      "{\"label\":\"$label\",\"exit_code\":$build_rc,\"artifact\":\"$artifact\",\"errors\":[$err_json]}"
  fi

  local warn_count
  warn_count=$(grep -cE '^warning' "$stderr_file" 2>/dev/null || echo 0)
  warn_count=${warn_count//[^0-9]/}
  [[ -z "$warn_count" ]] && warn_count=0
  if [[ "$warn_count" -gt 0 ]]; then
    local warn_lines warn_json
    warn_lines=$(grep -E '^warning' "$stderr_file" 2>/dev/null | tr '\n' '|' | sed 's/["\]/\\&/g; s/|$//')
    warn_json="\"${warn_lines:-}\""
    debug_log "H4b" "scripts/build.sh:build-warnings" "cargo build warnings for $label" \
      "{\"label\":\"$label\",\"count\":$warn_count,\"warnings\":[$warn_json]}"
  fi

  rm -f "$stderr_file"
  return $build_rc
}

# ---------------------------------------------------------------------------
# prepare_service_payloads — cross-compile each service ELF for init-lite embedding
# ---------------------------------------------------------------------------
declare -a SERVICES=()

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
      # shellcheck disable=SC2206
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
          set_env_var "$stack_var" "1"
          ;;
        *)
          set_env_var "$stack_var" "8"
          ;;
      esac
    fi
  done
}

# ---------------------------------------------------------------------------
# build_kernel_and_init — build kernel + init-lite (with embedded service ELFs)
# ---------------------------------------------------------------------------
build_kernel_and_init() {
  # Build init-lite FIRST — the kernel needs EMBED_INIT_ELF to point at it.
  require_or_build "$INIT_ELF" "init-lite" -- env RUSTFLAGS="$RUSTFLAGS_OS" cargo build -p init-lite --target "$TARGET" --release

  local -a kernel_args=(build -p neuron-boot --target "$TARGET" --release)
  if [[ -n "${NEURON_BOOT_FEATURES:-}" ]]; then
    kernel_args+=(--features "$NEURON_BOOT_FEATURES")
  fi
  require_or_build "$KERNEL_ELF" "kernel:neuron-boot" -- env EMBED_INIT_ELF="$INIT_ELF" RUSTFLAGS="$RUSTFLAGS_OS" cargo "${kernel_args[@]}"
}

# ---------------------------------------------------------------------------
# build_all — full build pipeline (services → init-lite → kernel with EMBED_INIT_ELF)
# ---------------------------------------------------------------------------
build_all() {
  prepare_build_tmpdir
  prepare_service_payloads
  build_kernel_and_init
  # Post-build artifact verification
  if [[ ! -f "$KERNEL_ELF" ]]; then
    echo "[error] build.sh: kernel ELF not produced: $KERNEL_ELF" >&2
    exit 1
  fi
  if [[ ! -f "$INIT_ELF" ]]; then
    echo "[error] build.sh: init-lite ELF not produced: $INIT_ELF" >&2
    exit 1
  fi
  local ksize isize
  ksize=$(wc -c <"$KERNEL_ELF" 2>/dev/null || echo 0)
  isize=$(wc -c <"$INIT_ELF" 2>/dev/null || echo 0)
  if [[ "$ksize" -lt 200000 ]]; then
    echo "[error] build.sh: kernel is too small (${ksize} bytes) — is EMBED_INIT_ELF set?" >&2
    exit 1
  fi
  if [[ "$isize" -lt 100000 ]]; then
    echo "[error] build.sh: init-lite is too small (${isize} bytes) — are services embedded?" >&2
    exit 1
  fi
  echo "[info] Build complete (kernel=${ksize}B, init=${isize}B)" >&2
}

# If executed directly, run the full build.
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  build_all
fi
