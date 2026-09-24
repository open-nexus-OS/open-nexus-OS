#!/usr/bin/env bash
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: RFC-0098 C4 gate (TASK-0286 P3b). Physical memory has ONE owner —
# the frame pool over the tree's banks (`mm/frame_pool.rs`) — and the kernel
# reaches it only through `crate::phys`. No kernel source may name a fixed
# physical window, one of the retired arena/pool types, or a RAM literal of
# the QEMU virt bank (`0x8xxx_xxxx`): the windows TASK-0286 deleted stay
# deleted. Allowed: comment lines (history may be explained), `#[cfg(test)]`
# modules and `tests.rs` files (fixtures name addresses), and the user VA
# limit of RFC-0085 (`USER_VADDR_LIMIT`, a virtual address). Every run first
# proves the scanner on fixtures (`--self-test` runs only that).
set -euo pipefail
cd "$(dirname "$0")/.."

SCAN=(source/kernel)
PATTERNS=(
  'USER_VMO_ARENA'                 # the fixed VMO arena (P3a)
  'KERNEL_PAGE_POOL'               # the init loader's page pool (P3b)
  '\bVmoPool\b|\bVMO_POOL\b'       # the arena's allocator (P3a)
  'STACK_POOL_(BASE|LIMIT)'        # the user stack window (P3b)
  'BOOTSTRAP_IDENTITY_WINDOW'      # the bootstrap identity VMO (P3b)
  '0x8[0-9a-f]{3}_?[0-9a-f]{4}\b'  # a RAM address of the virt bank as a constant
)
ALLOW_LINE_RE='USER_VADDR_LIMIT'
EXCLUDE_RE='(/tests/|tests\.rs$|/target/|/build/|\.md$)'

scan() {
  local root="$1" hits=0
  while IFS= read -r file; do
    [[ "$file" =~ $EXCLUDE_RE ]] && continue
    local n=0 in_tests=0
    while IFS= read -r line; do
      n=$((n + 1))
      # An inline test module ends the scan of this file (fixtures follow).
      [[ "$line" =~ ^#\[cfg\(test\)\] || "$line" =~ ^mod\ tests ]] && in_tests=1
      [[ $in_tests -eq 1 ]] && continue
      local code="${line%%//*}"
      [[ -z "${code//[[:space:]]/}" ]] && continue
      [[ "$code" =~ $ALLOW_LINE_RE ]] && continue
      for pat in "${PATTERNS[@]}"; do
        if [[ "$code" =~ $pat ]]; then
          echo "$file:$n: $line"
          hits=$((hits + 1))
          break
        fi
      done
    done < "$file"
  done < <(find "$root" -type f -name '*.rs' | sort)
  return $hits
}

self_test() {
  local dir
  dir="$(mktemp -d)"
  mkdir -p "$dir/kernel"
  cat > "$dir/kernel/fixture.rs" <<'EOF'
pub const USER_VMO_ARENA_BASE: usize = 0x8380_0000;
static POOL: Mutex<VmoPool> = Mutex::new(VmoPool::new());
const POOL_LIMIT: usize = KERNEL_PAGE_POOL_BASE + 1;
pub(super) const USER_VADDR_LIMIT: usize = 0x8000_0000;
// the old arena lived at 0x8380_0000 — a comment may say so
let cursor = 0x80100000;
#[cfg(test)]
mod tests {
    const BANK: u64 = 0x8000_0000;
}
EOF
  local hits=0
  set +e
  scan "$dir/kernel" > /dev/null
  hits=$?
  set -e
  rm -rf "$dir"
  if [[ "$hits" -ne 4 ]]; then
    echo "[FAIL] fixed-window scanner self-test: expected 4 hits on fixtures, got $hits" >&2
    exit 2
  fi
  echo "[ok]   fixed-window scanner self-test (4 hits on fixtures; comments, tests and the VA limit skipped)"
}

self_test
[[ "${1:-}" == "--self-test" ]] && exit 0

total=0
for root in "${SCAN[@]}"; do
  set +e
  scan "$root"
  hits=$?
  set -e
  total=$((total + hits))
done
if [[ "$total" -ne 0 ]]; then
  echo "[FAIL] $total fixed physical window(s) or RAM literal(s) in the kernel (RFC-0098 C4: the frame pool is the one owner)" >&2
  exit 1
fi
echo "[PASS] fixed windows: none left in the kernel (${SCAN[*]})"
