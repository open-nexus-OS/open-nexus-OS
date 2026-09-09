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
# TASK-0289 A4: the VMM -kernel payload is the first-stage loader; the
# kernel image travels inside the GPT disk (boot-a slot, NXBD-signed).
NXBOOT_ELF=$TARGET_ROOT/$TARGET/release/nxboot
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
    # Warning gate: turn the H4b telemetry into an actual gate on the REAL OS
    # build path. Opt-in via NEXUS_WARN_GATE=1 (set by `just build-os-workspace`
    # / test-all / CI) so interactive `just start`/`make build` stay friendly.
    # NEXUS_ALLOW_WARN=1 forces it off even when the gate is on (Phase-2 churn).
    if [[ "${NEXUS_WARN_GATE:-0}" == "1" && "${NEXUS_ALLOW_WARN:-0}" != "1" && $build_rc -eq 0 ]]; then
      echo "[error] warning gate: $label emitted $warn_count warning(s) (NEXUS_WARN_GATE=1)." >&2
      echo "[error] fix them, or set NEXUS_ALLOW_WARN=1 for local exploration." >&2
      grep -E '^warning' "$stderr_file" >&2 || true
      build_rc=1
    fi
  fi

  rm -f "$stderr_file"
  return $build_rc
}

# ---------------------------------------------------------------------------
# Build truth (TASK-0324 P0): ONE resolver for what a service is built with.
# `scripts/discover-services.sh` reads `[package.metadata.nexus-service]`
# (features + feature_profiles keyed on env such as GPU_MODE) — the embedded
# init-lite table and the system-volume bundles both go through
# build_service(), so a service can never ship with two feature sets. The
# artifact lives under build/services/<svc>/<feature-key>/payload.elf: two
# feature sets never share an ELF (the 2026-09-09 black screen was the
# embedded virgl build and the os-lite bundle build overwriting one file).
# ---------------------------------------------------------------------------
service_cargo_features() { scripts/discover-services.sh --cargo-features "$1"; }
service_feature_key()    { scripts/discover-services.sh --feature-key "$1"; }
service_elf_path()       { scripts/discover-services.sh --elf-path "$1"; }
service_stack_pages()    { scripts/discover-services.sh --stack-pages "$1"; }

# build_service <svc>: cargo-build with the resolved features, then publish the
# ELF at its keyed path (+ features.txt beside it). Prints nothing; the keyed
# path is what callers use. NEXUS_SKIP_BUILD=1 requires the keyed artifact.
build_service() {
  local svc=$1
  local features keyed
  features=$(service_cargo_features "$svc")
  keyed=$(service_elf_path "$svc")
  local cargo_out="$TARGET_ROOT/$TARGET/release/$svc"
  if [[ "$NEXUS_SKIP_BUILD" == "1" ]]; then
    require_or_build "$keyed" "service:$svc" -- true
    return 0
  fi
  require_or_build "$cargo_out" "service:$svc" -- env RUSTFLAGS="$RUSTFLAGS_OS" \
    cargo build -p "$svc" --target "$TARGET" --release --no-default-features --features "$features"
  mkdir -p "$(dirname "$keyed")"
  cp -f "$cargo_out" "$keyed"
  printf '%s\n' "$features" >"$(dirname "$keyed")/features.txt"
}

# ---------------------------------------------------------------------------
# prepare_service_payloads — cross-compile each service ELF for init-lite embedding
# ---------------------------------------------------------------------------
declare -a SERVICES=()

