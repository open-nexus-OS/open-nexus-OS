#!/usr/bin/env bash
# Copyright 2024 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
UART_LOG=${UART_LOG:-uart.log}
QEMU_LOG=${QEMU_LOG:-qemu.log}
RUN_TIMEOUT=${RUN_TIMEOUT:-90s}
RUN_UNTIL_MARKER=${RUN_UNTIL_MARKER:-1}
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

# Verify markers printed by the OS stack and selftest client. The harness waits
# for os-lite init to announce each service twice (`init: start <svc>` then
# `init: up <svc>`) before the packagefsd/vfsd readiness markers, the execd
# lifecycle markers, and the selftest tail. If os-lite userspace did not run,
# fall back to kernel selftest completion markers to avoid spurious failures
# during bring-up.
expected_sequence=(
  "neuron vers."
  "init: start"
  "init: start keystored"
  "init: up keystored"
  "init: start policyd"
  "init: up policyd"
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
  "policyd: ready"
  "samgrd: ready"
  "bundlemgrd: ready"
  "packagefsd: ready"
  "vfsd: ready"
  "execd: ready"
  "netstackd: ready"
  "net: virtio-net up"
  "SELFTEST: net iface ok"
  "net: smoltcp iface up 10.0.2.15"
  "SELFTEST: net ping ok"
  "dsoftbusd: ready"
  "SELFTEST: net udp dns ok"
  "SELFTEST: net tcp listen ok"
  "netstackd: facade up"
  "SELFTEST: ipc routing keystored ok"
  "dsoftbusd: discovery up (udp)"
  "SELFTEST: keystored v1 ok"
  "dsoftbusd: discovery announce sent"
  "SELFTEST: ipc routing samgrd ok"
  "dsoftbusd: discovery peer found device=local"
  "dsoftbusd: os transport up (udp+tcp)"
  "SELFTEST: samgrd v1 register ok"
  "SELFTEST: samgrd v1 lookup ok"
  "SELFTEST: samgrd v1 unknown ok"
  "SELFTEST: samgrd v1 malformed ok"
  "SELFTEST: ipc routing policyd ok"
  "SELFTEST: ipc routing bundlemgrd ok"
  "SELFTEST: bundlemgrd v1 list ok"
  "SELFTEST: bundlemgrd v1 image ok"
  "SELFTEST: bundlemgrd v1 malformed ok"
  "SELFTEST: policy allow ok"
  "SELFTEST: policy deny ok"
  "SELFTEST: policyd requester spoof denied ok"
  "SELFTEST: policy malformed ok"
  "SELFTEST: ipc routing execd ok"
  "child: hello-elf"
  "execd: elf load ok"
  "SELFTEST: e2e exec-elf ok"
  "child: exit0 start"
  "execd: child exited"
  "SELFTEST: child exit ok"
  "SELFTEST: exec denied ok"
  "SELFTEST: execd malformed ok"
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
  "dsoftbusd: auth ok"
  "dsoftbusd: os session ok"
  "SELFTEST: dsoftbus os connect ok"
  "SELFTEST: dsoftbus ping ok"
  "SELFTEST: end"
)

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
for marker in "${expected_sequence[@]}"; do
  if ! grep -aFq "$marker" "$UART_LOG"; then
    missing=1
    missing_marker="$marker"
    break
  fi
done
if [[ "$missing" -ne 0 ]]; then
  echo "[error] Missing UART marker: $missing_marker" >&2
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
    echo "Marker out of order: $marker (line $line)" >&2
    exit 1
  fi
  prev=$line
done

for policy_marker in "SELFTEST: policy allow ok" "SELFTEST: policy deny ok" "SELFTEST: policy malformed ok" "SELFTEST: bundlemgrd route execd denied ok"; do
  if ! grep -aFq "$policy_marker" "$UART_LOG"; then
    echo "Missing UART marker: $policy_marker" >&2
    exit 1
  fi
done

trim_log "$QEMU_LOG" "$QEMU_LOG_MAX"
trim_log "$UART_LOG" "$UART_LOG_MAX"

if [[ "$qemu_status" -ne 0 && "$RUN_UNTIL_MARKER" != "1" ]]; then
  echo "[warn] QEMU exited with status $qemu_status" >&2
fi
echo "QEMU selftest completed (markers verified)." >&2
