#!/usr/bin/env bash
# Copyright 2024 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
# The virgl GPU bringup (3D context + shader/draw/gradient selftests + GL
# scanout) adds boot time; the stock 90s default cuts the marker ladder a few
# lines short (late services like metricsd miss their ready line). Widen only
# the default — an explicit RUN_TIMEOUT always wins.
if [[ -z "${RUN_TIMEOUT:-}" && "${GPU_MODE:-}" == "virgl" ]]; then
  RUN_TIMEOUT=240s
fi
# TASK-0050 reset lane: ONE run carries TWO boots (guest SBI reboot mid-run),
# so the wall clock covers boot 1 up to the reset plus a full ladder.
if [[ -z "${RUN_TIMEOUT:-}" && "${PROFILE:-}" == "reset" ]]; then
  RUN_TIMEOUT=360s
  export QEMU_READY_GRACE_SECS="${QEMU_READY_GRACE_SECS:-330}"
fi
# TASK-0289-B ota-fallback: FOUR boots in one uart (stage + two bricked
# trials the watcher power-cycles + the loader's exhaustion flip).
if [[ -z "${RUN_TIMEOUT:-}" && "${PROFILE:-}" == "ota-fallback" ]]; then
  RUN_TIMEOUT=600s
  export QEMU_READY_GRACE_SECS="${QEMU_READY_GRACE_SECS:-570}"
fi
# Re-measured 2026-08-20: the reliability-spine proofs grew the ladder's END
# phase (3-cycle restart storm with real backoffs, evidence journal proofs,
# bootctld bring-up) past the old 90s wall-clock cap — the outer timeout cut
# the storm's third cycle deterministically. 180s outer cap pairs with the
# launcher's 150s ready-grace (early-stop still ends green runs promptly).
# 180s carried the service ladder until TASK-0140/0034 added the updates
# read-surface probe (cold data-volume mount) and the delta lane (base
# deny + a real reconstruction with COPY readback) — honest new work, the
# ladder now finishes at ~180-200s under TCG+icount and two runs died at
# DIFFERENT late phases purely on the clock.
# 2026-09-04: boot (~36 s) + the launcher's 200 s ready-grace must fit; the
# early stop ends green runs long before this bound.
RUN_TIMEOUT=${RUN_TIMEOUT:-300s}
RUN_UNTIL_MARKER=${RUN_UNTIL_MARKER:-1}
RUN_PHASE=${RUN_PHASE:-}
NEXUS_FORCE_WORKSPACE_TARGET=${NEXUS_FORCE_WORKSPACE_TARGET:-1}
if [[ "$NEXUS_FORCE_WORKSPACE_TARGET" == "1" ]]; then
  export CARGO_TARGET_DIR="$ROOT/target"
fi

# ---------------------------------------------------------------------------
# TASK-0023B P4-05: harness profile dispatch.
#
# `--profile=<name>` reads `[profile.<name>]` from the proof-manifest
# (P5-00: now a v2 split tree under
# `source/apps/selftest-client/proof-manifest/`; root is
# `manifest.toml`) via the host CLI
# (`nexus-proof-manifest list-env --profile=<name>`) and forwards the
# resolved env dictionary to the rest of this script. Bash arrays
# (`expected_sequence`, `PHASES`, `PHASE_START_MARKER`) currently mirror
# the manifest; a mirror-check (`pm_mirror_check`) fails fast on drift.
# Full array-purge migration lands incrementally in P4-06+.
# ---------------------------------------------------------------------------
PROFILE=""
PM_CLI_DEFAULT="$ROOT/target/debug/nexus-proof-manifest"
NEXUS_PROOF_MANIFEST_BIN=${NEXUS_PROOF_MANIFEST_BIN:-}
# P5-00: the legacy single-file `proof-manifest.toml` has been deleted;
# point at the v2 root manifest under `proof-manifest/manifest.toml`.
# Override via NEXUS_PROOF_MANIFEST_PATH if you need to test against an
# alternative tree (e.g. a vendor fork).
NEXUS_PROOF_MANIFEST_PATH=${NEXUS_PROOF_MANIFEST_PATH:-"$ROOT/source/apps/selftest-client/proof-manifest/manifest.toml"}
PM_MIRROR_CHECK=${PM_MIRROR_CHECK:-1}

# Pre-scan argv for `--profile=<name>`; preserve the rest for downstream
# parsers (RUN_PHASE etc still come from env per legacy contract).
new_args=()
for arg in "$@"; do
  case "$arg" in
    --profile=*) PROFILE="${arg#--profile=}" ;;
    *) new_args+=("$arg") ;;
  esac