prepare_service_payloads() {
  # TASK-0080D R1: the app-host runtime ELF is NOT a boot service (no
  # nexus-service metadata; init never spawns it). Build it FIRST and hand the
  # ELF to execd's build via EXECD_APPHOST_ELF — execd embeds it as the
  # IMG_APPHOST payload for on-demand spawns.
  build_service app-host
  set_env_var "EXECD_APPHOST_ELF" "$(service_elf_path app-host)"

  # P0.2 recv-wake regression gate: the probe child execd spawns once after
  # `ready` (blocking-recv sender-wake proof, #102 family). Same embedding
  # pattern as app-host: built first, handed to execd via EXECD_RECVWAKE_ELF.
  build_service recv-wake-probe
  set_env_var "EXECD_RECVWAKE_ELF" "$(service_elf_path recv-wake-probe)"

  if [[ -z "${INIT_LITE_SERVICE_LIST:-}" ]]; then
    INIT_LITE_SERVICE_LIST="$(scripts/discover-services.sh --list | paste -sd, -)"
    export INIT_LITE_SERVICE_LIST
  fi

  # TASK-0321 (RFC-0089 §12, ADR-0060): services listed in
  # scripts/system-volume-services.txt ship as bundles on the verified system
  # volume and — NEXUS_VOLUME_SPAWN=1, the P2 default — leave the embedded
  # init-lite table (init spawns them from the volume after the MMIO grants).
  # NEXUS_VOLUME_SPAWN=0 keeps them embedded as well (bring-up escape only).
  if [[ "${NEXUS_VOLUME_SPAWN:-1}" == "1" ]]; then
    local keep=()
    IFS=',' read -r -a _all <<<"$INIT_LITE_SERVICE_LIST"
    for raw in "${_all[@]}"; do
      local s=${raw//[[:space:]]/}
      [[ -z "$s" ]] && continue
      if grep -qx "$s" <(sed -e 's/#.*//' -e '/^\s*$/d' scripts/system-volume-services.txt); then
        echo "[build] $s spawns from the system volume (NEXUS_VOLUME_SPAWN=1)" >&2
        continue
      fi
      keep+=("$s")
    done
    INIT_LITE_SERVICE_LIST="$(IFS=','; echo "${keep[*]}")"
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
    build_service "$svc"
    set_env_var "INIT_LITE_SERVICE_${svc_upper}_ELF" "$(service_elf_path "$svc")"
    # stack_pages is manifest data (discover-services.sh); 0 = the 8-page default.
    local stack_pages
    stack_pages=$(service_stack_pages "$svc")
    [[ "$stack_pages" -gt 0 ]] || stack_pages=8
    set_env_var "INIT_LITE_SERVICE_${svc_upper}_STACK_PAGES" "$stack_pages"
  done
}

# ---------------------------------------------------------------------------
# prepare_system_bundles — TASK-0321 (RFC-0089 §12, ADR-0060): every service in
# scripts/system-volume-services.txt becomes a `.nxb` bundle directory under
# build/system-bundles/<svc>/ (ADR-0020 layout: manifest.nxb via nxb-pack from
# a generated TOML, payload.elf = the cross-compiled service, meta/launch.json =
# stack pages; the global pointer is derived from the ELF by `nx image`).
# The launcher hands the directory to `nx image build --system-bundles`.
# ---------------------------------------------------------------------------
prepare_system_bundles() {
  local list="scripts/system-volume-services.txt"
  [[ -f "$list" ]] || return 0
  local out_root="$ROOT/build/system-bundles"
  # TASK-0321 P3/P4: the NEXT set = the factory set with ONE bundle bumped
  # (metricsd@1.0.1). The `bundle-set.nxs` fixture (ota-bundle lane) is
  # built from it with `--reuse-from` the factory set, so it ships only the
  # changed bundle and boot 2 must REUSE every other one from the active
  # volume (`updated: bundle reused`) before spawning metricsd@1.0.1.
  local next_root="$ROOT/build/system-bundles-next"
  rm -rf "$out_root" "$next_root"
  local -a volume_services=()
  mapfile -t volume_services < <(sed -e 's/#.*//' -e '/^\s*$/d' "$list")
  [[ "${#volume_services[@]}" -eq 0 ]] && return 0
  local nxb_pack="$TARGET_ROOT/release/nxb-pack"
  (cd "$ROOT" && env -u RUSTFLAGS -u CARGO_ENCODED_RUSTFLAGS \
    cargo build --release -p nxb-pack >/dev/null)
  for svc in "${volume_services[@]}"; do
    # Same resolver + keyed artifact as the embedded table (build truth):
    # always let cargo decide (incremental — a no-op when nothing changed;
    # TASK-0043 P2: a `[[ ! -f ]]` guard once froze the first build forever).
    build_service "$svc"
    local elf_path features stack_pages
    elf_path=$(service_elf_path "$svc")
    features=$(service_cargo_features "$svc")
    stack_pages=$(service_stack_pages "$svc")
    [[ "$stack_pages" -gt 0 ]] || stack_pages=8
    local root ver
    for root in "$out_root" "$next_root"; do
      ver="1.0.0"
      [[ "$root" == "$next_root" && "$svc" == "metricsd" ]] && ver="1.0.1"
      local dir="$root/$svc"
      mkdir -p "$dir/meta"
      cat >"$dir/manifest.toml" <<EOF_TOML
name = "$svc"
version = "$ver"
abilities = ["service"]
caps = []
min_sdk = "0.1.0"
bundle_type = "service"
EOF_TOML
      "$nxb_pack" --toml "$dir/manifest.toml" "$elf_path" "$dir" >/dev/null
      rm -f "$dir/manifest.toml"
      printf '{ "stack_pages": %s, "features": "%s" }\n' "$stack_pages" "$features" >"$dir/meta/launch.json"
      echo "[build] system bundle $svc@$ver -> $dir (stack_pages=$stack_pages features=$features)" >&2
    done
  done
  prepare_app_bundles "$out_root" "$next_root"
}

# prepare_app_bundles — TASK-0321 P5: every ui-program app project under
# userspace/apps/ becomes a NON-spawnable bundle on the system volume:
# manifest.nxb (nxb-pack from the app's manifest.toml), payload.elf = the
# canonical ui-program bytes (`nx app compile`), meta/app.properties = the
# registry sidecar (label/icon/bundle_type) bundlemgrd reads. Plus the
# `system` data bundle (pkg:/system/build.prop). Nothing app-related is
# baked into the boot image any more. Identical content in both roots.
prepare_app_bundles() {
  local out_root="$1" next_root="$2"
  local nxb_pack="$TARGET_ROOT/release/nxb-pack"
  local nx_bin="$TARGET_ROOT/release/nx"
  (cd "$ROOT" && env -u RUSTFLAGS -u CARGO_ENCODED_RUSTFLAGS \
    cargo build --release -p nx >/dev/null)
  local tmp="$ROOT/build/app-bundles.tmp"
  rm -rf "$tmp"; mkdir -p "$tmp"
  local app_dir name
  for app_dir in "$ROOT"/userspace/apps/*/; do
    [[ -f "$app_dir/manifest.toml" ]] || continue
    grep -qE '^payload_kind *= *"ui-program"' "$app_dir/manifest.toml" || continue
    name=$(sed -nE 's/^name *= *"([^"]+)".*/\1/p' "$app_dir/manifest.toml" | head -1)
    [[ -n "$name" ]] || continue
    "$nx_bin" app compile --app "$app_dir" --out "$tmp/$name.payload" \
      --meta "$tmp/$name.properties" >/dev/null
    local root
    for root in "$out_root" "$next_root"; do
      local dir="$root/$name"
      mkdir -p "$dir/meta"
      "$nxb_pack" --toml "$app_dir/manifest.toml" "$tmp/$name.payload" "$dir" >/dev/null
      cp "$tmp/$name.properties" "$dir/meta/app.properties"
    done
    echo "[build] app bundle $name -> $out_root/$name" >&2
  done
  # `system`: the build.prop data bundle (pkg:/system/build.prop); its payload
  # IS build.prop (nxb-pack demands a payload; nothing executes it).
  printf 'ro.nexus.build=dev\n' >"$tmp/build.prop"
  cat >"$tmp/system.toml" <<EOF_TOML
name = "system"
version = "1.0.0"
# The manifest contract needs at least one ability; this bundle publishes build info.
abilities = ["system.BuildInfo"]
caps = []
min_sdk = "0.1.0"
bundle_type = "library"
EOF_TOML
  local root
  for root in "$out_root" "$next_root"; do
    local dir="$root/system"
    mkdir -p "$dir"
    "$nxb_pack" --toml "$tmp/system.toml" "$tmp/build.prop" "$dir" >/dev/null
    cp "$tmp/build.prop" "$dir/build.prop"
  done
  echo "[build] system bundle system@1.0.0 -> $out_root/system (build.prop)" >&2
  rm -rf "$tmp"
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

  # TASK-0289 A4: first-stage loader (verifies the slot before any OS code
  # runs; its linker script hard-asserts the 256 KiB budget, ADR-0059).
  require_or_build "$NXBOOT_ELF" "boot:nxboot" -- env RUSTFLAGS="$RUSTFLAGS_OS" cargo build -p nxboot --target "$TARGET" --release
}

# ---------------------------------------------------------------------------
# build_all — full build pipeline (services → init-lite → kernel with EMBED_INIT_ELF)
# ---------------------------------------------------------------------------
build_all() {
  prepare_build_tmpdir
  prepare_service_payloads
  prepare_system_bundles
  build_kernel_and_init
  # Build truth: every bundle carries the features the SSOT resolved, and the
  # ELF really contains them (gpud prints its feature set as its first line).
  scripts/check-bundle-provenance.sh
  # Post-build artifact verification
  if [[ ! -f "$KERNEL_ELF" ]]; then
    echo "[error] build.sh: kernel ELF not produced: $KERNEL_ELF" >&2
    exit 1
  fi
  if [[ ! -f "$INIT_ELF" ]]; then
    echo "[error] build.sh: init-lite ELF not produced: $INIT_ELF" >&2
    exit 1
  fi
  if [[ ! -f "$NXBOOT_ELF" ]]; then
    echo "[error] build.sh: nxboot ELF not produced: $NXBOOT_ELF" >&2
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
