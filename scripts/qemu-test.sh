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
QEMU_LOG_MAX=${QEMU_LOG_MAX:-52428800}
UART_LOG_MAX=${UART_LOG_MAX:-10485760}

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
  "init: start virtioblkd"
  "init: up virtioblkd"
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
  "updated: ready (non-persistent)"
  "packagefsd: ready"
  "vfsd: ready"
  "execd: ready"
  "netstackd: ready"
  "virtioblkd: ready"
  "net: virtio-net up"
  "SELFTEST: net iface ok"
  "net: dhcp bound"
  "net: smoltcp iface up"
  "SELFTEST: net ping ok"
  "virtioblkd: mmio window mapped ok"
  "logd: ready"
  "bundlemgrd: slot a active"
  "SELFTEST: ipc routing keystored ok"
  "SELFTEST: keystored v1 ok"
  "rngd: mmio window mapped ok"
  "SELFTEST: rng entropy ok"
  "SELFTEST: rng entropy oversized ok"
  "SELFTEST: device key pubkey ok"
  "SELFTEST: device key private export rejected ok"
  "SELFTEST: net udp dns ok"
  "SELFTEST: net tcp listen ok"
  "netstackd: facade up"
  "dsoftbusd: discovery up (udp loopback)"
  "dsoftbusd: discovery announce sent"
  "dsoftbusd: discovery peer found device=local"
  "dsoftbusd: os transport up (udp+tcp)"
  "dsoftbusd: session connect peer=node-b"
  "dsoftbusd: identity bound peer=node-b"
  "dsoftbusd: dual-node session ok"
  "dsoftbusd: ready"
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
  "SELFTEST: icmp ping ok"
  "dsoftbusd: auth ok"
  "dsoftbusd: os session ok"
  "SELFTEST: dsoftbus os connect ok"
  "SELFTEST: dsoftbus ping ok"
  "SELFTEST: end"
)

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
RUN_TIMEOUT="$RUN_TIMEOUT" \
RUN_UNTIL_MARKER="$RUN_UNTIL_MARKER" \
QEMU_LOG="$QEMU_LOG" \
UART_LOG="$UART_LOG" \
QEMU_LOG_MAX="$QEMU_LOG_MAX" \
UART_LOG_MAX="$UART_LOG_MAX" \
"$ROOT/scripts/run-qemu-rv64.sh" "${QEMU_EXTRA_ARGS[@]}"
qemu_status=$?
set -e

# SATP hang diagnostic: saw trampoline enter but no post-satp OK
if grep -aFq "AS: trampoline enter" "$UART_LOG" && ! grep -aFq "AS: post-satp OK" "$UART_LOG"; then
  echo "[error] SATP switch hang: trampoline entered but no post-satp marker" >&2
  exit 1
fi

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

prev=-1
for marker in "${expected_sequence[@]}"; do
  line=$(grep -aFn "$marker" "$UART_LOG" | head -n1 | cut -d: -f1)
  if [[ -z "$line" ]]; then
    # If we never matched the full sequence, tolerate missing lines here; the
    # final RUN_UNTIL_MARKER gating below decides success/failure.
    break
  fi
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