done
if [[ ${#new_args[@]} -gt 0 ]]; then
  set -- "${new_args[@]}"
else
  set --
fi
if [[ -n "$PROFILE" ]]; then
  PROFILE=${PROFILE,,}
fi

# Resolve the CLI binary lazily (build on first invocation if missing).
pm_cli() {
  if [[ -n "$NEXUS_PROOF_MANIFEST_BIN" && -x "$NEXUS_PROOF_MANIFEST_BIN" ]]; then
    echo "$NEXUS_PROOF_MANIFEST_BIN"
    return 0
  fi
  if [[ -x "$PM_CLI_DEFAULT" ]]; then
    # #region agent log
    if declare -F agent_debug_log >/dev/null 2>&1; then
      agent_debug_log "${RUN_ID:-qemu-preinit}" "H5" "scripts/qemu-test.sh:pm-cli-select" "selected proof-manifest CLI from workspace target" \
        "{\"path\":\"$PM_CLI_DEFAULT\",\"source\":\"workspace-target\"}"
    fi
    # #endregion
    echo "$PM_CLI_DEFAULT"
    return 0
  fi
  # Try cargo-target candidates produced by sandboxed builds.
  for cand in \
    "$ROOT/target/debug/nexus-proof-manifest" \
    /tmp/cursor-sandbox-cache/*/cargo-target/debug/nexus-proof-manifest; do
    if [[ -x "$cand" ]]; then
      # #region agent log
      if declare -F agent_debug_log >/dev/null 2>&1; then
        agent_debug_log "${RUN_ID:-qemu-preinit}" "H5" "scripts/qemu-test.sh:pm-cli-select" "selected proof-manifest CLI from sandbox cache" \
          "{\"path\":\"$cand\",\"source\":\"sandbox-cache\"}"
      fi
      # #endregion
      echo "$cand"
      return 0
    fi
  done
  echo "[info] building nexus-proof-manifest CLI..." >&2
  # One pinned toolchain (rust-toolchain.toml) — no floating `+stable`: the
  # harness gate must not build the manifest CLI with a different compiler
  # than the rest of the tree (same rule as `just check` / `scripts/build.sh`).
  (cd "$ROOT" && cargo build -p nexus-proof-manifest --bin nexus-proof-manifest --quiet) 1>&2
  for cand in \
    "$ROOT/target/debug/nexus-proof-manifest" \
    /tmp/cursor-sandbox-cache/*/cargo-target/debug/nexus-proof-manifest; do
    if [[ -x "$cand" ]]; then
      # #region agent log
      if declare -F agent_debug_log >/dev/null 2>&1; then
        agent_debug_log "${RUN_ID:-qemu-preinit}" "H5" "scripts/qemu-test.sh:pm-cli-select" "selected proof-manifest CLI after build" \
          "{\"path\":\"$cand\",\"source\":\"post-build-scan\"}"
      fi
      # #endregion
      echo "$cand"
      return 0
    fi
  done
  echo "[error] could not locate nexus-proof-manifest binary" >&2
  return 1
}

# Source env from the manifest profile chain (extends-aware).
#
# The profile is the SINGLE SOURCE OF TRUTH for lane topology (harts, icount,
# REQUIRE_* gates). A caller-exported value for a manifest-declared key used
# to be silently clobbered here, which let a lane lie about what it ran:
# `ci-os-smp1` passed `SMP=1` (deterministic 1 hart + icount by name and by
# comment) and got the profile's `SMP=2 QEMU_NO_ICOUNT=1` — i.e. `test-all`'s
# "deterministic boot gate" was really the 2-hart MTTCG lane, and it
# inherited that lane's nondeterministic cpu1 bring-up (the flaky
# `KSELFTEST: runtime timer budget ok`). A caller value that CONTRADICTS the
# profile is therefore a hard stop: declare the lane you want as a profile.
# NEXUS_PROFILE_ENV_OVERRIDE=1 keeps the caller's value (loudly) for a local
# one-off — never in a gate, never in CI.
#
# Note: keys the manifest can declare must not be defaulted by this script
# before this function runs, otherwise the script's own default reads as
# "caller intent" here.
pm_apply_profile_env() {
  local profile=$1
  local cli
  cli=$(pm_cli) || return 1
  local env_lines
  if ! env_lines=$("$cli" list-env --profile="$profile" --manifest="$NEXUS_PROOF_MANIFEST_PATH" 2>&1); then
    echo "[error] proof-manifest list-env failed for profile '$profile':" >&2
    echo "$env_lines" >&2
    return 1
  fi
  local conflicts=0
  while IFS='=' read -r k v; do
    [[ -z "$k" ]] && continue
    # Strip surrounding single-quotes added by `shell_quote`.
    if [[ "$v" == \'*\' ]]; then
      v="${v#\'}"
      v="${v%\'}"
    fi
    if [[ -n "${!k+x}" && "${!k}" != "$v" ]]; then
      if [[ "${NEXUS_PROFILE_ENV_OVERRIDE:-0}" == "1" ]]; then
        echo "[warn] proof-manifest($profile): caller override KEPT $k=${!k} (profile declares '$v')" >&2
        continue
      fi
      echo "[error] proof-manifest($profile): env conflict for $k — caller='${!k}', profile declares '$v'" >&2
      conflicts=$((conflicts + 1))
      continue
    fi
    export "$k=$v"
    echo "[info] proof-manifest($profile): $k=$v" >&2
  done <<< "$env_lines"
  if (( conflicts > 0 )); then
    cat >&2 <<EOF
[error] $conflicts profile-env conflict(s) for profile=$profile — refusing to run a
        lane that would not match its own declaration. The proof-manifest
        profile owns lane topology; pick the profile that declares what you
        want instead of overriding it:
          --profile=smp1   deterministic 1 hart + icount (the test-all gate)
          --profile=smp    2 harts, MTTCG, secondary-hart proofs required
        Profiles live in source/apps/selftest-client/proof-manifest/profiles/.
        NEXUS_PROFILE_ENV_OVERRIDE=1 forces the caller's value for a local
        one-off (never in CI or a gate).
EOF
    return 1
  fi
}

# Mirror-check disabled — manifest is the single source of truth.
# pm_mirror_check (lines 131-157) and expected_sequence (lines 366-532)
# have been removed. Use `nexus-proof-manifest verify-uart` instead.
# See P4-06+: "Full array-purge migration" in manifest profiles.
# below for the `full` profile) matches the manifest's expected-marker
# projection for the same profile. Drift = hard fail.
pm_mirror_check() {
  local profile=${1:-full}
  [[ "$PM_MIRROR_CHECK" != "1" ]] && return 0
  local cli
  cli=$(pm_cli) || return 1
  local manifest_seq
  manifest_seq=$("$cli" list-markers --profile="$profile" --manifest="$NEXUS_PROOF_MANIFEST_PATH") || return 1
  local script_seq
  script_seq=$(printf '%s\n' "${expected_sequence[@]}")
  # Manifest is a superset (it also tracks diagnostic / FAIL markers that
  # qemu-test.sh does not gate on). Drift check: every script-side marker
  # MUST appear in the manifest's `full` projection.
  local missing
  missing=$(comm -23 <(printf '%s\n' "$script_seq" | sort -u) <(printf '%s\n' "$manifest_seq" | sort -u))
  if [[ -n "$missing" ]]; then
    echo "[error] proof-manifest mirror-check: in-script expected_sequence has markers absent from manifest profile=$profile:" >&2
    printf '         %s\n' $missing >&2
    return 1
  fi
}

if [[ -n "$PROFILE" ]]; then
  pm_apply_profile_env "$PROFILE" || exit 1
fi

REQUIRE_SMP=${REQUIRE_SMP:-0}
QEMU_LOG_MAX=${QEMU_LOG_MAX:-52428800}
UART_LOG_MAX=${UART_LOG_MAX:-10485760}
# Semantic log directory: profile + iso8601 timestamp.
LOG_PROFILE=${PROFILE:-full}
LOG_DIR=${LOG_DIR:-"$ROOT/build/logs/$LOG_PROFILE--$(date +%Y-%m-%dT%H-%M-%S)"}
mkdir -p "$LOG_DIR"
# Symlink immediately so agents find the latest run even on failure.
ln -sfn "$LOG_DIR" "$ROOT/build/logs/latest" 2>/dev/null || true
HYPOTHESIS_LOG=${HYPOTHESIS_LOG:-$LOG_DIR/hypothesis.json}
RUN_ID=${RUN_ID:-"qemu-$(date +%s)-$$"}
export LOG_DIR HYPOTHESIS_LOG RUN_ID

# Log files go into the per-run directory, not repo root.
UART_LOG=${UART_LOG:-$LOG_DIR/uart.log}
QEMU_LOG=${QEMU_LOG:-$LOG_DIR/qemu.stderr}

# grep -c exits 1 on zero matches, so `$(grep -c ... || echo 0)` produces "0\n0".
# Use this helper instead: strips newlines, defaults to "0" when empty.
count_lines() {
  local n
  n=$(grep -aFc "$1" "$UART_LOG" 2>/dev/null | tr -d '\n' || true)
  printf '%s' "${n:-0}"
}

# #region agent log (ndjson hypothesis writer)
agent_debug_log() {
  local run_id=$1
  local hypothesis_id=$2
  local location=$3
  local message=$4
  local data_json=${5:-"{}"}
  local ts
  ts=$(date +%s%3N 2>/dev/null || date +%s000)
  # Strip embedded newlines from data_json — grep -c / wc -l output contains \n.
  data_json=$(printf '%s' "$data_json" | tr -d '\n\r')
  printf '{"runId":"%s","hypothesisId":"%s","location":"%s","message":"%s","data":%s,"timestamp":%s}\n' \
    "$run_id" "$hypothesis_id" "$location" "$message" "$data_json" "$ts" >>"$HYPOTHESIS_LOG" 2>/dev/null || true
}
# #endregion agent log

# #region agent log (session-specific input debug helper — delegates to agent_debug_log)
agent_input_debug_log() {
  agent_debug_log "$@"
}
# #endregion agent log

# #region agent log (always-on exit summary; Slice B)
agent_on_exit() {
  local code=$?
  local saw_init=false
  local dhcp_bound=false
  local dhcp_fallback=false
  if [[ -f "$UART_LOG" ]]; then
    if grep -aFq "init: start" "$UART_LOG"; then saw_init=true; fi
    if grep -aFq "net: dhcp bound" "$UART_LOG"; then dhcp_bound=true; fi
    if grep -aFq "net: dhcp unavailable (fallback static" "$UART_LOG"; then dhcp_fallback=true; fi
  fi
  agent_debug_log "$RUN_ID" "A" "scripts/qemu-test.sh:exit" "qemu smoke exit summary" \
    "{\"exit_code\":$code,\"saw_init_start\":$saw_init,\"dhcp_bound\":$dhcp_bound,\"dhcp_fallback\":$dhcp_fallback}"
}
trap agent_on_exit EXIT
# #endregion agent log

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

rm -f "$UART_LOG" "$QEMU_LOG"

# QEMU smoke harness builds `netstackd` in "qemu-smoke" mode unless overridden.
# This keeps single-VM bring-up deterministic (DSoftBus loopback) even if slirp DHCP is flaky.
if [[ -z "${INIT_LITE_SERVICE_NETSTACKD_CARGO_FLAGS:-}" ]]; then
  export INIT_LITE_SERVICE_NETSTACKD_CARGO_FLAGS="--no-default-features --features os-lite,qemu-smoke"
fi

# TASK-0014/TASK-0057: enforce the canonical os-lite service payload set for
# deterministic QEMU proofs from cargo metadata. The order policy lives in
# `scripts/discover-services.sh`; do not duplicate a static list here.
INIT_LITE_SERVICE_LIST="$(scripts/discover-services.sh --list | paste -sd, -)"
export INIT_LITE_SERVICE_LIST
if [[ -z "${INIT_LITE_SERVICE_METRICSD_STACK_PAGES:-}" ]]; then
  # Keep added observability service footprint bounded in bring-up proofs.
  export INIT_LITE_SERVICE_METRICSD_STACK_PAGES=1
fi
for svc in HIDRAWD TOUCHD INPUTD FBDEVD; do
  stack_var="INIT_LITE_SERVICE_${svc}_STACK_PAGES"
  if [[ -z "${!stack_var:-}" ]]; then
    # Input service entries perform bounded init-only proof work; keep their stacks small.
    export "$stack_var=1"
  fi
done
# #region agent log (H1: qemu-test effective config in make path)
agent_debug_log "$RUN_ID" "H1" "scripts/qemu-test.sh:effective-config" "effective flags/env before qemu run" \
  "{\"run_timeout\":\"$RUN_TIMEOUT\",\"run_until_marker\":\"$RUN_UNTIL_MARKER\",\"run_phase\":\"${RUN_PHASE:-}\",\"require_smp\":\"${REQUIRE_SMP:-0}\",\"smp\":\"${SMP:-}\",\"makelevel\":\"${MAKELEVEL:-}\",\"mode\":\"${MODE:-}\",\"qemu_session_mode\":\"${QEMU_SESSION_MODE:-proof}\",\"qemu_marker_level\":\"${QEMU_MARKER_LEVEL:-proof}\",\"guest_selftest_mode\":\"${NEXUS_SELFTEST_MODE:-}\",\"guest_selftest_profile\":\"${NEXUS_SELFTEST_PROFILE:-}\",\"qemu_icount_args\":\"${QEMU_ICOUNT_ARGS:-}\",\"netstackd_flags\":\"${INIT_LITE_SERVICE_NETSTACKD_CARGO_FLAGS:-}\",\"service_list\":\"${INIT_LITE_SERVICE_LIST:-}\",\"cargo_target_dir\":\"${CARGO_TARGET_DIR:-}\",\"hypothesis_log\":\"$HYPOTHESIS_LOG\",\"log_dir\":\"$LOG_DIR\"}"
# #endregion

QEMU_EXTRA_ARGS=()
if [[ "${DEBUG_QEMU:-0}" == "1" ]]; then
  QEMU_EXTRA_ARGS+=(-S -gdb tcp:localhost:1234)
fi


# RFC-0014 Phase 2: phase mapping for QEMU smoke triage + early exit.
# A "phase" is a named slice of the marker ladder. Failures should report the first failing phase.
declare -a PHASES=(
  "bring-up"
  "input-startup"
  "mmio"
  "routing"
  "ota"
  "policy"
  "logd"
  "vfs"
  "end"
)
declare -A PHASE_START_MARKER=(
  ["bring-up"]="init: start"
  ["input-startup"]="init: start"
  ["mmio"]="execd: ready"
  ["routing"]="SELFTEST: ipc routing keystored ok"
  ["ota"]="SELFTEST: ota stage ok"
  ["policy"]="SELFTEST: policy allow ok"
  ["logd"]="logd: ready"
  ["vfs"]="SELFTEST: vfs stat ok"
)
declare -A PHASE_END_MARKER=(
  ["bring-up"]="execd: ready"
  ["input-startup"]="inputd: os service payload ready"
  ["mmio"]="SELFTEST: cap query vmo ok"
  ["routing"]="SELFTEST: ipc routing ok"
  ["ota"]="SELFTEST: ota rollback ok"
  ["policy"]="SELFTEST: policy malformed ok"
  ["logd"]="SELFTEST: log query ok"
  ["vfs"]="SELFTEST: vfs ebadf ok"
  ["end"]="SELFTEST: end"
)

# hidrawd emits payload-ready unconditionally at service start (device probe
# happens later), so all three input services belong in the base ladder.
declare -a INPUT_STARTUP_MARKERS=(
  "hidrawd: os service payload ready"
  "touchd: os service payload ready"
  "inputd: os service payload ready"
)

find_marker_index() {
  local needle=$1
  shift
  local -a arr=("$@")
  local i
  for i in "${!arr[@]}"; do
    if [[ "${arr[$i]}" == "$needle" ]]; then
      echo "$i"
      return 0
    fi
  done
  return 1
}

print_phase_help() {
  echo "[error] Unknown RUN_PHASE='$RUN_PHASE' (supported: $(printf "%s " "${PHASES[@]}"))" >&2
}

print_uart_excerpt() {
  local start_marker=$1
  local prev_marker=${2:-}
  local max_lines=${3:-220}

  echo "[info] --- uart excerpt (bounded, phase-scoped) ---" >&2
  echo "[info] start_marker='$start_marker' prev_marker='${prev_marker:-}'" >&2

  local start_line=""
  if [[ -n "$prev_marker" ]]; then
    start_line=$(grep -aFn "$prev_marker" "$UART_LOG" | head -n1 | cut -d: -f1 || true)
  fi
  if [[ -z "$start_line" && -n "$start_marker" ]]; then
    start_line=$(grep -aFn "$start_marker" "$UART_LOG" | head -n1 | cut -d: -f1 || true)
  fi
  if [[ -z "$start_line" ]]; then
    echo "[info] (no start marker found; showing last $max_lines lines)" >&2
    tail -n "$max_lines" "$UART_LOG" >&2 || true
    return 0
  fi
  local end_line=$((start_line + max_lines))
  awk -v s="$start_line" -v e="$end_line" 'NR>=s && NR<=e { print }' "$UART_LOG" >&2 || true
}

# Verify markers printed by the OS stack and selftest client. The harness waits
# for os-lite init to announce each service twice (`init: start <svc>` then
# `init: up <svc>`) before the packagefsd/vfsd readiness markers, the execd
# lifecycle markers, and the selftest tail. If os-lite userspace did not run,
# fall back to kernel selftest completion markers to avoid spurious failures
# during bring-up.
#
# For headless/display-gpu profiles, truncate before display-specific markers
# (windowd: systemui loaded, launcher:, SELFTEST: ui*) which require GTK.
expected_sequence=(
  "neuron vers."
  "KSELFTEST: vmo zero ok"
  "KSELFTEST: as map ok"
  "KSELFTEST: vm map ok"
  "KSELFTEST: vm unmap ok"
  "KSELFTEST: vm map reject ok"
  "KSELFTEST: w^x enforced"
  "KSELFTEST: spawn reasons ok"
  "KSELFTEST: resource sentinel ok"
  "KSELFTEST: cpuid tp ok"
  "KSELFTEST: cpuid fallback counterfactual ok"
  "KSELFTEST: sched affinity reject ok"
  "KSELFTEST: sched abi ok"
  "KSELFTEST: tlb shootdown skipped (smp=1)"
  "init: start"
  "init: start keystored"
  "init: up keystored"
  "init: start rngd"
  "init: up rngd"
  "init: start policyd"
  "init: up policyd"
  "init: start logd"
  "init: up logd"
  "init: start samgrd"
  "init: up samgrd"
  "init: start bundlemgrd"
  "init: up bundlemgrd"
  "init: start packagefsd"
  "init: up packagefsd"
  "init: start vfsd"
  "init: up vfsd"
  "init: start execd"
  "init: up execd"
  "init: start bootctld"
  "init: up bootctld"
  "init: ready"
  # TASK-0321 (ADR-0060): metricsd is the system-volume pilot — spawned in
  # the SECOND pass after the MMIO grants (block plane live), so its
  # start/up ladder rungs land after `init: ready`, not in the embedded loop.
  "init: start metricsd"
  "init: up metricsd"
  "init: start netstackd"
  "init: up netstackd"
  "init: start dsoftbusd"
  "init: up dsoftbusd"
  "init: start hidrawd"
  "init: up hidrawd"
  "init: start touchd"
  "init: up touchd"
  "init: start gpud"
  "init: up gpud"
  "init: start windowd"
  "init: up windowd"
  "init: start inputd"
  "init: up inputd"
  "init: start imed"
  "init: up imed"
  # Service readiness markers are emitted asynchronously by the spawned processes.
  # With the kernel `exec` loader path, init emits spawn markers first, then yields;
  # services report `*: ready` after `init: ready`.
  "keystored: ready"
  "rngd: ready"
  "policyd: ready"
  "abi-profile: ready (server=policyd|abi-filterd)"
  "samgrd: ready"
  "bundlemgrd: ready"
  "statefsd: journal v2 mounted (2PC)"
  # TASK-0027: the mem-backed mount is always meta-less, so every boot
  # announces the default (off) exactly once; the on-marker is coupled to
  # the roundtrip selftest by the TASK-0027 fake-green guard below.
  "statefsd: encryption off"
  "statefsd: ready"
  "statefsd: write hardening on (auth-envelope)"
  "updated: ready (bootctl client)"
  "packagefsd: ready"
  "packagefsd: mounted (system volume slot="
  "vfsd: ready"
  "vfsd: namespace ready"
  "execd: ready"
  "${INPUT_STARTUP_MARKERS[@]}"
  "timed: ready"
  "timed: walltime anchored"
  "imed: ready"
  "netstackd: ready"
  "net: virtio-net up"
  "SELFTEST: net iface ok"
  "net: smoltcp iface up"
  "blk: virtio-blk up"
  "logd: ready"
  "metricsd: ready"
  "bootctld: ready"
  "bootctld: target=normal next=none"
  "bundlemgrd: slot a active"
  "SELFTEST: ipc routing keystored ok"
  "SELFTEST: keystored v1 ok"
  "SELFTEST: qos ok"
  "SELFTEST: timed coalesce ok"
  "SELFTEST: imed reject foreign ok"
  "SELFTEST: ime v2 osk ok"
  "SELFTEST: ime v2 cjk jp ok"
  "SELFTEST: ime v2 candidates ok"
  "SELFTEST: ime ranking ok"
  "SELFTEST: ime ranking persist ok"
  "SELFTEST: settings watch ok"
  "SELFTEST: i18n switch ok"
  "SELFTEST: walltime rtc ok"
  "SELFTEST: clock tz ok"
  "rngd: mmio window mapped ok"
  "SELFTEST: rng entropy ok"
  "SELFTEST: rng entropy oversized ok"
  "SELFTEST: device key pubkey ok"
  "SELFTEST: device key private export rejected ok"
  "SELFTEST: statefs put ok"
  "SELFTEST: statefs unauthorized access rejected"
  "SELFTEST: statefs persist ok"
  # RFC-0072 amendment / TASK-0043 P1: statefsd refuses the put over the hard
  # byte quota (EDQUOTA) before the journal append; a delete frees room.
  "statefs: quota warn subject=0x52c6c4a34ffb3f69"
  "statefs: quota deny subject=0x52c6c4a34ffb3f69"
  "SELFTEST: quota deny ok"
  "SELFTEST: statefs auth put ok"
  "SELFTEST: statefs tamper deny ok"
  "SELFTEST: statefs rollback deny ok"
  # TASK-0026 journal v2: 2PC crash-atomicity + bounded compaction (the
  # compaction-done line is reopen-verified by statefsd before it is emitted).
  # The cold-boot verdict is boot-dependent (seeded on a fresh image, persist
  # ok only on a preserved-image second boot) and is gated by the TASK-0026
  # fake-green guard below, not by this sequence.
  "SELFTEST: statefs v2 crash-atomic ok"
  "statefsd: compaction done (gen="
  "SELFTEST: statefs v2 compact ok"
  # TASK-0027: enablement + plaintext roundtrip through an enrolled prefix
  # (put/get equality before AND after Sync+Reopen — the AEAD-verified
  # replay path in-VM).
  "SELFTEST: statefs enc roundtrip ok"
  "SELFTEST: device key persist ok"
  "SELFTEST: net tcp listen ok"
  "netstackd: facade up"
  "SELFTEST: ipc routing samgrd ok"
  "SELFTEST: samgrd v1 register ok"
  "SELFTEST: samgrd v1 lookup ok"
  "SELFTEST: samgrd v1 unknown ok"
  "SELFTEST: samgrd v1 malformed ok"
  "SELFTEST: ipc routing policyd ok"
  "SELFTEST: ipc routing bundlemgrd ok"
  "SELFTEST: ipc routing updated ok"
  "SELFTEST: bundlemgrd v1 list ok"
  "SELFTEST: bundlemgrd volume ok"
  "SELFTEST: bundlemgrd v1 malformed ok"
  # TASK-0315: virtioblkd owns the ONE GPT disk and serves partition-scoped
  # block IO; clients attach over IPC and the deny-by-default partition
  # gate is proven every boot.
  "virtioblkd: gpt ok (parts=7)"
  "virtioblkd: irq endpoint bound"
  "blk: irq completion on"
  "statefsd: virtio upgrade ok"
  "SELFTEST: blk cross-partition deny ok"
  # TASK-0321 (RFC-0089 §12, ADR-0060): the verified system volume —
  # bundlemgrd verifies the NXSV paired with the measured boot slot, init
  # spawns the pilot (metricsd) from it, and the selftest's ungranted READ
  # of system-a is denied by the op-aware gate.
  "bundlemgrd: system volume verified (slot="
  "init: spawn from volume svc=metricsd"
  "SELFTEST: blk system volume deny ok"
  # TASK-0198 Phase 1: device publisher trust anchor — a validly self-signed
  # archive whose publisher is not in policies/update-trust.toml must be
  # rejected BEFORE the happy-path stage (the pre-fix hole accepted any key
  # read from the archive itself).
  # TASK-0140: the Settings Updates page's READ surface — updated's
  # status/feed/check answer coherently against the boot authority
  # (state-neutral, probed before the deny lanes and the OTA cycle).
  "SELFTEST: updates surface ok"
  "updated: stage rejected (untrusted publisher)"
  "SELFTEST: updates trust reject ok"
  # TASK-0034 (RFC-0090) delta lane: the wrong-base stream rejects
  # `delta-base` BEFORE any write, then the real delta reconstructs from
  # the ACTIVE slot through the unchanged readback/NXBD-last tail.
  "updated: stage rejected (delta-base)"
  "SELFTEST: ota delta base deny ok"
  "updated: component boot-image-delta verified (build=fixt-dl"
  "SELFTEST: ota delta stage ok"
  # TASK-0007 OTA proof: stage → switch → health gate → rollback (userspace-only, non-persistent)
  "SELFTEST: ota stage ok"
  "bundlemgrd: slot b active"
  # TASK-0051: authority-side proof that the switch persisted before the ack
  "bootctld: switch scheduled (to=b)"
  "SELFTEST: ota switch ok"
  # TASK-0036-A health-commit v2: the deadline arms with the switch and the
  # commit fires only on the COMPLETE declared quorum (RFC-0089 §13).
  "bootctld: commit deadline armed"
  "SELFTEST: ota health ok"
  "bootctld: health quorum ok (2/2)"
  "SELFTEST: bootctl quorum ok"
  # TASK-0036-B: runtime BSB projection — the switch commit projects
  # next=b to the bsb partition and the selftest reads the synced tail.
  "bootctld: bsb sync (seq="
  "SELFTEST: bootctl bsb ok"
  "SELFTEST: ota rollback ok"
  "SELFTEST: bootctl persist ok"
  # TASK-0289 B1: the loader's measured record surfaced via bootctld and
  # cross-checked against the authority's active slot (qemu-soft-root).
  "SELFTEST: measured boot log ok"
  "SELFTEST: policy allow ok"
  "SELFTEST: policy deny ok"
  "abi-filter: deny (subject=selftest-client syscall=statefs.put)"
  "SELFTEST: abi filter deny ok"
  "SELFTEST: abi filter allow ok"
  "abi-filter: deny (subject=selftest-client syscall=net.bind)"
  "SELFTEST: abi netbind deny ok"
  # RFC-0091 / TASK-0028 P3: the real seam. statefsd's put asks policyd
  # (OP_ABI_EVAL); the mode switch is authenticated + epoch-guarded; a
  # refusal in Learn mode lands as an abi.learn record at logd.
  "SELFTEST: abi stale epoch reject ok"
  "policyd: abi mode subject=52c6c4a34ffb3f69 mode=learn epoch="
  "SELFTEST: abi enforce allow ok"
  "statefsd: abi deny path=/state/app/selftest/secrets/probe subject=0x52c6c4a34ffb3f69"
  "SELFTEST: abi enforce deny ok"
  "SELFTEST: abi learn collected ok"
  "policyd: abi mode subject=52c6c4a34ffb3f69 mode=enforce epoch="
  "SELFTEST: abi mode switch auth ok"
  # TASK-0043 P2: netstackd's connect/listen/bind seam is armed over the
  # init-wired policyd slots (RFC-0091 §7) — the boot witness for egress.
  "init: netstackd policy slots 7/8/9"
  "net-egress: enforced (netstackd policy seam on)"
  "SELFTEST: mmio policy deny ok"
  "SELFTEST: policyd requester spoof denied ok"
  "SELFTEST: policy malformed ok"
  "SELFTEST: init supervision sweep ok"
  "SELFTEST: supervision restart ok"
  "init: crash-loop blocked svc=fault-probe reason="
  "SELFTEST: crash-loop cap ok"
  "pinched: selftest crash requested"
  "init: service exit name=pinched reason=error"
  "init: supervision persist restarts=0x"
  "init: service restarted name=pinched"
  "SELFTEST: service restart ok"
  "SELFTEST: crash-loop count ok"
  "SELFTEST: ipc routing execd ok"
  "child: hello-elf"
  "execd: elf load ok"
  "SELFTEST: e2e exec-elf ok"
  "child: exit0 start"
  "execd: child exited"
  "SELFTEST: child exit ok"
  "execd: exit pid="
  "child: fault start"
  "SELFTEST: exit reason ok"
  "child: minidump start"
  "execd: crash report pid="
  "execd: minidump written"
  "SELFTEST: crash report ok"
  "SELFTEST: minidump ok"
  # TASK-0051B: canonical at-rest artifact (.nxcd) + retention pass
  "crash: retention gc on (budget=256KiB)"
  "crash: dump written"
  "SELFTEST: crash artifact ok"
  "SELFTEST: crash redaction ok"
  # TASK-0053: .nxra break-glass chain (require -> accept -> replay deny)
  "SELFTEST: nxra require ok"
  "bootctld: nxra accept (key=197f6b23 action=slot-switch)"
  "SELFTEST: nxra accept ok"
  "bootctld: nxra reject (reason=replay)"
  "SELFTEST: nxra replay deny ok"
  "SELFTEST: minidump forged metadata rejected"
  "SELFTEST: minidump no-artifact metadata rejected"
  "SELFTEST: minidump mismatched build_id rejected"
  "SELFTEST: exec denied ok"
  "SELFTEST: execd malformed ok"
  "logd: reject invalid_args"
  "logd: reject over_limit"
  "logd: reject rate_limited"
  "SELFTEST: logd hardening rejects ok"
  "metricsd: reject invalid_args"
  "metricsd: reject over_limit"
  "metricsd: reject rate_limited"
  "SELFTEST: metrics security rejects ok"
  "SELFTEST: metrics counters ok"
  "SELFTEST: metrics gauges ok"
  "SELFTEST: metrics histograms ok"
  "SELFTEST: tracing spans ok"
  "SELFTEST: metrics retention ok"
  "SELFTEST: log query ok"
  "SELFTEST: nexus-log sink-logd ok"
  "SELFTEST: core services log ok"
  "SELFTEST: evidence query ok"
  "SELFTEST: evidence budget ok"
  "SELFTEST: ipc payload roundtrip ok"
  "SELFTEST: ipc deadline timeout ok"
  "SELFTEST: nexus-ipc kernel loopback ok"
  "SELFTEST: ipc sender pid ok"
  "SELFTEST: ipc sender service_id ok"
  "SELFTEST: vm map roundtrip ok"
  "SELFTEST: cap query vmo ok"
  "SELFTEST: ipc routing ok"
  "SELFTEST: ipc routing packagefsd ok"
  "SELFTEST: vfs stat ok"
  "SELFTEST: vfs read ok"
  "SELFTEST: capfd read ok"
  "vfsd: capfd grant ok"
  "SELFTEST: vfs real data ok"
  "SELFTEST: vfs readdir ok"
  "SELFTEST: vfs readdir deny ok"
  "SELFTEST: vfs ebadf ok"
  "vfsd: access denied"
  "SELFTEST: sandbox deny ok"
  "windowd: ready (w=1280, h=800, hz=120)"
  "windowd: systemui loaded (profile=desktop)"
  "windowd: present ok (seq=1 dmg=1)"
  "launcher: first frame ok"
  "SELFTEST: ui launcher present ok"
  "SELFTEST: ui resize ok"
)

# Profile-aware truncation: remove display markers that require GTK/QMP.
# headless and display-gpu profiles skip visual output.
case "${PROFILE:-full}" in
  smp)
    expected_sequence=(
      "neuron vers."
      "KSELFTEST: spawn reasons ok"
      "KSELFTEST: resource sentinel ok"
      "KSELFTEST: cpuid tp ok"
      "KSELFTEST: cpuid fallback counterfactual ok"
      "KSELFTEST: sched affinity reject ok"
      "KSELFTEST: sched abi ok"
      "cpu1 online"
      "init: start"
      "init: ready"
    )
    ;;
  ota-flip)
    # TASK-0179 CROWN LANE: two boots in ONE uart. The guest runs the
    # reduced `ota-flip` phase scope, so the headless service ladder does
    # not apply — this list is the FLIP itself, end to end:
    #   boot 1  stage the real container -> switch -> reset
    #   loader  picks the new slot from the projected BSB (DIFFERENT build)
    #   boot 2  running the new slot -> quorum -> commit -> floor raised
    # The `slot=a` loader rungs are prepended globally (boot 1); the
    # `slot=b` rungs below are the proof that the flip happened BEFORE any
    # OS code ran.
    expected_sequence=(
      "neuron vers."
      "init: start"
      "init: ready"
      "updated: stage begin (source=/updates/os-B.nxs)"
      # TASK-0321 P4: a boot-image update carries its PAIRED volume — every
      # window unchanged, so the device reuses all of them from system-a.
      "updated: component system-volume verified (build=otaB"
      "updated: bundle reused (name=metricsd@1.0.0"
      "updated: component boot-image verified (build=otaB"
      "updated: stage done (slot=b build=otaB"
      "bootctld: switch scheduled (to=b)"
      "bootctld: bsb sync (seq="
      "SELFTEST: ota flip staged ok"
      "nxboot: tries 2->1 (slot=b trial)"
      "nxboot: verify ok (slot=b build=otaB"
      "nxboot: jump slot=b"
      "bundlemgrd: system volume verified (slot=b build=otaB"
      "init: spawn from volume svc=metricsd bundle=metricsd@1.0.0"
      "bootctld: health quorum ok (2/2)"
      "bootctld: commit ok (slot=b)"
      "bootctld: rollback-min raised ("
      "SELFTEST: ota flip ok"
    )
    # This lane runs the reduced bringup+end scope, so the shared OTA-phase
    # guards below (quorum/tamper/downgrade) describe a cycle it never
    # runs — they belong to the headless lane that owns that cycle.
    OTA_PHASE_GUARDS=0
    ;;
  ota-bundle)
    # TASK-0321 P3 BUNDLE-SET LANE (RFC-0089 §12, ADR-0060): the ota-flip
    # shape, but the set carries the SYSTEM VOLUME with the boot image:
    #   boot 1  stage bundle-set.nxs = boot-image(otaB) + system-volume +
    #           metricsd@1.0.1 into the inactive slot PAIR (index +
    #           bundle windows readback-verified, NXSV LAST) -> switch -> reset
    #   loader  picks slot b (different build id)
    #   boot 2  bundlemgrd verifies system-b PAIRED with the measured image,
    #           init spawns metricsd@1.0.1 FROM that volume, quorum commits.
    # The factory volume ships metricsd@1.0.0, so the version in the spawn
    # line is the proof the volume travelled with the flip.
    # Marker order = uart order: the volume/bundle lines print as each
    # component finishes; the boot-image line is the SET summary after the
    # whole verify (stage_os), so it follows them.
    expected_sequence=(
      "neuron vers."
      "init: start"
      "bundlemgrd: system volume verified (slot=a"
      "init: spawn from volume svc=metricsd bundle=metricsd@1.0.0"
      "init: ready"
      "updated: stage begin (source=/updates/bundle-set.nxs)"
      "updated: component system-volume verified (build=otaB"
      "updated: component bundle verified (name=metricsd@1.0.1)"
      "updated: bundle reused (name="
      "updated: component boot-image verified (build=otaB"
      "updated: stage done (slot=b build=otaB"
      "bootctld: switch scheduled (to=b)"
      "SELFTEST: ota bundle-set staged ok"
      "nxboot: verify ok (slot=b build=otaB"
      "nxboot: jump slot=b"
      "bundlemgrd: system volume verified (slot=b build=otaB"
      "init: spawn from volume svc=metricsd bundle=metricsd@1.0.1"
      "bootctld: health quorum ok (2/2)"
      "bootctld: commit ok (slot=b)"
      "SELFTEST: ota bundle-set ok"
    )
    OTA_PHASE_GUARDS=0
    # TASK-0321 P4: the set ships ONE changed bundle; every other volume
    # service must be REUSED from the active volume (count gate below).
    OTA_BUNDLE_REUSE_MIN=$(( $(sed -e 's/#.*//' -e '/^\s*$/d' "$ROOT/scripts/system-volume-services.txt" | wc -l) - 1 ))
    ;;
  ota-bundle-delta)
    # TASK-0035 P3 (RFC-0089 §12.4 kind 4): the ota-bundle shape, but the
    # changed bundle ships as an RFC-0090 `.nxdelta` against the ACTIVE
    # volume's window (`delta-base` bound); the engine reconstructs it
    # through the assembler's verified window path, every other bundle is
    # reused, boot 2 serves the reconstructed metricsd@1.0.1 from system-b.
    expected_sequence=(
      "neuron vers."
      "init: start"
      "init: ready"
      "updated: stage begin (source=/updates/bundle-delta.nxs)"
      "updated: component system-volume verified (build=otaB"
      "updated: component bundle verified (name=metricsd@1.0.1)"
      "updated: component bundle-delta reconstructed (name=metricsd@1.0.1)"
      "updated: bundle reused (name="
      "updated: component boot-image verified (build=otaB"
      "updated: stage done (slot=b build=otaB"
      "bootctld: switch scheduled (to=b)"
      "SELFTEST: ota bundle delta staged ok"
      "nxboot: verify ok (slot=b build=otaB"
      "bundlemgrd: system volume verified (slot=b build=otaB"
      "init: spawn from volume svc=metricsd bundle=metricsd@1.0.1"
      "bootctld: commit ok (slot=b)"
      "SELFTEST: ota bundle delta ok"
    )
    OTA_PHASE_GUARDS=0
    ;;
  ota-bundle-resume)
    # TASK-0035 P1 (RFC-0089 §12.2 NXSJ): the stage journal. Boot 1 stages
    # the bundle set; the launcher power-cuts the machine at the first
    # `updated: bundle reused` (journal entry durable before the marker).
    # Boot 2 stages the same set: the engine reads the journal, readback-
    # verifies every journalled window and only rewrites the rest.
    expected_sequence=(
      "neuron vers."
      "init: start"
      "init: ready"
      "updated: stage begin (source=/updates/bundle-set.nxs)"
      "updated: component bundle verified (name=metricsd@1.0.1)"
      "updated: bundle reused (name="
      "updated: restage resume (bundles="
      "updated: stage done (slot=b build=otaB"
      "SELFTEST: ota stage resume ok"
    )
    OTA_PHASE_GUARDS=0
    ;;
  ota-fallback)
    # TASK-0289-B: the loader's tries-exhaustion backstop — FOUR boots in
    # ONE uart, and the point is that boots 2/3 are DEAD userspace:
    #   boot 1  stage real os-B -> switch -> reset (same arming as flip)
    #   boots 2/3  trial slot b; init parks (fault fixture) — the QMP
    #              watcher power-cycles; the loader decrements 2->1, 1->0
    #   boot 4  loader reads tries=0 -> exhaustion flip back to slot a;
    #           bootctld OBSERVES the rollback at attach; verdict marker.
    expected_sequence=(
      "neuron vers."
      "init: start"
      "init: ready"
      "updated: stage done (slot=b build=otaB"
      "bootctld: switch scheduled (to=b)"
      "bootctld: bsb sync (seq="
      "SELFTEST: ota fallback staged ok"
      "nxboot: tries 2->1 (slot=b trial)"
      "init: health withheld (fault fixture)"
      "nxboot: tries 1->0 (slot=b trial)"
      "nxboot: fallback (slot=b exhausted) -> slot=a"
      "nxboot: verify ok (slot=a build=dev-"
      "nxboot: jump slot=a"
      "bootctld: rollback observed (trial exhausted)"
      "SELFTEST: ota fallback ok"
    )
    OTA_PHASE_GUARDS=0
    ;;
  headless|smp1|reset|display-gpu|dhcp|dhcp-strict|quic-required|os2vm|supply-chain|ota-tamper|ota-downgrade)
    # Use a reduced expected sequence for headless — omits display-gated
    # metrics, VFS, sandbox, and windowd markers. (The exec child-lifecycle/
    # minidump chain is NOT display-gated: it is appended for headless/smp1
    # below — TASK-0049 reanimation.)
    # `smp1` (deterministic 1 hart + icount) shares this ladder: it is the
    # headless service chain on one hart, so the `tlb shootdown skipped
    # (smp=1)` line below is its explicit "no secondary hart" proof.
    expected_sequence=(
      "neuron vers."
      "KSELFTEST: vmo zero ok"
      "KSELFTEST: as map ok"
      "KSELFTEST: vm map ok"
      "KSELFTEST: vm unmap ok"
      "KSELFTEST: vm map reject ok"
      "KSELFTEST: w^x enforced"
      "KSELFTEST: spawn reasons ok"
      "KSELFTEST: resource sentinel ok"
      "KSELFTEST: cpuid tp ok"
      "KSELFTEST: cpuid fallback counterfactual ok"
      "KSELFTEST: sched affinity reject ok"
      "KSELFTEST: sched abi ok"
      "KSELFTEST: tlb shootdown skipped (smp=1)"
      "init: start"
      "init: start keystored"
      "init: up keystored"
      "init: start rngd"
      "init: up rngd"
      "init: start policyd"
      "init: up policyd"
      "init: start logd"
      "init: up logd"
      "init: start samgrd"
      "init: up samgrd"
      "init: start bundlemgrd"
      "init: up bundlemgrd"
      "init: start packagefsd"
      "init: up packagefsd"
      "init: start vfsd"
      "init: up vfsd"
      "init: start execd"
      "init: up execd"
      "init: ready"
      # TASK-0321 (ADR-0060): metricsd is the system-volume pilot — spawned in
      # the SECOND pass after the MMIO grants (block plane live), so its
      # start/up ladder rungs land after `init: ready`, not in the embedded loop.
      "init: start metricsd"
      "init: up metricsd"
      "init: start netstackd"
      "init: up netstackd"
      "init: start dsoftbusd"
      "init: up dsoftbusd"
      "init: start hidrawd"
      "init: up hidrawd"
      "init: start touchd"
      "init: up touchd"
      "init: start gpud"
      "init: up gpud"
      "init: start windowd"
      "init: up windowd"
      "init: start inputd"
      "init: up inputd"
      "init: start imed"
      "init: up imed"
      "keystored: ready"
      "rngd: ready"
      "policyd: ready"
      "samgrd: ready"
      "bundlemgrd: ready"
      "statefsd: journal v2 mounted (2PC)"
      "statefsd: encryption off"
      "statefsd: ready"
      "updated: ready (bootctl client)"
      "packagefsd: ready"
      "packagefsd: mounted (system volume slot="
      "vfsd: ready"
      "execd: ready"
      "netstackd: ready"
      "net: virtio-net up"
      "SELFTEST: net iface ok"
      "net: smoltcp iface up"
      "logd: ready"
      "metricsd: ready"
      "bundlemgrd: slot a active"
      "SELFTEST: ipc routing keystored ok"
      "SELFTEST: keystored v1 ok"
      "SELFTEST: qos ok"
      "SELFTEST: timed coalesce ok"
      "SELFTEST: imed reject foreign ok"
      "SELFTEST: ime v2 osk ok"
      "SELFTEST: ime v2 cjk jp ok"
      "SELFTEST: ime v2 candidates ok"
      "SELFTEST: settings watch ok"
      "SELFTEST: i18n switch ok"
      "SELFTEST: walltime rtc ok"
      "SELFTEST: clock tz ok"
      "rngd: mmio window mapped ok"
      "SELFTEST: ipc routing samgrd ok"
      "SELFTEST: samgrd v1 register ok"
      "SELFTEST: samgrd v1 lookup ok"
      "SELFTEST: samgrd v1 unknown ok"
      "SELFTEST: samgrd v1 malformed ok"
      "SELFTEST: ipc routing policyd ok"
      "SELFTEST: ipc routing bundlemgrd ok"
      "SELFTEST: ipc routing updated ok"
      "SELFTEST: bundlemgrd v1 list ok"
      "SELFTEST: bundlemgrd volume ok"
      "SELFTEST: bundlemgrd v1 malformed ok"
      "virtioblkd: gpt ok (parts=7)"
      "virtioblkd: irq endpoint bound"
      "blk: irq completion on"
      "statefsd: virtio upgrade ok"
      "SELFTEST: blk cross-partition deny ok"
      "bundlemgrd: system volume verified (slot="
      "init: spawn from volume svc=metricsd"
      "SELFTEST: blk system volume deny ok"
      "SELFTEST: updates surface ok"
      "updated: stage rejected (untrusted publisher)"
      "SELFTEST: updates trust reject ok"
      "updated: stage rejected (delta-base)"
      "SELFTEST: ota delta base deny ok"
      "updated: component boot-image-delta verified (build=fixt-dl"
      "SELFTEST: ota delta stage ok"
      "SELFTEST: ota stage ok"
      "bundlemgrd: slot b active"
      "bootctld: switch scheduled (to=b)"
      "SELFTEST: ota switch ok"
      "bootctld: commit deadline armed"
      "SELFTEST: ota health ok"
      "bootctld: health quorum ok (2/2)"
      "SELFTEST: bootctl quorum ok"
      "bootctld: bsb sync (seq="
      "SELFTEST: bootctl bsb ok"
      "SELFTEST: ota rollback ok"
      "SELFTEST: bootctl persist ok"
      "SELFTEST: measured boot log ok"
      "SELFTEST: policy allow ok"
      "SELFTEST: policy deny ok"
      "SELFTEST: abi filter deny ok"
      "SELFTEST: abi filter allow ok"
      "SELFTEST: abi netbind deny ok"
      "SELFTEST: abi stale epoch reject ok"
      "SELFTEST: abi enforce allow ok"
      "SELFTEST: abi enforce deny ok"
      "SELFTEST: abi learn collected ok"
      "SELFTEST: abi mode switch auth ok"
      "SELFTEST: mmio policy deny ok"
      "SELFTEST: policyd requester spoof denied ok"
      "SELFTEST: policy malformed ok"
      "SELFTEST: init supervision sweep ok"
      "SELFTEST: supervision restart ok"
      "init: crash-loop blocked svc=fault-probe reason="
      "SELFTEST: crash-loop cap ok"
      "pinched: selftest crash requested"
      "init: service exit name=pinched reason=error"
      "init: supervision persist restarts=0x"
      "init: service restarted name=pinched"
      "SELFTEST: service restart ok"
      "SELFTEST: crash-loop count ok"
      "SELFTEST: ipc routing execd ok"
      "execd: elf load ok"
      "SELFTEST: e2e exec-elf ok"
    )
    ;;
