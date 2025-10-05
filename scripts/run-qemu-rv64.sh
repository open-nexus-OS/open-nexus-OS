#!/usr/bin/env bash
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.
#
# Environment knobs:
#   RUN_TIMEOUT      – GNU timeout duration (default: 30s).
#   RUN_UNTIL_MARKER – When "1", stop once a success marker hits the UART log.
#   QEMU_LOG_MAX     – Maximum size for qemu.log after trimming (default: 50 MiB).
#   UART_LOG_MAX     – Maximum size for uart.log after trimming (default: 10 MiB).
#   QEMU_BINARY      – Override QEMU binary (default: qemu-system-riscv64).
#   NEURON_ELF       – Path to the neuron-boot ELF (default release target path).
#   UART_LOG         – Override UART log path (default: uart.log in repo root).
#   QEMU_LOG         – Override QEMU diagnostic log path (default: qemu.log).
#
# Continuous QEMU diagnostics can generate tens of gigabytes of logs.
# We trim the artifacts post-run to keep CI storage and developer machines
# sane without sacrificing insight into the most recent execution.

set -euo pipefail

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(cd -- "${SCRIPT_DIR}/.." && pwd)
cd "$REPO_ROOT"

RUN_TIMEOUT=${RUN_TIMEOUT:-30s}
RUN_UNTIL_MARKER=${RUN_UNTIL_MARKER:-0}
QEMU_LOG_MAX=${QEMU_LOG_MAX:-52428800}
UART_LOG_MAX=${UART_LOG_MAX:-10485760}
QEMU_BINARY=${QEMU_BINARY:-qemu-system-riscv64}
NEURON_ELF=${NEURON_ELF:-target/riscv64imac-unknown-none-elf/release/neuron-boot}
UART_LOG=${UART_LOG:-uart.log}
QEMU_LOG=${QEMU_LOG:-qemu.log}
QEMU_MACHINE=${QEMU_MACHINE:-virt}
QEMU_MEMORY=${QEMU_MEMORY:-512M}
QEMU_SMP=${QEMU_SMP:-1}
QEMU_BIOS=${QEMU_BIOS:-none}
QEMU_CPU=${QEMU_CPU:-rv64}

if [[ ! -f "$NEURON_ELF" ]]; then
  echo "[error] Missing NEURON boot ELF at: $NEURON_ELF" >&2
  echo "        Build it with 'cargo build -p neuron-boot --release --target riscv64imac-unknown-none-elf'." >&2
  exit 1
fi

trim_log() {
  local file="$1" max="$2"
  if [[ -f "$file" ]]; then
    local sz
    sz=$(wc -c <"$file" 2>/dev/null || echo 0)
    if [[ "$sz" -gt "$max" ]]; then
      echo "[info] Trimming $file from ${sz} bytes to last $max bytes"
      tail -c "$max" "$file" >"${file}.tmp" && mv "${file}.tmp" "$file"
    fi
  fi
}

MARKER_FLAG_FILE=""
cleanup() {
  if [[ -n "$MARKER_FLAG_FILE" && -f "$MARKER_FLAG_FILE" ]]; then
    rm -f "$MARKER_FLAG_FILE"
  fi
}
trap cleanup EXIT

if [[ "$RUN_UNTIL_MARKER" == "1" ]]; then
  MARKER_FLAG_FILE=$(mktemp)
fi

detect_markers() {
  local line
  while IFS= read -r line; do
    if [[ "$line" == *"SELFTEST: end"* || "$line" == *"samgrd: ready"* || "$line" == *"bundlemgrd: ready"* ]]; then
      echo "[info] Marker detected: $line" >&2
      if [[ -n "$MARKER_FLAG_FILE" ]]; then
        printf 'hit' >"$MARKER_FLAG_FILE"
      fi
      pkill -f "$QEMU_BINARY" >/dev/null 2>&1 || true
      break
    fi
  done
}

rm -f "$UART_LOG" "$QEMU_LOG"

cmd=(
  "$QEMU_BINARY"
  -machine "$QEMU_MACHINE"
  -cpu "$QEMU_CPU"
  -smp "$QEMU_SMP"
  -m "$QEMU_MEMORY"
  -nographic
  -bios "$QEMU_BIOS"
  -kernel "$NEURON_ELF"
  -serial stdio
  -d int,mmu,unimp
  -D "$QEMU_LOG"
)

if [[ $# -gt 0 ]]; then
  cmd+=("$@")
fi

echo "[info] Launching QEMU with timeout ${RUN_TIMEOUT}: ${cmd[*]}"

set +e
if [[ "$RUN_UNTIL_MARKER" == "1" ]]; then
  timeout --preserve-status "$RUN_TIMEOUT" "${cmd[@]}" | tee "$UART_LOG" >(detect_markers)
else
  timeout --preserve-status "$RUN_TIMEOUT" "${cmd[@]}" | tee "$UART_LOG"
fi
pipe_status=("${PIPESTATUS[@]}")
set -e

qemu_status=${pipe_status[0]:-0}

if [[ "$RUN_UNTIL_MARKER" == "1" && -n "$MARKER_FLAG_FILE" && -s "$MARKER_FLAG_FILE" ]]; then
  echo "[info] UART success marker observed; treating run as passed." >&2
  qemu_status=0
fi

if [[ "$qemu_status" -eq 124 ]]; then
  echo "[error] QEMU execution timed out after ${RUN_TIMEOUT}." >&2
fi

trim_log "$QEMU_LOG" "$QEMU_LOG_MAX"
trim_log "$UART_LOG" "$UART_LOG_MAX"

exit "$qemu_status"
