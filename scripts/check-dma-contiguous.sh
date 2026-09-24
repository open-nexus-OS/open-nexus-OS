#!/usr/bin/env bash
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: RFC-0098 C4 gate (TASK-0286 P4a). A physically contiguous VMO
# (`vmo_create_contiguous`) is the kind for memory a DEVICE addresses by one
# base — virtio queues, command and response pools, request buffers. Only code
# that programs a device creates one: drivers (`source/drivers`), the virtio-net
# driver crate (`userspace/nexus-net-os`), the proof harness that drives devices
# (`source/apps/selftest-client`) and the ABI that defines it. A service or an
# app never does: windowd's framebuffer and app-host's surfaces were contiguous
# (a 64 MiB block for 49 MiB) only because `cap_query` could name one base, and
# the device reads the framebuffer through its runs (`vmo_runs`). Comment lines
# are allowed (history may be explained). Every run first proves the scanner on
# fixtures (`--self-test` runs only that).
set -euo pipefail
cd "$(dirname "$0")/.."

SCAN=(source userspace)
TOKEN='vmo_create_contiguous'
ALLOW_RE='^(source/drivers/|userspace/nexus-net-os/|source/apps/selftest-client/|source/libs/nexus-abi/)'
EXCLUDE_RE='(/target/|/build/)'

scan() {
  local root="$1" prefix="$2" hits=0
  while IFS= read -r file; do
    local rel="${file#"$prefix"}"
    [[ "$rel" =~ $EXCLUDE_RE ]] && continue
    [[ "$rel" =~ $ALLOW_RE ]] && continue
    local n=0
    while IFS= read -r line; do
      n=$((n + 1))
      local code="${line%%//*}"
      if [[ "$code" == *"$TOKEN"* ]]; then
        echo "$rel:$n: $line"
        hits=$((hits + 1))
      fi
    done < "$file"
  done < <(find "$root" -type f -name '*.rs' | sort)
  return $hits
}

self_test() {
  local dir
  dir="$(mktemp -d)"
  mkdir -p "$dir/source/services/windowd" "$dir/source/drivers/gpud" "$dir/userspace/app"
  cat > "$dir/source/services/windowd/fb.rs" <<'EOF'
let fb = vmo_create_contiguous(len)?;
// windowd's framebuffer used to be vmo_create_contiguous — a comment may say so
use nexus_abi::vmo_create_contiguous;
EOF
  cat > "$dir/source/drivers/gpud/q.rs" <<'EOF'
let q = nexus_abi::vmo_create_contiguous(4096)?;
EOF
  cat > "$dir/userspace/app/surface.rs" <<'EOF'
let s = nexus_abi::vmo_create_contiguous(4096)?;
EOF
  local hits=0
  set +e
  scan "$dir/source" "$dir/" > /dev/null
  hits=$?
  scan "$dir/userspace" "$dir/" > /dev/null
  hits=$((hits + $?))
  set -e
  rm -rf "$dir"
  if [[ "$hits" -ne 3 ]]; then
    echo "[FAIL] dma-contiguous scanner self-test: expected 3 hits on fixtures, got $hits" >&2
    exit 2
  fi
  echo "[ok]   dma-contiguous scanner self-test (3 hits on fixtures; drivers and comments skipped)"
}

self_test
[[ "${1:-}" == "--self-test" ]] && exit 0

total=0
for root in "${SCAN[@]}"; do
  set +e
  scan "$root" ""
  hits=$?
  set -e
  total=$((total + hits))
done
if [[ "$total" -ne 0 ]]; then
  echo "[FAIL] $total contiguous VMO(s) outside the device-driving code (RFC-0098 C4: a device reads everything else through vmo_runs)" >&2
  exit 1
fi
echo "[PASS] dma-contiguous: only drivers, the net driver crate, the proof harness and the ABI create contiguous VMOs"