esac

# TASK-0049 reanimation (2026-08-19): the exec/exit0/crash/minidump chain is
# hard-gated again on the profiles that run the full service ladder. The
# retirement-era masking was exactly this hole: the headless arm never listed
# the chain, so "children load but don't execute" stayed green. Only appended
# for headless/smp1 (network/display profiles may stop before the exec phase);
# the `full` profile carries the same gates in the base list above. Order is
# free — the strict-order loop only checks init:/KSELFTEST: markers.
case "${PROFILE:-full}" in
  headless|smp1|reset)
    expected_sequence+=(
      "child: hello-elf"
      "child: exit0 start"
      "execd: child exited"
      "SELFTEST: child exit ok"
      "execd: exit pid="
      "child: fault start"
      "SELFTEST: exit reason ok"
      "child: minidump start"
      "execd: crash report pid="
      "execd: minidump written"
      "SELFTEST: crash report ok"
      "SELFTEST: minidump ok"
      "crash: retention gc on (budget=256KiB)"
      "crash: dump written"
      "SELFTEST: crash artifact ok"
      "SELFTEST: crash redaction ok"
      "SELFTEST: nxra require ok"
      "bootctld: nxra accept (key=197f6b23 action=slot-switch)"
      "SELFTEST: nxra accept ok"
      "bootctld: nxra reject (reason=replay)"
      "SELFTEST: nxra replay deny ok"
      "SELFTEST: minidump forged metadata rejected"
      "SELFTEST: minidump no-artifact metadata rejected"
      "SELFTEST: minidump mismatched build_id rejected"
      "SELFTEST: exec denied ok"
      "SELFTEST: execd malformed ok"
      "SELFTEST: evidence query ok"
      "SELFTEST: evidence budget ok"
    )
    ;;
esac

# TASK-0023B P4-05: drift gate. The in-script `expected_sequence` above
# is a curated subset of the manifest projection for the active harness
# profile; if a marker the script gates on disappears from (or is renamed
# in) the manifest, we want to know NOW, not after a fleet failure.
# --- pm_mirror_check disabled (P4-06+). Manifest is sole truth.
# A9: proof profiles are deterministic at SMP=1 unless a gate sets SMP
# explicitly (ci-os-smp runs SMP=4); the interactive launcher default is 4.
export SMP="${SMP:-1}"

echo "[info] proof-manifest CLI resolved: $PM_CLI_DEFAULT" >&2
if [[ "$REQUIRE_SMP" == "1" ]]; then
  if [[ "${SMP:-1}" -lt 2 ]]; then
    echo "[error] REQUIRE_SMP=1 requires SMP>=2 (current SMP=${SMP:-1})" >&2
    exit 2
  fi
  smp_markers=(
    "KINIT: cpu1 online"
    "KSELFTEST: smp online ok"
    "KSELFTEST: kernel lock contention bounded ok"
    "KSELFTEST: ipi counterfactual ok"
    "KSELFTEST: ipi resched ok"
    "KSELFTEST: test_reject_invalid_ipi_target_cpu ok"
    "KSELFTEST: test_reject_offline_cpu_resched ok"
    "KSELFTEST: runtime ipi budget ok"
    "KSELFTEST: runtime stress ok"
    "KSELFTEST: work stealing ok"
    "KSELFTEST: test_reject_steal_above_bound ok"
    "KSELFTEST: test_reject_steal_higher_qos ok"
    "KSELFTEST: tlb shootdown counterfactual ok"
    "KSELFTEST: tlb shootdown ok"
    "KSELFTEST: sched affinity clamp ok"
    "KSELFTEST: plic ctx cpu0 ok"
    "KSELFTEST: plic ctx cpu1 ok"
    "KSELFTEST: plic isolation ok"
  )
  # Kernel SMP selftests run before userspace init markers (and after the
  # cpuid + sched-ABI selftests, positions 3-6 of every base list).
  expected_sequence=(
    "${expected_sequence[@]:0:7}"
    "${smp_markers[@]}"
    "${expected_sequence[@]:7}"
  )
fi

# TASK-0289 A4 (boot flip): every profile boots through the nxboot
# first-stage loader now — the loader rungs precede the kernel banner and
# the measured-handoff rung precedes every other KSELFTEST line (strict
# KSELFTEST ordering below relies on that).
# The verify-ok build id is launcher-derived (dev-<kernelsha8>) — matched
# as a prefix. The bsb-ok seq is a PREFIX too since TASK-0036-B: bootctld
# projects the record to the BSB at runtime, so seq grows across boots
# (keep-blk / reset lanes) — the deterministic part is the selected slot.
case "${PROFILE:-full}" in
  ota-tamper|ota-downgrade)
    # TASK-0289-B: the armed BSB points at the planted boot-b trial; the
    # LOADER must reject it with the stable reason and fall back — only
    # then does the ordinary slot-a ladder (already in the list) begin.
    if [[ "${PROFILE}" == "ota-tamper" ]]; then
      nxboot_fail_reason="nxboot: verify FAIL (slot=b digest)"
    else
      nxboot_fail_reason="nxboot: verify FAIL (slot=b rollback 0 < min 1)"
    fi
    expected_sequence=(
      "nxboot: bsb ok (slot=b"
      "nxboot: tries 2->1 (slot=b trial)"
      "$nxboot_fail_reason"
      "nxboot: fallback -> slot=a"
      "nxboot: verify ok (slot=a build=dev-"
      "nxboot: jump slot=a"
      "KSELFTEST: boot handoff ok (measured)"
      "${expected_sequence[@]}"
    )
    ;;
  *)
    expected_sequence=(
      "nxboot: bsb ok (slot=a"
      "nxboot: verify ok (slot=a build=dev-"
      "nxboot: jump slot=a"
      "KSELFTEST: boot handoff ok (measured)"
      "${expected_sequence[@]}"
    )
    ;;
