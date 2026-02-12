#!/usr/bin/env bash
# Copyright 2024 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
UART_LOG=${UART_LOG:-"$ROOT/uart.log"}
QEMU_LOG=${QEMU_LOG:-"$ROOT/qemu.log"}
RUN_TIMEOUT=${RUN_TIMEOUT:-90s}
RUN_UNTIL_MARKER=${RUN_UNTIL_MARKER:-1}
RUN_PHASE=${RUN_PHASE:-}
REQUIRE_SMP=${REQUIRE_SMP:-0}
QEMU_LOG_MAX=${QEMU_LOG_MAX:-52428800}
UART_LOG_MAX=${UART_LOG_MAX:-10485760}
DEBUG_LOG=${DEBUG_LOG:-"$ROOT/.cursor/debug.log"}
DEBUG_SESSION_ID=${DEBUG_SESSION_ID:-""}
AGENT_RUN_ID=${AGENT_RUN_ID:-"qemu-$(date +%s)-$$"}

# #region agent log (ndjson debug log helper; Slice B)
agent_debug_log() {
  local run_id=$1
  local hypothesis_id=$2
  local location=$3
  local message=$4
  local data_json=${5:-"{}"}
  local ts
  ts=$(date +%s%3N 2>/dev/null || date +%s000)
  # Keep JSON stable and small. data_json must be a valid JSON object string.
  if [[ -n "$DEBUG_SESSION_ID" ]]; then
    printf '{"sessionId":"%s","runId":"%s","hypothesisId":"%s","location":"%s","message":"%s","data":%s,"timestamp":%s}\n' \
      "$DEBUG_SESSION_ID" "$run_id" "$hypothesis_id" "$location" "$message" "$data_json" "$ts" >>"$DEBUG_LOG" 2>/dev/null || true
  else
    printf '{"runId":"%s","hypothesisId":"%s","location":"%s","message":"%s","data":%s,"timestamp":%s}\n' \
      "$run_id" "$hypothesis_id" "$location" "$message" "$data_json" "$ts" >>"$DEBUG_LOG" 2>/dev/null || true
  fi
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
  agent_debug_log "$AGENT_RUN_ID" "A" "scripts/qemu-test.sh:exit" "qemu smoke exit summary" \
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

QEMU_EXTRA_ARGS=()
if [[ "${DEBUG_QEMU:-0}" == "1" ]]; then
  QEMU_EXTRA_ARGS+=(-S -gdb tcp:localhost:1234)
fi


# RFC-0014 Phase 2: phase mapping for QEMU smoke triage + early exit.
# A "phase" is a named slice of the marker ladder. Failures should report the first failing phase.
declare -a PHASES=(
  "bring-up"
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
  ["mmio"]="execd: ready"
  ["routing"]="SELFTEST: ipc routing keystored ok"
  ["ota"]="SELFTEST: ota stage ok"
  ["policy"]="SELFTEST: policy allow ok"
  ["logd"]="logd: ready"
  ["vfs"]="SELFTEST: vfs stat ok"
  ["end"]="SELFTEST: end"
)
declare -A PHASE_END_MARKER=(
  ["bring-up"]="execd: ready"
  ["mmio"]="SELFTEST: cap query vmo ok"
  ["routing"]="SELFTEST: ipc routing ok"
  ["ota"]="SELFTEST: ota rollback ok"
  ["policy"]="SELFTEST: policy malformed ok"
  ["logd"]="SELFTEST: log query ok"
  ["vfs"]="SELFTEST: vfs ebadf ok"
  ["end"]="SELFTEST: end"
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
expected_sequence=(
  "neuron vers."
  "KSELFTEST: spawn reasons ok"
  "KSELFTEST: resource sentinel ok"
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
  "init: start netstackd"
  "init: up netstackd"
  "init: start dsoftbusd"
  "init: up dsoftbusd"
  "init: ready"
  # Service readiness markers are emitted asynchronously by the spawned processes.
  # With the kernel `exec` loader path, init emits spawn markers first, then yields;
  # services report `*: ready` after `init: ready`.
  "keystored: ready"
  "rngd: ready"
  "policyd: ready"
  "samgrd: ready"
  "bundlemgrd: ready"
  "statefsd: ready"
  "updated: ready (statefs)"
  "packagefsd: ready"
  "vfsd: ready"
  "execd: ready"
  "timed: ready"
  "netstackd: ready"
  "net: virtio-net up"
  "SELFTEST: net iface ok"
  "net: smoltcp iface up"
  "blk: virtio-blk up"
  "logd: ready"
  "bundlemgrd: slot a active"
  "SELFTEST: ipc routing keystored ok"
  "SELFTEST: keystored v1 ok"
  "SELFTEST: qos ok"
  "SELFTEST: timed coalesce ok"
  "rngd: mmio window mapped ok"
  "SELFTEST: rng entropy ok"
  "SELFTEST: rng entropy oversized ok"
  "SELFTEST: device key pubkey ok"
  "SELFTEST: device key private export rejected ok"
  "SELFTEST: statefs put ok"
  "SELFTEST: statefs unauthorized access rejected"
  "SELFTEST: statefs persist ok"
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
  "SELFTEST: bundlemgrd v1 image ok"
  "SELFTEST: bundlemgrd v1 malformed ok"
  # TASK-0007 OTA proof: stage → switch → health gate → rollback (userspace-only, non-persistent)
  "SELFTEST: ota stage ok"
  "bundlemgrd: slot b active"
  "SELFTEST: ota switch ok"
  "SELFTEST: ota health ok"
  "SELFTEST: ota rollback ok"
  "SELFTEST: bootctl persist ok"
  "SELFTEST: policy allow ok"
  "SELFTEST: policy deny ok"
  "SELFTEST: mmio policy deny ok"
  "SELFTEST: policyd requester spoof denied ok"
  "SELFTEST: policy malformed ok"
  "SELFTEST: ipc routing execd ok"
  "child: hello-elf"
  "execd: elf load ok"
  "SELFTEST: e2e exec-elf ok"
  "child: exit0 start"
  "execd: child exited"
  "SELFTEST: child exit ok"
  "child: exit42 start"
  "execd: crash report pid="
  "SELFTEST: crash report ok"
  "SELFTEST: exec denied ok"
  "SELFTEST: execd malformed ok"
  "SELFTEST: log query ok"
  "SELFTEST: nexus-log sink-logd ok"
  "SELFTEST: core services log ok"
  "SELFTEST: ipc payload roundtrip ok"
  "SELFTEST: ipc deadline timeout ok"
  "SELFTEST: nexus-ipc kernel loopback ok"
  "SELFTEST: ipc sender pid ok"
  "SELFTEST: ipc sender service_id ok"
  "SELFTEST: mmio map ok"
  "SELFTEST: cap query mmio ok"
  "SELFTEST: cap query vmo ok"
  "SELFTEST: ipc routing ok"
  "SELFTEST: ipc routing packagefsd ok"
  "SELFTEST: vfs stat ok"
  "SELFTEST: vfs read ok"
  "SELFTEST: vfs real data ok"
  "SELFTEST: vfs ebadf ok"
  "SELFTEST: end"
)

if [[ "$REQUIRE_SMP" == "1" ]]; then
  if [[ "${SMP:-1}" -lt 2 ]]; then
    echo "[error] REQUIRE_SMP=1 requires SMP>=2 (current SMP=${SMP:-1})" >&2
    exit 2
  fi
  smp_markers=(
    "KINIT: cpu1 online"
    "KSELFTEST: smp online ok"
    "KSELFTEST: ipi counterfactual ok"
    "KSELFTEST: ipi resched ok"
    "KSELFTEST: test_reject_invalid_ipi_target_cpu ok"
    "KSELFTEST: test_reject_offline_cpu_resched ok"
    "KSELFTEST: work stealing ok"
    "KSELFTEST: test_reject_steal_above_bound ok"
    "KSELFTEST: test_reject_steal_higher_qos ok"
  )
  # Kernel SMP selftests run before userspace init markers.
  expected_sequence=(
    "${expected_sequence[@]:0:3}"
    "${smp_markers[@]}"
    "${expected_sequence[@]:3}"
  )
fi

# Optional: stop and validate only up to a given phase.
if [[ -n "$RUN_PHASE" ]]; then
  if [[ -z "${PHASE_END_MARKER[$RUN_PHASE]:-}" ]]; then
    print_phase_help
    exit 2
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
agent_debug_log "$AGENT_RUN_ID" "A" "scripts/qemu-test.sh:pre-run" "qemu smoke start" \
  "{\"run_timeout\":\"$RUN_TIMEOUT\",\"run_phase\":\"${RUN_PHASE:-}\",\"run_until_marker\":\"$RUN_UNTIL_MARKER\",\"require_smp\":\"${REQUIRE_SMP:-0}\",\"require_dhcp\":\"${REQUIRE_QEMU_DHCP:-0}\",\"require_dhcp_strict\":\"${REQUIRE_QEMU_DHCP_STRICT:-0}\",\"require_dsoftbus\":\"${REQUIRE_DSOFTBUS:-0}\",\"qemu_icount_args\":\"${QEMU_ICOUNT_ARGS:-}\"}"
RUN_TIMEOUT="$RUN_TIMEOUT" \
RUN_UNTIL_MARKER="$RUN_UNTIL_MARKER" \
QEMU_LOG="$QEMU_LOG" \
UART_LOG="$UART_LOG" \
QEMU_LOG_MAX="$QEMU_LOG_MAX" \
UART_LOG_MAX="$UART_LOG_MAX" \
"$ROOT/scripts/run-qemu-rv64.sh" "${QEMU_EXTRA_ARGS[@]}"
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
agent_debug_log "$AGENT_RUN_ID" "J" "scripts/qemu-test.sh:diag-runqemu-result" "run-qemu completion and log artifacts" \
  "{\"qemu_status\":$qemu_status,\"uart_exists\":$uart_exists,\"uart_size\":$uart_size,\"qemu_exists\":$qemu_exists,\"qemu_size\":$qemu_size}"
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
agent_debug_log "$AGENT_RUN_ID" "B" "scripts/qemu-test.sh:diag-kpgf-signature" "exec kpgf syscall signature" \
  "{\"kpgf_seen\":$kpgf_seen,\"pid\":\"${kpgf_pid}\",\"a7\":\"${kpgf_a7}\",\"a0\":\"${kpgf_a0}\",\"a2\":\"${kpgf_a2}\",\"is_as_map_signature\":$h_b_as_map_path}"
# #endregion agent log

h_c_timed_correlated=false
if [[ "$line_kpgf" -gt 0 && "$line_timed_ready" -gt 0 && "$line_timed_ok" -gt 0 && "$line_timed_ok" -lt "$line_kpgf" ]]; then
  h_c_timed_correlated=true
fi
# #region agent log (hypothesis C)
agent_debug_log "$AGENT_RUN_ID" "C" "scripts/qemu-test.sh:diag-timed-order" "timed markers before crash" \
  "{\"line_timed_ready\":$line_timed_ready,\"line_timed_ok\":$line_timed_ok,\"line_kpgf\":$line_kpgf,\"timed_before_kpgf\":$h_c_timed_correlated}"
# #endregion agent log

h_d_exec_progress=false
if [[ "$line_execd_ready" -gt 0 && "$line_kpgf" -gt 0 && ( "$line_child_hello" -eq 0 || "$line_kpgf" -lt "$line_child_hello" ) ]]; then
  h_d_exec_progress=true
fi
# #region agent log (hypothesis D)
agent_debug_log "$AGENT_RUN_ID" "D" "scripts/qemu-test.sh:diag-exec-progress" "exec phase progression around crash" \
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
agent_debug_log "$AGENT_RUN_ID" "E" "scripts/qemu-test.sh:diag-address-range" "fault address relative to syscall range" \
  "{\"stval\":\"${kpgf_stval}\",\"a0\":\"${kpgf_a0}\",\"a2\":\"${kpgf_a2}\",\"fault_inside_a0_a0_plus_a2\":$h_e_range_overlap}"
# #endregion agent log

h_f_exec_route_ready=false
if [[ "$line_exec_routing" -gt 0 && "$line_kpgf" -gt 0 && "$line_exec_routing" -lt "$line_kpgf" ]]; then
  h_f_exec_route_ready=true
fi
# #region agent log (hypothesis F)
agent_debug_log "$AGENT_RUN_ID" "F" "scripts/qemu-test.sh:diag-exec-routing" "exec routing readiness before crash" \
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
agent_debug_log "$AGENT_RUN_ID" "K" "scripts/qemu-test.sh:diag-qemu-launch-errors" "qemu launch error signatures" \
  "{\"qemu_exists\":$qemu_exists,\"qemu_launch_fail\":$qemu_launch_fail,\"qemu_lock_conflict\":$qemu_lock_conflict}"
# #endregion agent log
# #endregion agent log

# Require os-lite userspace init markers; kernel fallback no longer accepted
if ! grep -aFq "init: start" "$UART_LOG"; then
  echo "[error] os-lite init markers not found (init: start); userspace bring-up required" >&2
  exit 1
fi

missing=0
missing_marker=""
missing_pos=-1
for marker in "${expected_sequence[@]}"; do
  missing_pos=$((missing_pos + 1))
  if ! grep -aFq "$marker" "$UART_LOG"; then
    missing=1
    missing_marker="$marker"
    break
  fi
done
if [[ "$missing" -ne 0 ]]; then
  failed_phase="unknown"
  # Map missing marker position to the earliest phase whose end marker would include it.
  for phase in "${PHASES[@]}"; do
    end_marker="${PHASE_END_MARKER[$phase]}"
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
  if grep -aFq "net: dhcp bound" "$UART_LOG"; then
    for m in \
      "SELFTEST: net ping ok" \
      "SELFTEST: net udp dns ok" \
      "SELFTEST: icmp ping ok"; do
      if ! grep -aFq "$m" "$UART_LOG"; then
        echo "[error] first_failed_phase=mmio missing_marker='$m'" >&2
        echo "[error] Missing UART marker (REQUIRE_QEMU_DHCP=1): $m" >&2
        print_uart_excerpt "${PHASE_START_MARKER[mmio]}" "SELFTEST: net iface ok"
        exit 1
      fi
    done
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
  agent_debug_log "$AGENT_RUN_ID" "A" "scripts/qemu-test.sh:post-run" "qemu smoke uart summary" \
    "{\"exit_code\":$qemu_status,\"dhcp_bound\":$dhcp_bound,\"dhcp_fallback\":$dhcp_fallback,\"first_failed_phase\":\"${failed_phase:-}\",\"missing_marker\":\"$mm\"}"
}
# #endregion agent log

# Optional DSoftBus E2E proof:
# - Default QEMU smoke does not require cross-node DSoftBus behavior (that proof is covered by
#   the dedicated 2-VM harness: `just os2vm` / `tools/os2vm.sh`).
# - When REQUIRE_DSOFTBUS=1, enforce the DSoftBus marker ladder.
REQUIRE_DSOFTBUS=${REQUIRE_DSOFTBUS:-0}
if [[ "$REQUIRE_DSOFTBUS" == "1" ]]; then
  for m in \
    "dsoftbusd: discovery up (udp loopback)" \
    "dsoftbusd: discovery announce sent" \
    "dsoftbusd: discovery peer found device=local" \
    "dsoftbusd: os transport up (udp+tcp)" \
    "dsoftbusd: session connect peer=node-b" \
    "dsoftbusd: identity bound peer=node-b" \
    "dsoftbusd: dual-node session ok" \
    "dsoftbusd: ready" \
    "dsoftbusd: auth ok" \
    "dsoftbusd: os session ok" \
    "SELFTEST: dsoftbus os connect ok" \
    "SELFTEST: dsoftbus ping ok"; do
    if ! grep -aFq "$m" "$UART_LOG"; then
      echo "[error] first_failed_phase=routing missing_marker='$m'" >&2
      echo "[error] Missing UART marker (REQUIRE_DSOFTBUS=1): $m" >&2
      print_uart_excerpt "${PHASE_START_MARKER[routing]}" "netstackd: facade up"
      exit 1
    fi
  done
fi

prev=-1
for marker in "${expected_sequence[@]}"; do
  line=$(grep -aFn "$marker" "$UART_LOG" | head -n1 | cut -d: -f1)
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
echo "QEMU selftest completed (markers verified)." >&2
