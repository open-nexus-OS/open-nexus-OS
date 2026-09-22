#!/usr/bin/env bash
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: RFC-0098 C1/C3 gate (TASK-0245 P4). The device tree is the one hardware
# truth: no kernel, loader, init, driver or service source may carry a QEMU-virt
# platform address, interrupt-line arithmetic or timebase constant. The literals
# below are exactly the ones TASK-0245 deleted; a hit is a regression, named by
# file:line. Allowed: the FDT goldens and their tests, the board's own tree, docs,
# task ledgers, and comment lines (a comment may explain history). Every run first
# proves the scanner on fixtures (`--self-test` runs only that).
set -euo pipefail
cd "$(dirname "$0")/.."

SCAN=(source userspace tools/nx/src)
# Each pattern is an extended regex over ONE source line with comments stripped.
PATTERNS=(
  '0x1000_0000\b'      # QEMU virt UART
  '0x0c00_0000\b'      # QEMU virt PLIC
  '0x0200_0000\b'      # QEMU virt CLINT
  '0x1000_1000\b'      # QEMU virt virtio-mmio window
  '0x1000_8000\b'      # QEMU virt virtio-mmio slot 7 (the GPU)
  '0x0010_1000\b'      # QEMU virt goldfish RTC
  '0x1010_0000\b'      # QEMU virt fw_cfg
  'TICKS_PER_US\s*(:\s*\w+\s*)?=\s*[0-9]'   # the 10 MHz timebase as a constant
  'irq\s*[:=]\s*[0-9]+\s*\+\s*\w+'  # PLIC line derived from a slot index
)
EXCLUDE_RE='(/tests/|/goldens/|/target/|/build/|\.md$|\.dts$|\.dtb$)'

scan() {
  local root="$1" hits=0
  while IFS= read -r file; do
    [[ "$file" =~ $EXCLUDE_RE ]] && continue
    local n=0
    while IFS= read -r line; do
      n=$((n + 1))
      # Strip a trailing `// …` comment and skip doc/plain comment lines.
      local code="${line%%//*}"
      [[ -z "${code//[[:space:]]/}" ]] && continue
      for pat in "${PATTERNS[@]}"; do
        if [[ "$code" =~ $pat ]]; then
          echo "$file:$n: $line"
          hits=$((hits + 1))
          break
        fi
      done
    done < "$file"
  done < <(find "$root" -name '*.rs' -type f 2>/dev/null)
  return $hits
}

self_test() {
  local tmp; tmp=$(mktemp -d)
  mkdir -p "$tmp/src" "$tmp/tests"
  cat > "$tmp/src/a.rs" <<'RS'
const UART: usize = 0x1000_0000; // must hit
// const OLD: usize = 0x1000_0000;   (comment — must not hit)
let irq = 3 + idx as u32;         // must hit
const TICKS_PER_US: u64 = 10;      // must hit
RS
  cat > "$tmp/tests/b.rs" <<'RS'
const UART: usize = 0x1000_0000; // tests are excluded
RS
  local out rc=0
  out=$(scan "$tmp") || rc=$?
  rm -rf "$tmp"
  if [[ "$rc" -ne 3 ]]; then
    echo "[FAIL] platform-literal scanner self-test: expected 3 hits, got $rc" >&2
    echo "$out" >&2
    exit 1
  fi
  echo "[ok]   platform-literal scanner self-test (3 hits on fixtures, comments/tests skipped)"
}

self_test
if [[ "${1:-}" == "--self-test" ]]; then exit 0; fi

total=0
for root in "${SCAN[@]}"; do
  rc=0
  scan "$root" || rc=$?
  total=$((total + rc))
done
if [[ "$total" -ne 0 ]]; then
  echo "[FAIL] $total platform literal(s) outside the device tree (RFC-0098 C1: the FDT is the one hardware truth)" >&2
  exit 1
fi
echo "[PASS] platform literals: none outside the device tree (${SCAN[*]})"