esac

# Optional: stop and validate only up to a given phase.
if [[ -n "$RUN_PHASE" ]]; then
  if [[ -z "${PHASE_END_MARKER[$RUN_PHASE]:-}" ]]; then
    print_phase_help
    exit 2
  fi
  if [[ "$RUN_PHASE" == "input-startup" ]]; then
    expected_sequence=(
      "neuron vers."
      "KSELFTEST: spawn reasons ok"
      "KSELFTEST: resource sentinel ok"
      "KSELFTEST: cpuid tp ok"
      "KSELFTEST: cpuid fallback counterfactual ok"
      "KSELFTEST: sched affinity reject ok"
      "KSELFTEST: sched abi ok"
      "KSELFTEST: tlb shootdown skipped (smp=1)"
      "init: start"
      "init: start hidrawd"
      "init: up hidrawd"
      "init: start touchd"
      "init: up touchd"
      "init: start windowd"
      "init: up windowd"
      "init: start inputd"
      "init: up inputd"
      "${INPUT_STARTUP_MARKERS[@]}"
    )
  fi
  phase_end="${PHASE_END_MARKER[$RUN_PHASE]}"
  phase_end_idx=$(find_marker_index "$phase_end" "${expected_sequence[@]}" || true)
  if [[ -z "$phase_end_idx" ]]; then
    echo "[error] RUN_PHASE=$RUN_PHASE end_marker='$phase_end' not found in expected marker ladder" >&2
    exit 2
  fi

  # For RUN_PHASE runs, prefer marker-string early exit (run-qemu-rv64.sh supports this) unless
  # the user explicitly chose a different early-exit mode.
  if [[ "$RUN_UNTIL_MARKER" == "1" ]]; then
    RUN_UNTIL_MARKER="$phase_end"
  fi

  # Trim the expected marker ladder to the end of the requested phase.
  expected_sequence=( "${expected_sequence[@]:0:$((phase_end_idx + 1))}" )
  echo "[info] RUN_PHASE=$RUN_PHASE end_marker='$phase_end' (early-exit via RUN_UNTIL_MARKER='${RUN_UNTIL_MARKER}')" >&2
fi

# Execute QEMU (optionally stopping early at RUN_UNTIL_MARKER).
set +e
# #region agent log (H2: build artifact presence/freshness before run)
target_root="${CARGO_TARGET_DIR:-$ROOT/target}"
kernel_elf_path="$target_root/riscv64imac-unknown-none-elf/release/neuron-boot"
init_elf_path="$target_root/riscv64imac-unknown-none-elf/release/init-lite"
netstackd_elf_path="$target_root/riscv64imac-unknown-none-elf/release/netstackd"
kernel_exists=false
init_exists=false
netstackd_exists=false
kernel_size=0
init_size=0
netstackd_size=0
kernel_mtime=0
init_mtime=0
netstackd_mtime=0
if [[ -f "$kernel_elf_path" ]]; then
  kernel_exists=true
  kernel_size=$(wc -c <"$kernel_elf_path" 2>/dev/null || echo 0)
  kernel_mtime=$(stat -c %Y "$kernel_elf_path" 2>/dev/null || echo 0)
fi
if [[ -f "$init_elf_path" ]]; then
  init_exists=true
  init_size=$(wc -c <"$init_elf_path" 2>/dev/null || echo 0)
  init_mtime=$(stat -c %Y "$init_elf_path" 2>/dev/null || echo 0)
fi
if [[ -f "$netstackd_elf_path" ]]; then
  netstackd_exists=true
  netstackd_size=$(wc -c <"$netstackd_elf_path" 2>/dev/null || echo 0)
  netstackd_mtime=$(stat -c %Y "$netstackd_elf_path" 2>/dev/null || echo 0)
fi
agent_debug_log "$RUN_ID" "H2" "scripts/qemu-test.sh:artifact-state" "kernel/init-lite artifact state before qemu launch" \
  "{\"target_root\":\"$target_root\",\"kernel_exists\":$kernel_exists,\"kernel_size\":$kernel_size,\"kernel_mtime\":$kernel_mtime,\"init_exists\":$init_exists,\"init_size\":$init_size,\"init_mtime\":$init_mtime,\"netstackd_exists\":$netstackd_exists,\"netstackd_size\":$netstackd_size,\"netstackd_mtime\":$netstackd_mtime}"
# #endregion
agent_debug_log "$RUN_ID" "A" "scripts/qemu-test.sh:pre-run" "qemu smoke start" \
  "{\"run_timeout\":\"$RUN_TIMEOUT\",\"run_phase\":\"${RUN_PHASE:-}\",\"run_until_marker\":\"$RUN_UNTIL_MARKER\",\"require_smp\":\"${REQUIRE_SMP:-0}\",\"require_dhcp\":\"${REQUIRE_QEMU_DHCP:-0}\",\"require_dhcp_strict\":\"${REQUIRE_QEMU_DHCP_STRICT:-0}\",\"require_dsoftbus\":\"${REQUIRE_DSOFTBUS:-0}\",\"qemu_session_mode\":\"${QEMU_SESSION_MODE:-proof}\",\"qemu_marker_level\":\"${QEMU_MARKER_LEVEL:-proof}\",\"guest_selftest_mode\":\"${NEXUS_SELFTEST_MODE:-}\",\"guest_selftest_profile\":\"${NEXUS_SELFTEST_PROFILE:-}\",\"qemu_icount_args\":\"${QEMU_ICOUNT_ARGS:-}\"}"
QEMU_SESSION_MODE="${QEMU_SESSION_MODE:-proof}" \
QEMU_MARKER_LEVEL="${QEMU_MARKER_LEVEL:-proof}" \
NEXUS_SELFTEST_MODE="${NEXUS_SELFTEST_MODE:-}" \
NEXUS_SELFTEST_PROFILE="${NEXUS_SELFTEST_PROFILE:-}" \
RUN_TIMEOUT="$RUN_TIMEOUT" \
RUN_UNTIL_MARKER="$RUN_UNTIL_MARKER" \
QEMU_LOG="$QEMU_LOG" \
UART_LOG="$UART_LOG" \
QEMU_LOG_MAX="$QEMU_LOG_MAX" \
UART_LOG_MAX="$UART_LOG_MAX" \
QEMU_SESSION_MODE="$QEMU_SESSION_MODE" \
QEMU_MARKER_LEVEL="$QEMU_MARKER_LEVEL" \
QEMU_INPUT_AUTOINJECT="${QEMU_INPUT_AUTOINJECT:-0}" \
NEXUS_SELFTEST_MODE="${NEXUS_SELFTEST_MODE:-}" \
NEXUS_SELFTEST_PROFILE="${NEXUS_SELFTEST_PROFILE:-}" \
"$ROOT/scripts/qemu-launcher.sh" "${QEMU_EXTRA_ARGS[@]}"
qemu_status=$?
set -e

# #region agent log (hypothesis J/K/L: run-qemu result + artifact presence)
uart_exists=false
qemu_exists=false
uart_size=0
qemu_size=0
if [[ -f "$UART_LOG" ]]; then
  uart_exists=true
  uart_size=$(wc -c <"$UART_LOG" 2>/dev/null || echo 0)
fi
if [[ -f "$QEMU_LOG" ]]; then
  qemu_exists=true
  qemu_size=$(wc -c <"$QEMU_LOG" 2>/dev/null || echo 0)
fi
agent_debug_log "$RUN_ID" "J" "scripts/qemu-test.sh:diag-runqemu-result" "run-qemu completion and log artifacts" \
  "{\"qemu_status\":$qemu_status,\"uart_exists\":$uart_exists,\"uart_size\":$uart_size,\"qemu_exists\":$qemu_exists,\"qemu_size\":$qemu_size}"
# #region agent log (H3: marker progression in timeout/no-init cases)
count_init_start=0
count_init_ready=0
count_kself_wait_ok=0
last_uart_line=""
if [[ -f "$UART_LOG" ]]; then
  count_init_start=$(count_lines "init: start")
  count_init_ready=$(count_lines "init: ready")
  count_kself_wait_ok=$(count_lines "KSELFTEST: wait ok")
  last_uart_line=$(awk 'NF{line=$0} END{print line}' "$UART_LOG" | tr -d '\r' | sed 's/"/\\"/g' | sed 's/\\/\\\\/g')
fi
agent_debug_log "$RUN_ID" "H3" "scripts/qemu-test.sh:marker-progression" "marker counts and last uart line after qemu run" \
  "{\"count_init_start\":$count_init_start,\"count_init_ready\":$count_init_ready,\"count_kself_wait_ok\":$count_kself_wait_ok,\"last_uart_line\":\"${last_uart_line}\"}"
# #endregion
# #region agent log (H15: build-failure summary — signal if artifacts still missing post-run)
kernel_still_missing=false
init_still_missing=false
if [[ ! -f "$kernel_elf_path" ]]; then kernel_still_missing=true; fi
if [[ ! -f "$init_elf_path" ]]; then init_still_missing=true; fi
if $kernel_still_missing || $init_still_missing; then
  agent_debug_log "$RUN_ID" "H15" "scripts/qemu-test.sh:build-failure-summary" "post-run: kernel or init-lite artifact still missing" \
    "{\"kernel_missing\":$kernel_still_missing,\"init_missing\":$init_still_missing}"
fi
# #endregion
# #region agent log (hypothesis DNS1-DNS5: DHCP DNS proof path diagnostics)
count_dhcp_bound=0
count_dhcp_fallback=0
count_dns_ok=0
count_dns_unavailable=0
count_dns_fail=0
count_dns_rx_other=0
if [[ -f "$UART_LOG" ]]; then
  count_dhcp_bound=$(count_lines "net: dhcp bound")
  count_dhcp_fallback=$(count_lines "net: dhcp unavailable (fallback static")
  count_dns_ok=$(count_lines "SELFTEST: net udp dns ok")
  count_dns_unavailable=$(count_lines "netstackd: udp dns unavailable (fallback dhcp proof)")
  count_dns_fail=$(count_lines "netstackd: net dns proof fail")
  count_dns_rx_other=$(count_lines "netstackd: udp dns rx other")
fi
agent_debug_log "$RUN_ID" "DNS" "scripts/qemu-test.sh:diag-net-dns-proof" "dhcp/dns proof markers and dns loop stats" \
  "{\"dhcp_bound\":$count_dhcp_bound,\"dhcp_fallback\":$count_dhcp_fallback,\"dns_ok\":$count_dns_ok,\"dns_unavailable\":$count_dns_unavailable,\"dns_fail\":$count_dns_fail,\"dns_rx_other\":$count_dns_rx_other}"
# #endregion
# #endregion agent log

# SATP hang diagnostic: saw trampoline enter but no post-satp OK
if grep -aFq "AS: trampoline enter" "$UART_LOG" && ! grep -aFq "AS: post-satp OK" "$UART_LOG"; then
  echo "[error] SATP switch hang: trampoline entered but no post-satp marker" >&2
  exit 1
fi

# #region agent log (exec KPGF diagnostics hypotheses B-F)
kpgf_raw=$(grep -aF "KPGF sepc=" "$UART_LOG" | head -n1 || true)
kpgf_seen=false
kpgf_sepc=""
kpgf_stval=""
kpgf_a7=""
kpgf_a0=""
kpgf_a2=""
kpgf_pid=""
if [[ -n "$kpgf_raw" ]]; then
  kpgf_seen=true
  kpgf_sepc=$(printf "%s" "$kpgf_raw" | sed -n 's/.*sepc=\(0x[0-9a-fA-F]\+\).*/\1/p')
  kpgf_stval=$(printf "%s" "$kpgf_raw" | sed -n 's/.*stval=\(0x[0-9a-fA-F]\+\).*/\1/p')
  kpgf_a7=$(printf "%s" "$kpgf_raw" | sed -n 's/.* a7=\(0x[0-9a-fA-F]\+\).*/\1/p')
  kpgf_a0=$(printf "%s" "$kpgf_raw" | sed -n 's/.* a0=\(0x[0-9a-fA-F]\+\).*/\1/p')
  kpgf_a2=$(printf "%s" "$kpgf_raw" | sed -n 's/.* a2=\(0x[0-9a-fA-F]\+\).*/\1/p')
  kpgf_pid=$(printf "%s" "$kpgf_raw" | sed -n 's/.* pid=\(0x[0-9a-fA-F]\+\).*/\1/p')
fi
line_kpgf=$(grep -aFn "KPGF sepc=" "$UART_LOG" | head -n1 | cut -d: -f1 || echo 0)
line_timed_ready=$(grep -aFn "timed: ready" "$UART_LOG" | head -n1 | cut -d: -f1 || echo 0)
line_timed_ok=$(grep -aFn "SELFTEST: timed coalesce ok" "$UART_LOG" | head -n1 | cut -d: -f1 || echo 0)
line_execd_ready=$(grep -aFn "execd: ready" "$UART_LOG" | head -n1 | cut -d: -f1 || echo 0)
line_child_hello=$(grep -aFn "child: hello-elf" "$UART_LOG" | head -n1 | cut -d: -f1 || echo 0)
line_exec_routing=$(grep -aFn "SELFTEST: ipc routing execd ok" "$UART_LOG" | head -n1 | cut -d: -f1 || echo 0)

h_b_as_map_path=false
if [[ "$kpgf_seen" == "true" && "$kpgf_a7" == "0x000000000000000a" ]]; then
  h_b_as_map_path=true
fi
# #region agent log (hypothesis B)
agent_debug_log "$RUN_ID" "B" "scripts/qemu-test.sh:diag-kpgf-signature" "exec kpgf syscall signature" \
  "{\"kpgf_seen\":$kpgf_seen,\"pid\":\"${kpgf_pid}\",\"a7\":\"${kpgf_a7}\",\"a0\":\"${kpgf_a0}\",\"a2\":\"${kpgf_a2}\",\"is_as_map_signature\":$h_b_as_map_path}"
# #endregion agent log

h_c_timed_correlated=false
if [[ "$line_kpgf" -gt 0 && "$line_timed_ready" -gt 0 && "$line_timed_ok" -gt 0 && "$line_timed_ok" -lt "$line_kpgf" ]]; then
  h_c_timed_correlated=true
fi
# #region agent log (hypothesis C)
agent_debug_log "$RUN_ID" "C" "scripts/qemu-test.sh:diag-timed-order" "timed markers before crash" \
  "{\"line_timed_ready\":$line_timed_ready,\"line_timed_ok\":$line_timed_ok,\"line_kpgf\":$line_kpgf,\"timed_before_kpgf\":$h_c_timed_correlated}"
# #endregion agent log

h_d_exec_progress=false
if [[ "$line_execd_ready" -gt 0 && "$line_kpgf" -gt 0 && ( "$line_child_hello" -eq 0 || "$line_kpgf" -lt "$line_child_hello" ) ]]; then
  h_d_exec_progress=true
fi
# #region agent log (hypothesis D)
agent_debug_log "$RUN_ID" "D" "scripts/qemu-test.sh:diag-exec-progress" "exec phase progression around crash" \
  "{\"line_execd_ready\":$line_execd_ready,\"line_child_hello\":$line_child_hello,\"line_kpgf\":$line_kpgf,\"crash_before_child_hello\":$h_d_exec_progress}"
# #endregion agent log

h_e_range_overlap=false
if [[ "$kpgf_seen" == "true" && -n "$kpgf_a0" && -n "$kpgf_a2" && -n "$kpgf_stval" ]]; then
  a0_dec=$((16#${kpgf_a0#0x}))
  a2_dec=$((16#${kpgf_a2#0x}))
  stval_dec=$((16#${kpgf_stval#0x}))
  end_dec=$((a0_dec + a2_dec))
  if [[ "$stval_dec" -ge "$a0_dec" && "$stval_dec" -lt "$end_dec" ]]; then
    h_e_range_overlap=true
  fi
fi
# #region agent log (hypothesis E)
agent_debug_log "$RUN_ID" "E" "scripts/qemu-test.sh:diag-address-range" "fault address relative to syscall range" \
  "{\"stval\":\"${kpgf_stval}\",\"a0\":\"${kpgf_a0}\",\"a2\":\"${kpgf_a2}\",\"fault_inside_a0_a0_plus_a2\":$h_e_range_overlap}"
# #endregion agent log

h_f_exec_route_ready=false
if [[ "$line_exec_routing" -gt 0 && "$line_kpgf" -gt 0 && "$line_exec_routing" -lt "$line_kpgf" ]]; then
  h_f_exec_route_ready=true
fi
# #region agent log (hypothesis F)
agent_debug_log "$RUN_ID" "F" "scripts/qemu-test.sh:diag-exec-routing" "exec routing readiness before crash" \
  "{\"line_exec_routing_ok\":$line_exec_routing,\"line_kpgf\":$line_kpgf,\"routing_ready_before_crash\":$h_f_exec_route_ready}"
# #endregion agent log

qemu_lock_conflict=false
qemu_launch_fail=false
if [[ "$qemu_exists" == "true" ]]; then
  if rg -q "Could not acquire|write lock|Failed to initialize KVM|No such file or directory|Permission denied" "$QEMU_LOG"; then
    qemu_launch_fail=true
  fi
  if rg -q "write lock" "$QEMU_LOG"; then
    qemu_lock_conflict=true
  fi
fi
# #region agent log (hypothesis K/L)
agent_debug_log "$RUN_ID" "K" "scripts/qemu-test.sh:diag-qemu-launch-errors" "qemu launch error signatures" \
  "{\"qemu_exists\":$qemu_exists,\"qemu_launch_fail\":$qemu_launch_fail,\"qemu_lock_conflict\":$qemu_lock_conflict}"
# #endregion agent log
# #endregion agent log

# #region agent log (hypotheses H6-H10: metricsd -> logd visibility path)
line_metrics_rejects_ok=$(grep -aFn "SELFTEST: metrics security rejects ok" "$UART_LOG" | head -n1 | cut -d: -f1 || echo 0)
line_metrics_counter_fail=$(grep -aFn "SELFTEST: metrics counters FAIL" "$UART_LOG" | head -n1 | cut -d: -f1 || echo 0)
line_metrics_hist_fail=$(grep -aFn "SELFTEST: metrics histograms FAIL" "$UART_LOG" | head -n1 | cut -d: -f1 || echo 0)
line_metrics_span_fail=$(grep -aFn "SELFTEST: tracing spans FAIL" "$UART_LOG" | head -n1 | cut -d: -f1 || echo 0)
line_metrics_logd_no_increase=$(grep -aFn "dbg: metrics logd total did-not-increase" "$UART_LOG" | head -n1 | cut -d: -f1 || echo 0)
line_metrics_counter_miss=$(grep -aFn "dbg: metrics counter query miss" "$UART_LOG" | head -n1 | cut -d: -f1 || echo 0)
line_metrics_hist_miss=$(grep -aFn "dbg: metrics hist query miss" "$UART_LOG" | head -n1 | cut -d: -f1 || echo 0)
line_metrics_span_miss=$(grep -aFn "dbg: metrics span query miss" "$UART_LOG" | head -n1 | cut -d: -f1 || echo 0)

count_metricsd_semantic_counter=$( (grep -aF "[INFO metricsd] metrics snapshot counter name=selftest.counter" "$UART_LOG" || true) | wc -l | tr -d ' ' )
count_metricsd_semantic_hist=$( (grep -aF "[INFO metricsd] metrics snapshot histogram name=selftest.hist" "$UART_LOG" || true) | wc -l | tr -d ' ' )
count_metricsd_semantic_span=$( (grep -aF "[INFO metricsd] tracing span end name=selftest.span" "$UART_LOG" || true) | wc -l | tr -d ' ' )
count_logd_metrics_append_rx=$( (grep -aF "dbg: logd metrics append rx" "$UART_LOG" || true) | wc -l | tr -d ' ' )
count_logd_metrics_append_ok=$( (grep -aF "dbg: logd metrics append status ok" "$UART_LOG" || true) | wc -l | tr -d ' ' )
count_logd_metrics_append_rate=$( (grep -aF "dbg: logd metrics append status rate_limited" "$UART_LOG" || true) | wc -l | tr -d ' ' )
count_sink_metrics_slots_none=$( (grep -aF "dbg: sink-logd metrics ensure_slots none" "$UART_LOG" || true) | wc -l | tr -d ' ' )
count_sink_metrics_cap_clone_fail=$( (grep -aF "dbg: sink-logd metrics cap_clone fail" "$UART_LOG" || true) | wc -l | tr -d ' ' )
count_sink_metrics_send_err=$( (grep -aF "dbg: sink-logd metrics send err" "$UART_LOG" || true) | wc -l | tr -d ' ' )
count_sink_metrics_send_ok=$( (grep -aF "dbg: sink-logd metrics send ok" "$UART_LOG" || true) | wc -l | tr -d ' ' )
count_logd_scope_metricsd=$( (grep -aF "dbg: logd scope metricsd sid=0x" "$UART_LOG" || true) | wc -l | tr -d ' ' )
count_sink_selftest_slots_none=$( (grep -aF "dbg: sink-logd selftest ensure_slots none" "$UART_LOG" || true) | wc -l | tr -d ' ' )
count_sink_selftest_direct_slots=$( (grep -aF "dbg: sink-logd selftest direct slots" "$UART_LOG" || true) | wc -l | tr -d ' ' )
count_sink_selftest_routed_slots=$( (grep -aF "dbg: sink-logd selftest routed slots" "$UART_LOG" || true) | wc -l | tr -d ' ' )
count_sink_selftest_send_err=$( (grep -aF "dbg: sink-logd selftest send err" "$UART_LOG" || true) | wc -l | tr -d ' ' )
count_sink_selftest_send_ok=$( (grep -aF "dbg: sink-logd selftest send ok" "$UART_LOG" || true) | wc -l | tr -d ' ' )
line_selftest_sink_fail=$(grep -aFn "SELFTEST: nexus-log sink-logd FAIL" "$UART_LOG" | head -n1 | cut -d: -f1 || echo 0)
line_selftest_sink_ok=$(grep -aFn "SELFTEST: nexus-log sink-logd ok" "$UART_LOG" | head -n1 | cut -d: -f1 || echo 0)

agent_debug_log "$RUN_ID" "H6" "scripts/qemu-test.sh:diag-metrics-selftest-order" "metrics selftest marker order and misses" \
  "{\"line_metrics_rejects_ok\":$line_metrics_rejects_ok,\"line_metrics_counter_fail\":$line_metrics_counter_fail,\"line_metrics_hist_fail\":$line_metrics_hist_fail,\"line_metrics_span_fail\":$line_metrics_span_fail,\"line_metrics_logd_no_increase\":$line_metrics_logd_no_increase,\"line_metrics_counter_miss\":$line_metrics_counter_miss,\"line_metrics_hist_miss\":$line_metrics_hist_miss,\"line_metrics_span_miss\":$line_metrics_span_miss}"

agent_debug_log "$RUN_ID" "H7" "scripts/qemu-test.sh:diag-metrics-emission-counts" "metricsd semantic emission counts in uart" \
  "{\"count_semantic_counter\":$count_metricsd_semantic_counter,\"count_semantic_hist\":$count_metricsd_semantic_hist,\"count_semantic_span\":$count_metricsd_semantic_span}"

agent_debug_log "$RUN_ID" "H8" "scripts/qemu-test.sh:diag-logd-metrics-append" "logd observed metricsd append statuses" \
  "{\"count_metrics_append_rx\":$count_logd_metrics_append_rx,\"count_metrics_append_ok\":$count_logd_metrics_append_ok,\"count_metrics_append_rate_limited\":$count_logd_metrics_append_rate}"

agent_debug_log "$RUN_ID" "H9" "scripts/qemu-test.sh:diag-sink-logd-send-path" "nexus-log sink-logd metrics send path markers" \
  "{\"count_sink_slots_none\":$count_sink_metrics_slots_none,\"count_sink_cap_clone_fail\":$count_sink_metrics_cap_clone_fail,\"count_sink_send_err\":$count_sink_metrics_send_err,\"count_sink_send_ok\":$count_sink_metrics_send_ok}"

agent_debug_log "$RUN_ID" "H10" "scripts/qemu-test.sh:diag-logd-scope-metricsd" "logd observed append frames with metricsd scope" \
  "{\"count_logd_scope_metricsd\":$count_logd_scope_metricsd}"

agent_debug_log "$RUN_ID" "H11" "scripts/qemu-test.sh:diag-sink-selftest-slot-mode" "selftest sink-logd slot resolution mode" \
  "{\"count_selftest_slots_none\":$count_sink_selftest_slots_none,\"count_selftest_direct_slots\":$count_sink_selftest_direct_slots,\"count_selftest_routed_slots\":$count_sink_selftest_routed_slots}"

agent_debug_log "$RUN_ID" "H12" "scripts/qemu-test.sh:diag-sink-selftest-send" "selftest sink-logd send outcome" \
  "{\"count_selftest_send_ok\":$count_sink_selftest_send_ok,\"count_selftest_send_err\":$count_sink_selftest_send_err}"

agent_debug_log "$RUN_ID" "H13" "scripts/qemu-test.sh:diag-sink-selftest-probe-marker" "selftest sink-logd probe marker result" \
  "{\"line_sink_probe_ok\":$line_selftest_sink_ok,\"line_sink_probe_fail\":$line_selftest_sink_fail}"
# #endregion agent log

# #region agent log (hypotheses N1-N4: selftest failure locus and net/mmio side effects)
line_selftest_service_error=$(grep -aFn "service error svc=selftest-client type=()" "$UART_LOG" | head -n1 | cut -d: -f1 || echo 0)
line_net_mmio_cap_missing=$(grep -aFn "netstackd: mmio cap48 missing" "$UART_LOG" | head -n1 | cut -d: -f1 || echo 0)
line_net_mmio_map_fail=$(grep -aFn "netstackd: net FAIL mmio-map" "$UART_LOG" | head -n1 | cut -d: -f1 || echo 0)
line_sink_slots_configured=$(grep -aFn "dbg: selftest sink slots configured" "$UART_LOG" | head -n1 | cut -d: -f1 || echo 0)
line_sink_slots_config_fail=$(grep -aFn "dbg: selftest sink slots configure-fail" "$UART_LOG" | head -n1 | cut -d: -f1 || echo 0)
count_statefs_denied_keystore=$( (grep -aF "statefsd: access denied path=/state/keystore/" "$UART_LOG" || true) | wc -l | tr -d ' ' )
count_statefs_reply_send_fail=$( (grep -aF "statefsd: reply send fail" "$UART_LOG" || true) | wc -l | tr -d ' ' )

last_selftest_line=$(grep -aFn "SELFTEST:" "$UART_LOG" | tail -n1 | cut -d: -f1 || echo 0)
last_selftest_marker=$(grep -aF "SELFTEST:" "$UART_LOG" | tail -n1 | sed 's/"/\\"/g' || true)
if [[ -z "$last_selftest_marker" ]]; then
  last_selftest_marker="<none>"
fi

agent_debug_log "$RUN_ID" "N1" "scripts/qemu-test.sh:diag-selftest-failure-locus" "selftest failure and last marker locus" \
  "{\"line_selftest_service_error\":$line_selftest_service_error,\"last_selftest_line\":$last_selftest_line,\"last_selftest_marker\":\"$last_selftest_marker\"}"

agent_debug_log "$RUN_ID" "N2" "scripts/qemu-test.sh:diag-selftest-sink-config-result" "selftest sink slot config result marker" \
  "{\"line_sink_slots_configured\":$line_sink_slots_configured,\"line_sink_slots_config_fail\":$line_sink_slots_config_fail}"

agent_debug_log "$RUN_ID" "N3" "scripts/qemu-test.sh:diag-statefs-deny-pressure" "statefs deny/reply-fail pressure counts" \
  "{\"count_statefs_denied_keystore\":$count_statefs_denied_keystore,\"count_statefs_reply_send_fail\":$count_statefs_reply_send_fail}"

agent_debug_log "$RUN_ID" "N4" "scripts/qemu-test.sh:diag-netstackd-mmio-failure" "netstackd mmio failure marker locus" \
  "{\"line_net_mmio_cap_missing\":$line_net_mmio_cap_missing,\"line_net_mmio_map_fail\":$line_net_mmio_map_fail}"

line_cp0=$(grep -aFn "dbg: selftest cp0 run-start" "$UART_LOG" | head -n1 | cut -d: -f1 || echo 0)
line_cp1=$(grep -aFn "dbg: selftest cp1 keystored-resolved" "$UART_LOG" | head -n1 | cut -d: -f1 || echo 0)
line_cp2=$(grep -aFn "dbg: selftest cp2 post-statefs-keystore" "$UART_LOG" | head -n1 | cut -d: -f1 || echo 0)
line_cp3=$(grep -aFn "dbg: selftest cp3 post-reply-capmove" "$UART_LOG" | head -n1 | cut -d: -f1 || echo 0)
line_cp4=$(grep -aFn "dbg: selftest cp4 updated-routed" "$UART_LOG" | head -n1 | cut -d: -f1 || echo 0)
line_fail_resolve_keystored=$(grep -aFn "dbg: selftest fail resolve-keystored" "$UART_LOG" | head -n1 | cut -d: -f1 || echo 0)
line_fail_route_samgrd=$(grep -aFn "dbg: selftest fail route-samgrd" "$UART_LOG" | head -n1 | cut -d: -f1 || echo 0)
line_fail_route_policyd=$(grep -aFn "dbg: selftest fail route-policyd" "$UART_LOG" | head -n1 | cut -d: -f1 || echo 0)
line_fail_route_bundlemgrd=$(grep -aFn "dbg: selftest fail route-bundlemgrd" "$UART_LOG" | head -n1 | cut -d: -f1 || echo 0)
line_fail_route_updated=$(grep -aFn "dbg: selftest fail route-updated" "$UART_LOG" | head -n1 | cut -d: -f1 || echo 0)
line_execd_spawn_denied_id_mismatch=$(grep -aFn "execd: spawn denied (id mismatch)" "$UART_LOG" | head -n1 | cut -d: -f1 || echo 0)
line_execd_spawn_denied_policy=$(grep -aFn "execd: spawn denied (policy)" "$UART_LOG" | head -n1 | cut -d: -f1 || echo 0)
count_execd_spawn_id_mismatch_detail=$( (grep -aF "execd: spawn id mismatch sender=" "$UART_LOG" || true) | wc -l | tr -d ' ' )
first_execd_spawn_id_mismatch_detail=$(grep -aF "execd: spawn id mismatch sender=" "$UART_LOG" | head -n1 | sed 's/"/\\"/g' || true)
if [[ -z "$first_execd_spawn_id_mismatch_detail" ]]; then
  first_execd_spawn_id_mismatch_detail="<none>"
fi
line_policyd_mmio_net=$(grep -aFn "policyd: mmio net " "$UART_LOG" | head -n1 | cut -d: -f1 || echo 0)
line_policyd_mmio_rng=$(grep -aFn "policyd: mmio rng " "$UART_LOG" | head -n1 | cut -d: -f1 || echo 0)
line_policyd_mmio_blk=$(grep -aFn "policyd: mmio blk " "$UART_LOG" | head -n1 | cut -d: -f1 || echo 0)
first_policyd_mmio_net=$(grep -aF "policyd: mmio net " "$UART_LOG" | head -n1 | sed 's/"/\\"/g' || true)
first_policyd_mmio_rng=$(grep -aF "policyd: mmio rng " "$UART_LOG" | head -n1 | sed 's/"/\\"/g' || true)
first_policyd_mmio_blk=$(grep -aF "policyd: mmio blk " "$UART_LOG" | head -n1 | sed 's/"/\\"/g' || true)
line_rng_sender_sid_canonical=$(grep -aFn "rngd: sender sid selftest canonical" "$UART_LOG" | head -n1 | cut -d: -f1 || echo 0)
line_rng_sender_sid_alt=$(grep -aFn "rngd: sender sid selftest alt" "$UART_LOG" | head -n1 | cut -d: -f1 || echo 0)
line_rng_sender_sid_other=$(grep -aFn "rngd: sender sid other" "$UART_LOG" | head -n1 | cut -d: -f1 || echo 0)
line_rng_policy_allow=$(grep -aFn "rngd: policy allow" "$UART_LOG" | head -n1 | cut -d: -f1 || echo 0)
line_rng_policy_deny=$(grep -aFn "rngd: policy deny" "$UART_LOG" | head -n1 | cut -d: -f1 || echo 0)
line_policyd_delegated_rng_sender_canonical=$(grep -aFn "policyd: delegated rng sender canonical" "$UART_LOG" | head -n1 | cut -d: -f1 || echo 0)
line_policyd_delegated_rng_sender_other=$(grep -aFn "policyd: delegated rng sender other" "$UART_LOG" | head -n1 | cut -d: -f1 || echo 0)
first_policyd_delegated_rng_sender_sid=$(grep -aF "policyd: delegated rng sender sid=0x" "$UART_LOG" | head -n1 | sed 's/"/\\"/g' || true)
line_policyd_delegated_rng_allow=$(grep -aFn "policyd: delegated rng allow" "$UART_LOG" | head -n1 | cut -d: -f1 || echo 0)
line_policyd_delegated_rng_deny=$(grep -aFn "policyd: delegated rng deny" "$UART_LOG" | head -n1 | cut -d: -f1 || echo 0)
if [[ -z "$first_policyd_delegated_rng_sender_sid" ]]; then first_policyd_delegated_rng_sender_sid="<none>"; fi
if [[ -z "$first_policyd_mmio_net" ]]; then first_policyd_mmio_net="<none>"; fi
if [[ -z "$first_policyd_mmio_rng" ]]; then first_policyd_mmio_rng="<none>"; fi
if [[ -z "$first_policyd_mmio_blk" ]]; then first_policyd_mmio_blk="<none>"; fi

agent_debug_log "$RUN_ID" "N5" "scripts/qemu-test.sh:diag-selftest-checkpoints" "selftest checkpoint progression and explicit fail labels" \
  "{\"line_cp0\":$line_cp0,\"line_cp1\":$line_cp1,\"line_cp2\":$line_cp2,\"line_cp3\":$line_cp3,\"line_cp4\":$line_cp4,\"line_fail_resolve_keystored\":$line_fail_resolve_keystored,\"line_fail_route_samgrd\":$line_fail_route_samgrd,\"line_fail_route_policyd\":$line_fail_route_policyd,\"line_fail_route_bundlemgrd\":$line_fail_route_bundlemgrd,\"line_fail_route_updated\":$line_fail_route_updated}"

agent_debug_log "$RUN_ID" "N6" "scripts/qemu-test.sh:diag-execd-deny-class" "execd deny class and mismatch detail" \
  "{\"line_execd_spawn_denied_id_mismatch\":$line_execd_spawn_denied_id_mismatch,\"line_execd_spawn_denied_policy\":$line_execd_spawn_denied_policy,\"count_execd_spawn_id_mismatch_detail\":$count_execd_spawn_id_mismatch_detail,\"first_execd_spawn_id_mismatch_detail\":\"$first_execd_spawn_id_mismatch_detail\"}"

agent_debug_log "$RUN_ID" "N7" "scripts/qemu-test.sh:diag-policyd-mmio-cap-check" "policyd mmio check-cap decisions for grant subjects" \
  "{\"line_policyd_mmio_net\":$line_policyd_mmio_net,\"line_policyd_mmio_rng\":$line_policyd_mmio_rng,\"line_policyd_mmio_blk\":$line_policyd_mmio_blk,\"first_policyd_mmio_net\":\"$first_policyd_mmio_net\",\"first_policyd_mmio_rng\":\"$first_policyd_mmio_rng\",\"first_policyd_mmio_blk\":\"$first_policyd_mmio_blk\"}"

agent_debug_log "$RUN_ID" "N8" "scripts/qemu-test.sh:diag-rngd-policy-subject" "rngd caller SID class and delegated policy result" \
  "{\"line_rng_sender_sid_canonical\":$line_rng_sender_sid_canonical,\"line_rng_sender_sid_alt\":$line_rng_sender_sid_alt,\"line_rng_sender_sid_other\":$line_rng_sender_sid_other,\"line_rng_policy_allow\":$line_rng_policy_allow,\"line_rng_policy_deny\":$line_rng_policy_deny}"

agent_debug_log "$RUN_ID" "N9" "scripts/qemu-test.sh:diag-policyd-rng-delegated" "policyd delegated rng sender class and decision" \
  "{\"line_policyd_delegated_rng_sender_canonical\":$line_policyd_delegated_rng_sender_canonical,\"line_policyd_delegated_rng_sender_other\":$line_policyd_delegated_rng_sender_other,\"first_policyd_delegated_rng_sender_sid\":\"$first_policyd_delegated_rng_sender_sid\",\"line_policyd_delegated_rng_allow\":$line_policyd_delegated_rng_allow,\"line_policyd_delegated_rng_deny\":$line_policyd_delegated_rng_deny}"
# #endregion agent log

# Require os-lite userspace init markers; kernel fallback no longer accepted
if ! grep -aFq "init: start" "$UART_LOG"; then
  echo "[error] os-lite init markers not found (init: start); userspace bring-up required" >&2
  exit 1
fi

missing=0
# --- UART verification via expected_sequence (manifest-verified via pm_mirror_check pre-run) ---
missing_marker=""
missing_pos=-1
metrics_markers_required=0
if grep -aFq "init: start metricsd" "$UART_LOG" || grep -aFq "metricsd: ready" "$UART_LOG"; then
  metrics_markers_required=1
fi
for marker in "${expected_sequence[@]}"; do
  missing_pos=$((missing_pos + 1))
  if ! grep -aFq "$marker" "$UART_LOG"; then
    if [[ "$metrics_markers_required" -eq 0 ]]; then
      case "$marker" in
        "init: start metricsd"|"init: up metricsd"|"metricsd: ready"|\
        "metricsd: reject invalid_args"|"metricsd: reject over_limit"|"metricsd: reject rate_limited"|\
        "SELFTEST: metrics security rejects ok"|"SELFTEST: metrics counters ok"|\
        "SELFTEST: metrics gauges ok"|"SELFTEST: metrics histograms ok"|\
        "SELFTEST: tracing spans ok"|"SELFTEST: metrics retention ok")
          continue
          ;;
      esac
    fi
    missing=1
    missing_marker="$marker"
    break
  fi
done
if [[ "$missing" -ne 0 ]]; then
  failed_phase="unknown"
  # Map missing marker position to the earliest phase whose end marker would include it.
  for phase in "${PHASES[@]}"; do
    end_marker="${PHASE_END_MARKER[$phase]:-}"
    if [[ -z "$end_marker" ]]; then
      continue
    fi
    end_idx=$(find_marker_index "$end_marker" "${expected_sequence[@]}" || true)
    if [[ -n "$end_idx" && "$missing_pos" -le "$end_idx" ]]; then
      failed_phase="$phase"
      break
    fi
  done

  if [[ "$missing_marker" == *": ready" ]]; then
    svc="${missing_marker%%:*}"
    if grep -aFq "init: up $svc" "$UART_LOG"; then
      echo "[error] first_failed_phase=$failed_phase missing_marker='$missing_marker'" >&2
      echo "[error] Service up but not ready: missing '$missing_marker' after 'init: up $svc'" >&2
      print_uart_excerpt "${PHASE_START_MARKER[$failed_phase]:-}" "${expected_sequence[$((missing_pos - 1))]:-}"
      exit 1
    fi
  fi
  echo "[error] first_failed_phase=$failed_phase missing_marker='$missing_marker'" >&2
  echo "[error] Missing UART marker: $missing_marker" >&2
  print_uart_excerpt "${PHASE_START_MARKER[$failed_phase]:-}" "${expected_sequence[$((missing_pos - 1))]:-}"
  exit 1
fi

# Optional deterministic DHCP proof:
# - By default, QEMU smoke tests validate "network stack configured" via `net: smoltcp iface up`
#   and do NOT require slirp/usernet DHCP to be present (which can vary across environments).
# - When REQUIRE_QEMU_DHCP=1, we prefer enforcing DHCP + dependent L3/L4 proofs, but some host
#   environments still lack functional slirp DHCP under icount. In that case, we accept the honest
#   fallback marker and skip the DHCP-dependent proofs.
#
# If you want strict enforcement, use REQUIRE_QEMU_DHCP_STRICT=1.
REQUIRE_QEMU_DHCP_STRICT=${REQUIRE_QEMU_DHCP_STRICT:-0}
REQUIRE_QEMU_DHCP=${REQUIRE_QEMU_DHCP:-0}
if [[ "$REQUIRE_QEMU_DHCP" == "1" ]]; then
  dhcp_bound_seen=false
  dhcp_fallback_seen=false
  dns_ok_seen=false
  dns_fail_seen=false
  dns_unavailable_seen=false
  if grep -aFq "net: dhcp bound" "$UART_LOG"; then
    dhcp_bound_seen=true
  fi
  if grep -aFq "net: dhcp unavailable (fallback static" "$UART_LOG"; then
    dhcp_fallback_seen=true
  fi
  if grep -aFq "SELFTEST: net udp dns ok" "$UART_LOG"; then
    dns_ok_seen=true
  fi
  if grep -aFq "netstackd: net dns proof fail" "$UART_LOG"; then
    dns_fail_seen=true
  fi
  if grep -aFq "netstackd: udp dns unavailable (fallback dhcp proof)" "$UART_LOG"; then
    dns_unavailable_seen=true
  fi
  # #region agent log (DHCP gate determinism / fake-green guard)
  agent_debug_log "$RUN_ID" "DNS_GATE" "scripts/qemu-test.sh:dhcp-gate" "dhcp gate marker state snapshot" \
    "{\"dhcp_bound\":$dhcp_bound_seen,\"dhcp_fallback\":$dhcp_fallback_seen,\"dns_ok\":$dns_ok_seen,\"dns_fail\":$dns_fail_seen,\"dns_unavailable\":$dns_unavailable_seen,\"require_dhcp_strict\":$REQUIRE_QEMU_DHCP_STRICT}"
  # #endregion
  if [[ "$dhcp_bound_seen" == "true" && "$dhcp_fallback_seen" == "true" ]]; then
    echo "[error] first_failed_phase=mmio missing_marker='dhcp-state-exclusive'" >&2
    echo "[error] Contradictory DHCP markers: both bound and fallback seen in same run" >&2
    print_uart_excerpt "${PHASE_START_MARKER[mmio]}" "SELFTEST: net iface ok"
    exit 1
  fi
  if [[ "$dhcp_bound_seen" == "true" ]]; then
    if [[ "$dns_fail_seen" == "true" || "$dns_unavailable_seen" == "true" ]]; then
      echo "[error] first_failed_phase=mmio missing_marker='dns-proof-no-fail-marker'" >&2
      echo "[error] DHCP bound run emitted DNS failure marker (fake-green guard tripped)" >&2
      print_uart_excerpt "netstackd: net dns proof fail" "SELFTEST: net ping ok"
      exit 1
    fi
     for m in \
       "SELFTEST: net ping ok" \
       "SELFTEST: net udp dns ok"; do
       if ! grep -aFq "$m" "$UART_LOG"; then
         echo "[error] first_failed_phase=mmio missing_marker='$m'" >&2
         echo "[error] Missing UART marker (REQUIRE_QEMU_DHCP=1): $m" >&2
         print_uart_excerpt "${PHASE_START_MARKER[mmio]}" "SELFTEST: net iface ok"
         exit 1
       fi
     done
     # ICMP ping is only mandatory under strict DHCP (slirp+icount can drop ICMP)
     if [[ "$REQUIRE_QEMU_DHCP_STRICT" == "1" ]]; then
       if ! grep -aFq "SELFTEST: icmp ping ok" "$UART_LOG"; then
         echo "[error] first_failed_phase=mmio missing_marker='SELFTEST: icmp ping ok'" >&2
         echo "[error] Missing UART marker (REQUIRE_QEMU_DHCP_STRICT=1): SELFTEST: icmp ping ok" >&2
         print_uart_excerpt "${PHASE_START_MARKER[mmio]}" "SELFTEST: net iface ok"
         exit 1
       fi
     fi
  else
    if [[ "$REQUIRE_QEMU_DHCP_STRICT" == "1" ]]; then
      echo "[error] first_failed_phase=mmio missing_marker='net: dhcp bound'" >&2
      echo "[error] Missing UART marker (REQUIRE_QEMU_DHCP_STRICT=1): net: dhcp bound" >&2
      print_uart_excerpt "${PHASE_START_MARKER[mmio]}" "SELFTEST: net iface ok"
      exit 1
    fi
    if ! grep -aFq "net: dhcp unavailable (fallback static" "$UART_LOG"; then
      echo "[error] first_failed_phase=mmio missing_marker='net: dhcp bound|net: dhcp unavailable'" >&2
      echo "[error] Missing UART marker (REQUIRE_QEMU_DHCP=1): net: dhcp bound (or fallback marker)" >&2
      print_uart_excerpt "${PHASE_START_MARKER[mmio]}" "SELFTEST: net iface ok"
      exit 1
    fi
    echo "[warn] REQUIRE_QEMU_DHCP=1: DHCP not bound; static fallback in use (skipping DHCP-dependent proofs)" >&2
  fi
fi

# #region agent log (post-run summary; Slice B)
{
  dhcp_bound=false
  dhcp_fallback=false
  if grep -aFq "net: dhcp bound" "$UART_LOG"; then dhcp_bound=true; fi
  if grep -aFq "net: dhcp unavailable (fallback static" "$UART_LOG"; then dhcp_fallback=true; fi
  # Sanitize missing_marker for JSON (avoid quotes/newlines).
  mm=${missing_marker//$'\"'/"'"}
  mm=${mm//$'\n'/ }
  agent_debug_log "$RUN_ID" "A" "scripts/qemu-test.sh:post-run" "qemu smoke uart summary" \
    "{\"exit_code\":$qemu_status,\"dhcp_bound\":$dhcp_bound,\"dhcp_fallback\":$dhcp_fallback,\"first_failed_phase\":\"${failed_phase:-}\",\"missing_marker\":\"$mm\"}"
}
# #endregion agent log

# Optional DSoftBus E2E proof:
# - Default QEMU smoke does not require cross-node DSoftBus behavior (that proof is covered by
#   the dedicated 2-VM harness: `just os2vm` / `tools/os2vm.sh`).
# - When REQUIRE_DSOFTBUS=1, enforce the DSoftBus marker ladder.
REQUIRE_DSOFTBUS=${REQUIRE_DSOFTBUS:-0}
REQUIRE_DSOFTBUS_REMOTE_PKGFS=${REQUIRE_DSOFTBUS_REMOTE_PKGFS:-0}
REQUIRE_DSOFTBUS_REMOTE_STATEFS=${REQUIRE_DSOFTBUS_REMOTE_STATEFS:-0}
if [[ "$REQUIRE_DSOFTBUS" == "1" ]]; then
  for m in \
    "dsoftbusd: discovery up (udp loopback)" \
    "dsoftbusd: discovery announce sent" \
    "dsoftbusd: discovery peer found device=local" \
    "dsoftbusd: os transport up (udp+tcp)" \
    "dsoftbusd: transport selected quic" \
    "dsoftbusd: session connect peer=node-b" \
    "dsoftbusd: identity bound peer=node-b" \
    "dsoftbusd: dual-node session ok" \
    "dsoftbusd: ready" \
    "dsoftbusd: auth ok" \
    "dsoftbusd: os session ok" \
    "SELFTEST: quic session ok" \
    "dsoftbus:mux session up" \
    "dsoftbus:mux data ok" \
    "SELFTEST: mux pri control ok" \
    "SELFTEST: mux bulk ok" \
    "SELFTEST: mux backpressure ok" \
    "SELFTEST: dsoftbus os connect ok" \
    "SELFTEST: dsoftbus ping ok"; do
    if ! grep -aFq "$m" "$UART_LOG"; then
      echo "[error] first_failed_phase=routing missing_marker='$m'" >&2
      echo "[error] Missing UART marker (REQUIRE_DSOFTBUS=1): $m" >&2
      print_uart_excerpt "${PHASE_START_MARKER[routing]}" "netstackd: facade up"
      exit 1
    fi
  done
  if grep -aFq "dsoftbusd: transport selected tcp" "$UART_LOG"; then
    echo "[error] first_failed_phase=routing unexpected_marker='dsoftbusd: transport selected tcp'" >&2
    echo "[error] Unexpected fallback marker while QUIC session path is required: dsoftbusd: transport selected tcp" >&2
    print_uart_excerpt "${PHASE_START_MARKER[routing]}" "dsoftbusd: ready"
    exit 1
  fi
  if grep -aFq "dsoftbus: quic os disabled (fallback tcp)" "$UART_LOG"; then
    echo "[error] first_failed_phase=routing unexpected_marker='dsoftbus: quic os disabled (fallback tcp)'" >&2
    echo "[error] Unexpected fallback marker while QUIC session path is required: dsoftbus: quic os disabled (fallback tcp)" >&2
    print_uart_excerpt "${PHASE_START_MARKER[routing]}" "dsoftbusd: ready"
    exit 1
  fi
  if grep -aFq "SELFTEST: quic fallback ok" "$UART_LOG"; then
    echo "[error] first_failed_phase=routing unexpected_marker='SELFTEST: quic fallback ok'" >&2
    echo "[error] Unexpected fallback selftest marker while QUIC session path is required: SELFTEST: quic fallback ok" >&2
    print_uart_excerpt "${PHASE_START_MARKER[routing]}" "dsoftbusd: ready"
    exit 1
  fi
  if [[ "$REQUIRE_DSOFTBUS_REMOTE_PKGFS" == "1" ]]; then
    if grep -aFq "dsoftbusd: cross-vm session ok" "$UART_LOG"; then
      for m in \
        "dsoftbusd: remote packagefs served" \
        "SELFTEST: remote pkgfs stat ok" \
        "SELFTEST: remote pkgfs open ok" \
        "SELFTEST: remote pkgfs read step ok" \
        "SELFTEST: remote pkgfs close ok" \
        "SELFTEST: remote pkgfs read ok"; do
        if ! grep -aFq "$m" "$UART_LOG"; then
          echo "[error] first_failed_phase=routing missing_marker='$m'" >&2
          echo "[error] Missing UART marker (REQUIRE_DSOFTBUS_REMOTE_PKGFS=1): $m" >&2
          print_uart_excerpt "${PHASE_START_MARKER[routing]}" "SELFTEST: remote query ok"
          exit 1
        fi
      done
    else
      echo "[info] REQUIRE_DSOFTBUS_REMOTE_PKGFS=1 but cross-vm session marker is absent; remote packagefs gate is not applicable in single-VM runs" >&2
    fi
  fi
  if [[ "$REQUIRE_DSOFTBUS_REMOTE_STATEFS" == "1" ]]; then
    if grep -aFq "dsoftbusd: cross-vm session ok" "$UART_LOG"; then
      for m in \
        "dsoftbusd: remote statefs served" \
        "SELFTEST: remote statefs rw ok"; do
        if ! grep -aFq "$m" "$UART_LOG"; then
          echo "[error] first_failed_phase=routing missing_marker='$m'" >&2
          echo "[error] Missing UART marker (REQUIRE_DSOFTBUS_REMOTE_STATEFS=1): $m" >&2
          print_uart_excerpt "${PHASE_START_MARKER[routing]}" "SELFTEST: remote query ok"
          exit 1
        fi
      done
    else
      echo "[info] REQUIRE_DSOFTBUS_REMOTE_STATEFS=1 but cross-vm session marker is absent; remote statefs gate is not applicable in single-VM runs" >&2
    fi
  fi
fi

# TASK-0055 UI fake-green guard: UI selftest markers summarize checked
# `windowd` present state and must not appear without their prerequisites.
if grep -aFq "SELFTEST: ui launcher present ok" "$UART_LOG"; then
  for m in \
    "windowd: ready (w=1280, h=800, hz=120)" \
    "windowd: systemui loaded (profile=desktop)" \
    "windowd: present ok (seq=1 dmg=1)" \
    "launcher: first frame ok"; do
    if ! grep -aFq "$m" "$UART_LOG"; then
      echo "[error] first_failed_phase=end missing_marker='$m'" >&2
      echo "[error] UI selftest marker appeared before required checked present state: $m" >&2
      print_uart_excerpt "${PHASE_START_MARKER[end]}" "SELFTEST: sandbox deny ok"
      exit 1
    fi
  done
fi
# TASK-0025 fake-green guard: once statefsd reports envelope hardening on, the
# three hardening selftests must complete ok (the proof-manifest phase walker
# counts FAIL variants as "marker seen", so enforce the ok/FAIL split here).
if grep -aFq "statefsd: write hardening on (auth-envelope)" "$UART_LOG"; then
  for m in \
    "SELFTEST: statefs auth put ok" \
    "SELFTEST: statefs tamper deny ok" \
    "SELFTEST: statefs rollback deny ok"; do
    if ! grep -aFq "$m" "$UART_LOG"; then
      echo "[error] first_failed_phase=bringup missing_marker='$m'" >&2
      echo "[error] statefs hardening active but selftest marker missing: $m" >&2
      print_uart_excerpt "statefsd: write hardening on (auth-envelope)" "SELFTEST: statefs persist ok"
      exit 1
    fi
  done
  for m in \
    "SELFTEST: statefs auth put FAIL" \
    "SELFTEST: statefs tamper deny FAIL" \
    "SELFTEST: statefs rollback deny FAIL"; do
    if grep -aFq "$m" "$UART_LOG"; then
      echo "[error] first_failed_phase=bringup missing_marker='${m% FAIL} ok'" >&2
      echo "[error] statefs hardening selftest emitted failure marker: $m" >&2
      print_uart_excerpt "statefsd: write hardening on (auth-envelope)" "SELFTEST: statefs persist ok"
      exit 1
    fi
  done
fi
# TASK-0026 fake-green guard: once statefsd reports journal v2 mounted, the
# 2PC/compaction selftests must complete ok and one honest cold-boot verdict
# must appear ("seeded" on a fresh image; "persist ok" only ever on a second
# boot against a preserved image). FAIL variants and a failed compaction
# reopen-verify are fatal (the proof-manifest phase walker counts FAIL
# variants as "marker seen", so enforce the ok/FAIL split here).
if grep -aFq "statefsd: journal v2 mounted (2PC)" "$UART_LOG"; then
  for m in \
    "SELFTEST: statefs v2 crash-atomic ok" \
    "statefsd: compaction done (gen=" \
    "SELFTEST: statefs v2 compact ok"; do
    if ! grep -aFq "$m" "$UART_LOG"; then
      echo "[error] first_failed_phase=bringup missing_marker='$m'" >&2
      echo "[error] statefs journal v2 active but selftest marker missing: $m" >&2
      print_uart_excerpt "statefsd: journal v2 mounted (2PC)" "SELFTEST: statefs rollback deny ok"
      exit 1
    fi
  done
  if ! grep -aFq "SELFTEST: statefs cold-boot persist ok" "$UART_LOG" \
     && ! grep -aFq "SELFTEST: statefs cold-boot seeded" "$UART_LOG"; then
    echo "[error] first_failed_phase=bringup missing_marker='SELFTEST: statefs cold-boot persist ok'" >&2
    echo "[error] statefs journal v2 active but no cold-boot verdict (seeded|persist ok) appeared" >&2
    print_uart_excerpt "statefsd: journal v2 mounted (2PC)" "SELFTEST: statefs v2 compact ok"
    exit 1
  fi
  # Cold-boot lane (REQUIRE_STATEFS_COLD_BOOT=1, run with NEXUS_KEEP_BLK=1
  # against an image a prior boot seeded): the sentinel must be PRESENT —
  # "seeded" on this lane means persistence silently failed.
  if [[ "${REQUIRE_STATEFS_COLD_BOOT:-0}" == "1" ]] \
     && ! grep -aFq "SELFTEST: statefs cold-boot persist ok" "$UART_LOG"; then
    echo "[error] first_failed_phase=bringup missing_marker='SELFTEST: statefs cold-boot persist ok'" >&2
    echo "[error] cold-boot lane: preserved-image boot did not replay the sentinel" >&2
    print_uart_excerpt "statefsd: journal v2 mounted (2PC)" "SELFTEST: statefs v2 compact ok"
    exit 1
  fi
  # TASK-0049C: on the preserved-image boot logd's spill attach must have
  # loaded last boot's evidence ring (loaded=0x00 on a keep-blk lane means
  # persistence silently failed).
  if [[ "${REQUIRE_STATEFS_COLD_BOOT:-0}" == "1" ]] \
     && ! grep -aqE "logd: evidence persist on \(loaded=0x0*[1-9a-f]" "$UART_LOG"; then
    echo "[error] first_failed_phase=logd missing_marker='logd: evidence persist on (loaded>0)'" >&2
    echo "[error] cold-boot lane: no evidence records survived the reboot" >&2
    grep -a "logd: evidence persist on" "$UART_LOG" | head -n 4 >&2
    exit 1
  fi
  # TASK-0049B PR-B3c: on the preserved-image boot the prior boot's restart
  # counters must be visible again (baseline > 0 → persist marker).
  if [[ "${REQUIRE_STATEFS_COLD_BOOT:-0}" == "1" ]] \
     && ! grep -aFq "SELFTEST: crash-loop persist ok" "$UART_LOG"; then
    echo "[error] first_failed_phase=end missing_marker='SELFTEST: crash-loop persist ok'" >&2
    echo "[error] cold-boot lane: restart counters did not survive the reboot" >&2
    grep -a "init: supervision persist\|SELFTEST: crash-loop" "$UART_LOG" | head -n 8 >&2
    exit 1
  fi
  for m in \
    "SELFTEST: statefs v2 crash-atomic FAIL" \
    "SELFTEST: statefs v2 compact FAIL" \
    "SELFTEST: statefs cold-boot persist FAIL" \
    "statefsd: compaction verify failed"; do
    if grep -aFq "$m" "$UART_LOG"; then
      echo "[error] first_failed_phase=bringup missing_marker='$m'" >&2
      echo "[error] statefs journal v2 selftest emitted failure signature: $m" >&2
      print_uart_excerpt "statefsd: journal v2 mounted (2PC)" "SELFTEST: statefs v2 compact ok"
      exit 1
    fi
  done
fi
# TASK-0027 fake-green guard: the `encryption on` marker (emitted only after
# statefsd's in-process AEAD self-check) and the roundtrip selftest must
# agree in both directions; the loud failure signatures are fatal.
if grep -aFq "statefsd: encryption on (xchacha20poly1305)" "$UART_LOG" \
   && ! grep -aFq "SELFTEST: statefs enc roundtrip ok" "$UART_LOG"; then
  echo "[error] first_failed_phase=bringup missing_marker='SELFTEST: statefs enc roundtrip ok'" >&2
  echo "[error] statefs encryption on but the roundtrip selftest did not pass" >&2
  print_uart_excerpt "statefsd: encryption on (xchacha20poly1305)" "SELFTEST: statefs v2 compact ok"
  exit 1
fi
if grep -aFq "SELFTEST: statefs enc roundtrip ok" "$UART_LOG" \
   && ! grep -aFq "statefsd: encryption on (xchacha20poly1305)" "$UART_LOG"; then
  echo "[error] first_failed_phase=bringup missing_marker='statefsd: encryption on (xchacha20poly1305)'" >&2
  echo "[error] statefs enc roundtrip claimed ok without the encryption-on marker (fake green)" >&2
  print_uart_excerpt "statefsd: journal v2 mounted (2PC)" "SELFTEST: statefs v2 compact ok"
  exit 1
fi
# TASK-0049B supervision guard (PR-B3b form): every announced NON-CLEAN
# service death must be PAIRED with a supervised restart or an announced
# crash-loop block — an unpaired death means the rest of the ladder ran
# against a corpse; an unpaired restart line would be fake green. Clean
# exits rest by policy (RFC-0087 §2, EngineDecision::Rest): run-to-
# completion fixtures like touchd legitimately exit 0 once a hart has idle
# time for them (SMP=2 MTTCG), and pairing those would demand restarts the
# supervision SSOT forbids. (The restart E2E proof kills pinched once per
# storm round on purpose; the fault-probe injector's cap line is excluded
# because its deaths are never announced as service-exit lines.)
svc_deaths=$(grep -aEc "init: service exit name=[^ ]+ reason=(error|fault|killed|unknown)" "$UART_LOG" || true)
svc_restarts=$(grep -ac "init: service restarted name=" "$UART_LOG" || true)
svc_blocked=$(grep -a "init: crash-loop blocked svc=" "$UART_LOG" | grep -vc "svc=fault-probe" || true)
if [[ "${svc_deaths:-0}" -ne $(( ${svc_restarts:-0} + ${svc_blocked:-0} )) ]]; then
  echo "[error] first_failed_phase=bringup missing_marker='init: service restarted name='" >&2
  echo "[error] non-clean service deaths ($svc_deaths) != supervised restarts ($svc_restarts) + crash-loop blocks ($svc_blocked) in a proof boot" >&2
  grep -a "init: service exit name=\|init: service restarted name=\|init: crash-loop blocked svc=\|init: FAIL" "$UART_LOG" | head -n 8 >&2
  exit 1
fi
# TASK-0049B PR-B3c: restart-counter truth is fatal in both directions — a
# count mismatch means init lost bumps (or the selftest read a stale value),
# and a persist FAIL means the supervisor silently forgot its history.
for m in \
  "SELFTEST: service restart FAIL" \
  "SELFTEST: crash-loop count FAIL" \
  "init: FAIL supervision persist"; do
  if grep -aFq "$m" "$UART_LOG"; then
    echo "[error] first_failed_phase=end missing_marker='$m'" >&2
    echo "[error] supervision restart/persistence emitted failure signature: $m" >&2
    grep -a "init: service exit name=\|init: service restarted name=\|init: supervision persist\|SELFTEST: crash-loop\|SELFTEST: service restart" "$UART_LOG" | head -n 12 >&2
    exit 1
  fi
done
# TASK-0051B guards: a degraded container write in a proof boot means the
# canonical at-rest artifact path is broken (the .nmd fallback kept the
# evidence, but the claim ".nxcd is THE artifact" would be fake green); a
# crash-artifact FAIL means the .nxcd never landed or the intermediate
# survived its deletion.
for m in \
  "crash: container write degraded" \
  "SELFTEST: crash artifact FAIL" \
  "SELFTEST: crash redaction FAIL" \
  "SELFTEST: nxra require FAIL" \
  "SELFTEST: nxra accept FAIL" \
  "SELFTEST: nxra replay deny FAIL"; do
  if grep -aFq "$m" "$UART_LOG"; then
    echo "[error] first_failed_phase=exec missing_marker='$m'" >&2
    echo "[error] crash-evidence-at-rest emitted failure signature: $m" >&2
    grep -a "crash: \|SELFTEST: crash artifact\|execd: minidump written" "$UART_LOG" | head -n 12 >&2
    exit 1
  fi
done
# TASK-0289 A4 guards: loader failure signatures are fatal in EVERY clean
# lane. A PANIC means the boot chain died (reset loop); a verify FAIL in a
# lane that stages no broken slot means the trust chain rejected bytes it
# must accept (or the disk went stale) — both are never survivable noise.
if grep -aFq "nxboot: PANIC" "$UART_LOG"; then
  echo "[error] first_failed_phase=bringup missing_marker='nxboot: jump slot=a'" >&2
  echo "[error] first-stage loader panicked (boot chain dead)" >&2
  grep -a "nxboot: " "$UART_LOG" | head -n 8 >&2
  exit 1
fi
if [[ "${PROFILE:-full}" == "ota-tamper" || "${PROFILE:-full}" == "ota-downgrade" ]]; then
  # Backstop lanes expect EXACTLY the slot-b reject in their sequence; a
  # slot-a FAIL still means the trust chain broke and stays fatal.
  if grep -aFq "nxboot: verify FAIL (slot=a" "$UART_LOG"; then
    echo "[error] first_failed_phase=bringup missing_marker='nxboot: verify ok'" >&2
    echo "[error] loader rejected the CLEAN slot in a backstop lane" >&2
    grep -a "nxboot: " "$UART_LOG" | head -n 8 >&2
    exit 1
  fi
elif grep -aFq "nxboot: verify FAIL" "$UART_LOG"; then
  echo "[error] first_failed_phase=bringup missing_marker='nxboot: verify ok'" >&2
  echo "[error] loader rejected a slot in a clean lane (trust chain or stale disk)" >&2
  grep -a "nxboot: " "$UART_LOG" | head -n 8 >&2
  exit 1
fi
# TASK-0315 guard: the partition gate failing open (or the probe dying)
# would silently hand any sender the state partition.
if grep -aFq "SELFTEST: blk cross-partition deny FAIL" "$UART_LOG"; then
  echo "[error] first_failed_phase=bringup missing_marker='SELFTEST: blk cross-partition deny ok'" >&2
  echo "[error] block-plane partition gate not enforced" >&2
  grep -a "virtioblkd: \|SELFTEST: blk" "$UART_LOG" | head -n 8 >&2
  exit 1
fi
# TASK-0036-A guard: a quorum FAIL means health-commit v2 never completed
# (or committed without the full mask) — the OTA health claim would be
# fake green either way.
if [[ "${OTA_PHASE_GUARDS:-1}" == "1" ]] && grep -aFq "SELFTEST: bootctl quorum FAIL" "$UART_LOG"; then
  echo "[error] first_failed_phase=ota missing_marker='SELFTEST: bootctl quorum ok'" >&2
  echo "[error] health-commit quorum did not complete (or committed early)" >&2
  grep -a "bootctld: health quorum\|bootctld: commit deadline\|SELFTEST: ota health\|SELFTEST: bootctl quorum" "$UART_LOG" | head -n 8 >&2
  exit 1
fi
# TASK-0198 Phase 1 guard: the trust deny lane failing means the device
# publisher anchor is not enforced — every downstream OTA verify claim would
# be fake green (the pre-fix hole accepted any archive-supplied key).
if grep -aFq "SELFTEST: updates trust reject FAIL" "$UART_LOG"; then
  echo "[error] first_failed_phase=ota missing_marker='SELFTEST: updates trust reject ok'" >&2
  echo "[error] update trust anchor not enforced (self-signed archive was accepted)" >&2
  grep -a "updated: stage rejected\|updated: ready\|SELFTEST: updates trust" "$UART_LOG" | head -n 8 >&2
  exit 1
fi
# TASK-0050 PR-1 guards: the boot-state authority must come up against a
# readable record — "defaults" markers in a proof boot mean statefsd was
# unreachable (or the record rotted), which would fake-green every
# OTA/target claim downstream.
for m in \
  "bootctld: record unavailable (defaults)" \
  "bootctld: record corrupt (defaults)"; do
  if grep -aFq "$m" "$UART_LOG"; then
    echo "[error] first_failed_phase=bringup missing_marker='$m'" >&2
    echo "[error] bootctld emitted failure signature: $m" >&2
    grep -a "bootctld:" "$UART_LOG" | head -n 6 >&2
    exit 1
  fi
done

# TASK-0050 PR-3 reset-lane guards: the proof is a REAL reboot — two boots
# in one uart stream, request on boot 1, ok strictly on boot 2 (sentinel).
if [[ "${REQUIRE_RESET_PROOF:-0}" == "1" ]]; then
  # TASK-0050 PR-5: the lane is a THREE-boot cycle now
  # (normal → recovery graph → normal). Substring counts on purpose: UART
  # lines may carry CR endings, which a `$` anchor silently refuses.
  ready_count=$(grep -ac "init: ready" "$UART_LOG" || true)
  if [[ "${ready_count:-0}" -lt 3 ]]; then
    echo "[error] first_failed_phase=bringup missing_marker='init: ready (three boots)'" >&2
    echo "[error] reset lane: expected THREE boots in one uart stream, saw $ready_count" >&2
    grep -a "SELFTEST: reset\|bootctld: reset\|stage graph" "$UART_LOG" | head -n 8 >&2
    exit 1
  fi
  for m in \
    "SELFTEST: reset request" \
    "bootctld: reset (reboot)" \
    "SELFTEST: reset ok" \
    "bootctld: target=normal next=recovery" \
    "init: next boot target=recovery" \
    "init: stage graph target=recovery" \
    "init: stage graph drivers skipped (recovery)" \
    "SELFTEST: recovery graph reached" \
    "SELFTEST: boot target roundtrip ok" \
    "SELFTEST: recovery cycle ok" \
    "statefsd: fsck repaired (n=1)" \
    "statefsd: fsck busy (open txns)" \
    "statefsd: fsck check ok (clean)" \
    "SELFTEST: recovery fsck ok" \
    "bootctld: commit blocked (target=recovery)" \
    "SELFTEST: recovery slot ok" \
    "SELFTEST: recovery ops deny ok"; do
    if ! grep -aFq "$m" "$UART_LOG"; then
      echo "[error] first_failed_phase=bringup missing_marker='$m'" >&2
      echo "[error] reset lane: cycle chain marker missing" >&2
      grep -a "SELFTEST: reset\|SELFTEST: recovery\|bootctld: reset\|stage graph" "$UART_LOG" | head -n 8 >&2
      exit 1
    fi
  done
  # The recovery boot must NOT have resumed the display stack. Segment
  # check (counting is fragile — a full boot prints windowd: ready twice):
  # boot 2 spans the 2nd..3rd `init: ready`; no windowd line may fall there.
  r2=$(grep -an "init: ready" "$UART_LOG" | sed -n 2p | cut -d: -f1)
  r3=$(grep -an "init: ready" "$UART_LOG" | sed -n 3p | cut -d: -f1)
  if [[ -n "$r2" && -n "$r3" ]] \
     && sed -n "${r2},${r3}p" "$UART_LOG" | grep -aq "windowd: ready"; then
    echo "[error] first_failed_phase=bringup missing_marker='drivers suspended in recovery'" >&2
    echo "[error] reset lane: windowd came up INSIDE the recovery boot segment" >&2
    exit 1
  fi
fi
for m in \
  "SELFTEST: reset request FAIL" \
  "SELFTEST: boot target roundtrip FAIL" \
  "SELFTEST: recovery fsck FAIL" \
  "SELFTEST: recovery slot FAIL" \
  "SELFTEST: recovery ops deny FAIL" \
  "statefsd: fsck fail (unrecoverable)" \
  "bootctld: reset refused"; do
  if grep -aFq "$m" "$UART_LOG"; then
    echo "[error] first_failed_phase=bringup missing_marker='$m'" >&2
    echo "[error] reset path emitted failure signature: $m" >&2
    grep -a "SELFTEST: reset\|bootctld: reset" "$UART_LOG" | head -n 6 >&2
    exit 1
  fi
done

# TASK-0049C evidence-journal guards: the persisted-scope proofs are fatal
# in both directions, and a proof boot that ran the logd phase must have
# attached the spill (the persist-on marker carries the honest count of
# records loaded FROM DISK). Degrade/spill-fail signatures are fatal on any
# profile (they cannot appear unless the spill path ran and broke).
if grep -aFq "SELFTEST: log query ok" "$UART_LOG" \
   && ! grep -aFq "logd: evidence persist on (loaded=0x" "$UART_LOG"; then
  echo "[error] first_failed_phase=logd missing_marker='logd: evidence persist on (loaded=0x'" >&2
  echo "[error] logd phase ran but the evidence spill never attached to statefsd" >&2
  grep -a "logd: evidence\|logd: degrade" "$UART_LOG" | head -n 6 >&2
  exit 1
fi
for m in \
  "SELFTEST: evidence query FAIL" \
  "SELFTEST: evidence budget FAIL" \
  "logd: degrade evidence volatile" \
  "logd: evidence spill fail (txn)"; do
  if grep -aFq "$m" "$UART_LOG"; then
    echo "[error] first_failed_phase=logd missing_marker='$m'" >&2
    echo "[error] persistent evidence journal emitted failure signature: $m" >&2
    grep -a "logd: evidence\|SELFTEST: evidence" "$UART_LOG" | head -n 8 >&2
    exit 1
  fi
done

# TASK-0049 / RFC-0087 exhaustion-is-an-event guard: a proof boot MUST
# upgrade statefs to virtio — a degrade marker means /state durability was
# silently RAM-only for the whole run, which would fake-green every
# persistence claim downstream (cold-boot lanes would catch it a run later;
# this catches it in the run that degraded).
if grep -aq "statefsd: degrade ram-backed" "$UART_LOG"; then
  m=$(grep -a "statefsd: degrade ram-backed" "$UART_LOG" | head -n1)
  echo "[error] first_failed_phase=bringup missing_marker='statefsd: virtio upgrade ok'" >&2
  echo "[error] statefs degraded to RAM-only in a proof boot: $m" >&2
  print_uart_excerpt "statefsd: ready" "SELFTEST: statefs put ok"
  exit 1
fi
for m in \
  "SELFTEST: statefs enc roundtrip FAIL" \
  "statefsd: enc self-check failed" \
  "statefsd: enc meta invalid"; do
  if grep -aFq "$m" "$UART_LOG"; then
    echo "[error] first_failed_phase=bringup missing_marker='$m'" >&2
    echo "[error] statefs record encryption emitted failure signature: $m" >&2
    print_uart_excerpt "statefsd: journal v2 mounted (2PC)" "SELFTEST: statefs v2 compact ok"
    exit 1
  fi
done
if grep -aFq "SELFTEST: ui resize ok" "$UART_LOG" && ! grep -aFq "SELFTEST: ui launcher present ok" "$UART_LOG"; then
  echo "[error] first_failed_phase=end missing_marker='SELFTEST: ui launcher present ok'" >&2
  echo "[error] UI resize marker appeared without launcher-present proof" >&2
  print_uart_excerpt "${PHASE_START_MARKER[end]}" "SELFTEST: sandbox deny ok"
  exit 1
fi

# Lanes that run WITHOUT a display/GPU device. Their guest still emits the UI
# summary markers, but never the scanout/visible prerequisites, so the display
# fake-green guards below must not run for them. ONE list, three consumers: the
# same predicate used to live as three hand-maintained `!=` chains, and adding
# the `smp1` lane to two of them but not the third turned the deterministic gate
# red with "GPU chain contract broken" on a headless boot (2026-07-25).
profile_has_display() {
  case "${PROFILE:-full}" in
    headless | smp | smp1 | reset | dhcp | dhcp-strict | quic-required | os2vm | supply-chain) return 1 ;;
    *) return 0 ;;
  esac
}

# TASK-0055B fake-green guard (GPU-capable profiles): the guest marker summarizes a
# configured GPU scanout and must not appear without mode/present/handoff prerequisites.
# Only enforced for GPU-capable profiles (headless has no virtio-gpu device).
if profile_has_display; then
if grep -aFq "SELFTEST: ui v2 present ok" "$UART_LOG"; then
  for m in \
    "display: bootstrap on" \
    "display: mode 1280x800 argb8888" \
    "windowd: present ok (seq=1 dmg=1)" \
    "windowd: fb handoff to gpud ok" \
    "gpud: scanout ok"; do
    if ! grep -aFq "$m" "$UART_LOG"; then
      echo "[error] CONTRACT VIOLATION: ui v2 present ok without '$m'" >&2
      echo "[error] GPU chain contract broken — display markers are false positives" >&2
      exit 1
    fi
  done
fi
fi

# TASK-0055C visible-present fake-green guard: the UI visible marker summarizes
# a configured visible backend, visible present, scanout, and SystemUI first frame.
# Skip for headless/display-gpu profiles which run UI phases without GPU scanout.
# (`display-gpu` HAS a GPU device but no visible backend, hence the extra term.)
if profile_has_display && [[ "${PROFILE:-full}" != "display-gpu" ]]; then
if grep -aFq "SELFTEST: ui visible present ok" "$UART_LOG"; then
  for m in \
    "gpud: virtio-gpu probed" \
    "gpud: scanout ok" \
    "gpud: cursor on" \
    "gpud: ready" \
    "display: bootstrap on" \
    "display: mode 1280x800 argb8888" \
    "windowd: backend=visible" \
    "windowd: present visible ok" \
    "layout: engine on" \
    "text: wrapping on" \
    "display: first scanout ok" \
    "systemui: first frame visible"; do
    if ! grep -aFq "$m" "$UART_LOG"; then
      echo "[error] first_failed_phase=end missing_marker='$m'" >&2
      echo "[error] UI visible marker appeared before required visible-present proof: $m" >&2
      print_uart_excerpt "${PHASE_START_MARKER[end]}" "SELFTEST: sandbox deny ok"
      exit 1
    fi
  done
fi

# TASK-0056B visible-input fake-green guard: the visible-input marker summarizes
# routed pointer movement, focus transfer, launcher click, and visible frame state.
if grep -aFq "SELFTEST: ui visible input ok" "$UART_LOG"; then
  for m in \
    "SELFTEST: ui visible present ok" \
    "hidrawd: virtio-input mmio ready" \
    "hidrawd: virtio-input keyboard ready" \
    "hidrawd: virtio-input pointer ready" \
    "hidrawd: virtio-input raw event seen" \
    "hidrawd: ingress adapter ready" \
    "inputd: live pointer route on" \
    "inputd: live keyboard route on" \
    "windowd: input visible on" \
    "windowd: full-window color visible" \
    "windowd: cursor move visible" \
    "windowd: hover visible" \
    "windowd: focus visible" \
    "launcher: click visible ok" \
    "windowd: keyboard visible"; do
    if ! grep -aFq "$m" "$UART_LOG"; then
      echo "[error] first_failed_phase=end missing_marker='$m'" >&2
      echo "[error] UI visible input marker appeared before required visible-input proof: $m" >&2
      print_uart_excerpt "${PHASE_START_MARKER[end]}" "SELFTEST: sandbox deny ok"
      exit 1
    fi
  done
fi
fi

# TASK-0253 visible-wheel fake-green guard: the wheel marker summarizes the
# routed visible-input proof plus a real transient wheel indicator.
if grep -aFq "SELFTEST: ui visible wheel ok" "$UART_LOG"; then
  for m in \
    "SELFTEST: ui visible input ok" \
    "windowd: wheel visible"; do
    if ! grep -aFq "$m" "$UART_LOG"; then
      echo "[error] first_failed_phase=end missing_marker='$m'" >&2
      echo "[error] UI visible wheel marker appeared before required wheel proof: $m" >&2
      print_uart_excerpt "${PHASE_START_MARKER[end]}" "SELFTEST: sandbox deny ok"
      exit 1
    fi
  done
fi

# TASK-0057 DisplayServer v0 fake-green guard: the v2b summary marker is valid
# only after service-owned asset and scanout evidence is visible.
if grep -aFq "SELFTEST: ui v2b assets ok" "$UART_LOG"; then
  for m in \
    "SELFTEST: ui visible wheel ok" \
    "windowd: cursor svg loaded" \
    "windowd: wallpaper visible" \
    "windowd: text target visible" \
    "windowd: icon target visible"; do
    if ! grep -aFq "$m" "$UART_LOG"; then
      echo "[error] fake-green guard: '$m' missing before ui v2b assets ok" >&2
      exit 1
    fi
  done
fi

# TASK-0062 real-GPU animation fake-green guard: the v5 transition marker is
# only valid after gpud hardware markers and windowd animation bridge markers.
if grep -aFq "SELFTEST: ui v5 transition ok" "$UART_LOG"; then
  for m in \
    "gpud: virtio-gpu probed" \
    "gpud: scanout ok" \
    "gpud: cursor on" \
    "gpud: ready" \
    "SELFTEST: ui v2b assets ok" \
    "uiruntime: on" \
    "uianim: timeline on" \
    "windowd: implicit transitions on" \
    "uiruntime: batch commit ok" \
    "windowd: live transition ok" \
    "uianim: spring converge ok"; do
    if ! grep -aFq "$m" "$UART_LOG"; then
      echo "[error] first_failed_phase=end missing_marker='$m'" >&2
      echo "[error] UI v5 transition marker appeared before required real-GPU animation proof: $m" >&2
      print_uart_excerpt "${PHASE_START_MARKER[end]}" "SELFTEST: ui v2b assets ok"
      exit 1
    fi
  done
fi

prev=-1
for marker in "${expected_sequence[@]}"; do
  line=$(grep -aFn "$marker" "$UART_LOG" | head -n1 | cut -d: -f1 || true)
  if [[ -z "$line" ]]; then
    # If we never matched the full sequence, tolerate missing lines here; the
    # final RUN_UNTIL_MARKER gating below decides success/failure.
    break
  fi
  # Only enforce strict ordering for phase-critical init markers (init: start/up/ready).
  # All other markers (service-internal state, SELFTEST:, async chatter) can reorder.
  case "$marker" in
    "init: start"|"init: start "*|"init: up "*|"init: ready"|"KSELFTEST:"*)
      ;;
    *)
      continue
      ;;
  esac
  if [[ "$prev" -ne -1 && "$line" -le "$prev" ]]; then
    echo "[error] Marker out of order: $marker (line $line)" >&2
    print_uart_excerpt "${PHASE_START_MARKER[bring-up]}" ""
    exit 1
  fi
  prev=$line
done

# Additional required policy checks only apply when policy markers are within the active ladder.
for policy_marker in \
  "SELFTEST: policy allow ok" \
  "SELFTEST: policy deny ok" \
  "SELFTEST: policy malformed ok" \
  "SELFTEST: bundlemgrd route execd denied ok"; do
  if grep -aFq "$policy_marker" <(printf "%s\n" "${expected_sequence[@]}"); then
    if ! grep -aFq "$policy_marker" "$UART_LOG"; then
      echo "[error] first_failed_phase=policy missing_marker='$policy_marker'" >&2
      echo "[error] Missing UART marker: $policy_marker" >&2
      print_uart_excerpt "${PHASE_START_MARKER[policy]}" ""
      exit 1
    fi
  fi
done

trim_log "$QEMU_LOG" "$QEMU_LOG_MAX"
trim_log "$UART_LOG" "$UART_LOG_MAX"

if [[ "$qemu_status" -ne 0 && "$RUN_UNTIL_MARKER" != "1" ]]; then
  echo "[warn] QEMU exited with status $qemu_status" >&2
fi

# TASK-0023B P4-09: deny-by-default analyzer post-pass.
# Profile MUST be set (defaults to `full` if --profile= was not given);
# any UART marker that the manifest does not expect for the profile, or
# that the profile lists as forbidden, fails this script. Skip if the
# CLI is unavailable (e.g. minimal sandboxed bring-up that hasn't built
# nexus-proof-manifest yet) — non-strict by env opt-out for now; will
# become hard-required in P4-10.
PM_VERIFY_UART=${PM_VERIFY_UART:-1}
# Skip manifest verify-uart for the non-display lanes + display-gpu — manifest
# markers are still being populated for them (same one list as the guards above).
if ! profile_has_display || [[ "${PROFILE:-full}" == "display-gpu" ]]; then
  PM_VERIFY_UART=0
fi
if [[ "$PM_VERIFY_UART" == "1" ]]; then
  PM_PROFILE_FOR_VERIFY=${PROFILE:-full}
  # #region agent log (H1: profile-vs-SMP mismatch capture)
  agent_debug_log "$RUN_ID" "H1" "scripts/qemu-test.sh:verify-uart-pre" \
    "verify-uart preconditions: PROFILE used vs SMP harts in image" \
    "{\"profile_for_verify\":\"$PM_PROFILE_FOR_VERIFY\",\"profile_arg_was_set\":\"${PROFILE:-<unset>}\",\"smp\":\"${SMP:-<unset>}\",\"require_smp\":\"${REQUIRE_SMP:-0}\",\"makelevel\":\"${MAKELEVEL:-}\"}"
  # #endregion
  cli=$(pm_cli 2>/dev/null || true)
  if [[ -n "$cli" && -x "$cli" && -f "$UART_LOG" ]]; then
    verify_out=$("$cli" verify-uart \
        --profile="$PM_PROFILE_FOR_VERIFY" \
        --manifest="$NEXUS_PROOF_MANIFEST_PATH" \
        --uart="$UART_LOG" 2>&1) && verify_rc=0 || verify_rc=$?
    # Echo to stderr exactly like before so callers see the message verbatim.
    printf '%s\n' "$verify_out" >&2
    # #region agent log (H1: verify-uart result + first 6 unexpected/forbidden lines)
    # Extract the first 6 violation literals (substring after "  - ") for diagnostics; keep payload bounded.
    violations=$(printf '%s\n' "$verify_out" | awk '/^  - /{ sub(/^  - /, ""); print; n++; if (n>=6) exit }' | tr '\n' '|' | sed 's/|$//')
    agent_debug_log "$RUN_ID" "H1" "scripts/qemu-test.sh:verify-uart-post" \
      "verify-uart result for profile vs hardware SMP" \
      "{\"profile_for_verify\":\"$PM_PROFILE_FOR_VERIFY\",\"smp\":\"${SMP:-<unset>}\",\"verify_rc\":$verify_rc,\"first_violations\":\"${violations//\"/\\\"}\"}"
    # #endregion
    if [[ "$verify_rc" == "0" ]]; then
      echo "[info] verify-uart: profile=$PM_PROFILE_FOR_VERIFY clean" >&2
    else
      echo "[error] verify-uart failed for profile=$PM_PROFILE_FOR_VERIFY" >&2
      exit 1
    fi
  else
    echo "[warn] verify-uart skipped (CLI not available); set PM_VERIFY_UART=0 to silence" >&2
  fi
fi

# TASK-0023B P5-05: post-pass evidence bundle (assemble + seal).
#
# Runs after verify-uart succeeds; failure here is fatal because a
# successful run that fails to seal would silently drop the audit
# trail on the floor. Knobs:
#
#   NEXUS_EVIDENCE_DISABLE=1   skip the seal step (rejected when CI=1)
#   NEXUS_EVIDENCE_SEAL=1      force mandatory seal even outside CI
#   CI=1                       implies NEXUS_EVIDENCE_SEAL=1; also
#                              rejects NEXUS_EVIDENCE_DISABLE=1
#
# Label resolution mirrors tools/seal-evidence.sh: if
# NEXUS_EVIDENCE_CI_PRIVATE_KEY_BASE64 is set the bundle is sealed
# with --label=ci, otherwise with --label=bringup. Bundles land at
# target/evidence/<utc>-<profile>-<gitsha>.tar.gz (gitignored).
seal_required=0
if [[ "${CI:-0}" == "1" || "${NEXUS_EVIDENCE_SEAL:-0}" == "1" ]]; then
  seal_required=1
fi
if [[ "${NEXUS_EVIDENCE_DISABLE:-0}" == "1" ]]; then
  if [[ "$seal_required" == "1" ]]; then
    echo "[error] NEXUS_EVIDENCE_DISABLE=1 is rejected when CI=1 (or NEXUS_EVIDENCE_SEAL=1) — refusing to drop the audit trail" >&2
    exit 1
  fi
  echo "[warn] NEXUS_EVIDENCE_DISABLE=1: skipping post-pass evidence bundle (local dev only)" >&2
else
  evidence_profile=${PROFILE:-full}
  evidence_dir="$ROOT/target/evidence"
  mkdir -p "$evidence_dir"
  utc_stamp=$(date -u +%Y%m%dT%H%M%SZ)
  git_sha=$(git -C "$ROOT" rev-parse --short HEAD 2>/dev/null || echo "no-git")
  evidence_out="$evidence_dir/${utc_stamp}-${evidence_profile}-${git_sha}.tar.gz"

  # Locate (or build) the nexus-evidence CLI. Mirrors pm_cli().
  ne_cli=""
  for cand in \
    "${NEXUS_EVIDENCE_BIN:-}" \
    "$ROOT/target/debug/nexus-evidence" \
    "$ROOT/target/release/nexus-evidence" \
    /tmp/cursor-sandbox-cache/*/cargo-target/debug/nexus-evidence \
    /tmp/cursor-sandbox-cache/*/cargo-target/release/nexus-evidence; do
    if [[ -n "$cand" && -x "$cand" ]]; then
      ne_cli="$cand"
      break
    fi
  done
  if [[ -z "$ne_cli" ]]; then
    echo "[info] building nexus-evidence CLI..." >&2
    (cd "$ROOT" && cargo build -p nexus-evidence --bin nexus-evidence --quiet) 1>&2
    for cand in \
      "$ROOT/target/debug/nexus-evidence" \
      /tmp/cursor-sandbox-cache/*/cargo-target/debug/nexus-evidence; do
      if [[ -x "$cand" ]]; then
        ne_cli="$cand"
        break
      fi
    done
  fi
  if [[ -z "$ne_cli" || ! -x "$ne_cli" ]]; then
    echo "[error] nexus-evidence binary not found — cannot assemble bundle" >&2
    exit 1
  fi

  # Collect a small set of run-side metadata. Anything not available
  # locally is left empty; the bundle schema tolerates missing values
  # but the CI gate requires the host-info string to be non-empty so
  # sealed runs can be partitioned by runner class.
  rustc_ver=$(rustc -V 2>/dev/null || echo "")
  qemu_ver=$("${QEMU:-qemu-system-riscv64}" --version 2>/dev/null | head -n1 || echo "")
  host_info="$(uname -srm 2>/dev/null || echo unknown)"

  echo "[info] assembling evidence bundle: $evidence_out" >&2
  if ! "$ne_cli" assemble \
      --uart="$UART_LOG" \
      --manifest="$NEXUS_PROOF_MANIFEST_PATH" \
      --profile="$evidence_profile" \
      --out="$evidence_out" \
      --kernel-cmdline="${KERNEL_CMDLINE:-}" \
      --host-info="$host_info" \
      --build-sha="$(git -C "$ROOT" rev-parse HEAD 2>/dev/null || echo unknown)" \
      --rustc-version="$rustc_ver" \
      --qemu-version="$qemu_ver" \
      --wall-clock="$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
      --env="PROFILE=$evidence_profile" \
      --env="REQUIRE_SMP=${REQUIRE_SMP:-0}" \
      --env="REQUIRE_QEMU_DHCP=${REQUIRE_QEMU_DHCP:-0}" \
      --env="REQUIRE_DSOFTBUS=${REQUIRE_DSOFTBUS:-0}"; then
    echo "[error] nexus-evidence assemble failed" >&2
    exit 1
  fi

  if [[ "$seal_required" == "1" ]]; then
    seal_label="bringup"
    if [[ -n "${NEXUS_EVIDENCE_CI_PRIVATE_KEY_BASE64:-}" ]]; then
      seal_label="ci"
    fi
    echo "[info] sealing evidence bundle (label=$seal_label)" >&2
    if ! NEXUS_EVIDENCE_BIN="$ne_cli" "$ROOT/tools/seal-evidence.sh" "$evidence_out" --label="$seal_label" 2>/dev/null; then
      echo "[warn] tools/seal-evidence.sh failed for $evidence_out (non-fatal)" >&2
    fi
  else
    echo "[info] evidence bundle assembled unsigned (set NEXUS_EVIDENCE_SEAL=1 to seal)" >&2
  fi
fi

# A3 SMP exec proof: unordered presence check — user execution on a secondary
# hart happens at a workload-dependent point (work stealing), so it cannot be
# a position in expected_sequence. Presence in the log is the requirement.
if [[ "$REQUIRE_SMP" == "1" ]]; then
  # P2 gate: the BKL budget must PASS (declarative budgets in
  # core/trap/budgets.rs) — a returning >10ms convoy fails the boot here.
  if [[ "$(count_lines "KSELFTEST: bkl budget ok")" -lt 1 ]]; then
    echo "[error] BKL budget gate failed: KSELFTEST: bkl budget ok" >&2
    exit 1
  fi
  if [[ "$(count_lines "KSELFTEST: runtime timer budget ok")" -lt 1 ]]; then
    echo "[error] tick budget proof missing: KSELFTEST: runtime timer budget ok" >&2
    exit 1
  fi
  if [[ "$(count_lines "KSELFTEST: smp exec cpu1 ok")" -lt 1 ]]; then
    echo "[error] SMP exec proof missing: KSELFTEST: smp exec cpu1 ok (no user dispatch observed on cpu1)" >&2
    exit 1
  fi
  # A7: every secondary hart must have a live preemption tick.
  if [[ "$(count_lines "KSELFTEST: smp per-hart ticks ok")" -lt 1 ]]; then
    echo "[error] SMP per-hart timer proof missing: KSELFTEST: smp per-hart ticks ok" >&2
    exit 1
  fi
fi

# TASK-0140: the host CLI against the LIVE-produced disk truth — after the
# flip lane the commit raised the anti-downgrade floor (1->2) and left the
# committed standing slot at B; `nx update status` (the same bootfmt codecs
# the loader links) must decode exactly that story from the disk the run
# just wrote. This is the honest "CLI status vs live services" seam: no
# host↔guest transport exists, so the disk the machinery produced IS the
# meeting point.
if [[ "${PROFILE:-full}" == "ota-bundle" ]]; then
  reused_count=$(grep -ac "updated: bundle reused (name=" "$UART_LOG" || true)
  if [[ "$reused_count" -lt "${OTA_BUNDLE_REUSE_MIN:-1}" ]]; then
    echo "[error] ota-bundle: expected >= ${OTA_BUNDLE_REUSE_MIN:-1} reused bundles, uart shows $reused_count" >&2
    exit 1
  fi
  echo "[info] ota-bundle: $reused_count bundles reused from the active volume (min ${OTA_BUNDLE_REUSE_MIN:-1})"
fi
if [[ "${PROFILE:-full}" == "ota-flip" || "${PROFILE:-full}" == "ota-bundle" || "${PROFILE:-full}" == "ota-bundle-delta" ]]; then
  nxupdate_bin="$ROOT/target/release/nx"
  nxupdate_img="${QEMU_BLK_IMG:-$ROOT/build/nexus.img}"
  if ! nxupdate_out=$("$nxupdate_bin" update status --image "$nxupdate_img" 2>&1); then
    echo "[error] verify-nxupdate: nx update status failed on the flip-lane disk" >&2
    echo "$nxupdate_out" >&2
    exit 1
  fi
  if ! grep -q "update: bsb active=b" <<<"$nxupdate_out" \
    || ! grep -q "committed=yes" <<<"$nxupdate_out" \
    || ! grep -q "floor=2" <<<"$nxupdate_out" \
    || ! grep -q "update: slot-b build=otaB" <<<"$nxupdate_out"; then
    echo "[error] verify-nxupdate: CLI status disagrees with the flip lane's end state" >&2
    echo "$nxupdate_out" >&2
    exit 1
  fi
  echo "[info] verify-nxupdate: ok (active=b committed floor=2 build=otaB from $nxupdate_img)"
fi

# Chain-marker contract reconciliation (tools/nx/chains/markers.txt): the
# simulated chain tests and the real boot must agree on the hop markers.
# MARKER_CONTRACT=0 disables (e.g. for exotic manual profiles).
if [[ "${MARKER_CONTRACT:-1}" == "1" ]]; then
  case "${PROFILE:-full}" in
    headless|full|smp|smp1|display-gpu)
      bash "$ROOT/scripts/check-chain-markers.sh" --log "$UART_LOG" --groups input-route,gpu-core,display || exit 1
      ;;
  esac
fi

echo "SELFTEST: Completed (markers verified)" >&2
