#!/usr/bin/env bash
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: retired names stay retired (TASK-0246 P4a, ADR-0067). A service that was
# renamed must not come back through a stale copy, a script, a policy row, a marker or a
# living document. The table lists each retired name — every spelling it was written in,
# matched exactly and inside longer identifiers (`VIRTIOBLKD_SLOT`) — with its successor and
# the decision that retired it. Exact spellings, not a case-insensitive match: the virtio
# device type `VirtioBlkDevice` contains the old name case-folded and is not the service.
# Every file of the tree is scanned — tracked or new; git-ignored output (build/, target/)
# is not the tree — except the dated records that must keep the old name to stay true
# (task ledgers, the changelog, ADRs, RFCs, board measurements) and this table. Every run
# first proves the scanner on a fixture tree (`--self-test` runs only that).
set -euo pipefail
cd "$(dirname "$0")/.."

# spellings (space-separated) ; successor ; the decision that retired it
RETIRED=(
  'virtioblkd Virtioblkd VirtioBlkd VIRTIOBLKD;blkd;ADR-0067, TASK-0246 P4a: one block owner on every platform'
  'boot_display_mode SYSCALL_BOOT_DISPLAY_MODE resolve_boot_display_mode;gpud (the framebuffer grant, OP_FRAMEBUFFER_REQUEST);RFC-0098 C7, TASK-0251 P1: the display mode is gpud'"'"'s'
)
# Dated records describe the tree as it was, so they keep the names it had.
HISTORY=(tasks/ CHANGELOG.md docs/adr/ docs/rfcs/ docs/board/measurements/ scripts/check-retired-names.sh)

# scan <repo>: one line per hit, `path:line:text  (retired: old -> new; why)`.
scan() {
  local repo="$1" entry spellings successor why hits sp
  local excludes=() patterns
  for h in "${HISTORY[@]}"; do excludes+=(":(exclude)$h"); done
  for entry in "${RETIRED[@]}"; do
    IFS=';' read -r spellings successor why <<< "$entry"
    patterns=()
    for sp in $spellings; do patterns+=(-e "$sp"); done
    hits="$(git -C "$repo" grep --untracked -n -I -F "${patterns[@]}" -- . "${excludes[@]}" || true)"
    [[ -z "$hits" ]] && continue
    while IFS= read -r hit; do
      echo "$hit  (retired: ${spellings%% *} -> $successor; $why)"
    done <<< "$hits"
  done
}

self_test() {
  local dir hits
  dir="$(mktemp -d)"
  git -C "$dir" init -q
  mkdir -p "$dir/source/services/blkd/src" "$dir/policies" "$dir/docs/architecture" \
    "$dir/docs/adr" "$dir/tasks" "$dir/build"
  echo 'use VirtioBlkd as _;' > "$dir/source/services/blkd/src/main.rs"     # hit (CamelCase)
  echo 'pub const VIRTIOBLKD_WATCHDOG: u32 = 1;' > "$dir/source/services/blkd/src/slots.rs"  # hit
  echo 'let dev = VirtioBlkDevice::new(slot);' > "$dir/source/services/blkd/src/dev.rs"     # the device type
  echo 'virtioblkd = ["device.mmio.blk"]' > "$dir/policies/base.toml"       # hit
  echo 'blkd = ["device.mmio.blk"]' > "$dir/policies/ok.toml"               # successor
  echo 'The owner is `virtioblkd`.' > "$dir/docs/architecture/storage.md"   # hit (living doc)
  echo 'ADR-0044 made virtioblkd the owner.' > "$dir/docs/adr/0044.md"      # history
  echo 'P4a renames virtioblkd.' > "$dir/tasks/TASK-0246.md"                # history
  echo '- virtioblkd became blkd' > "$dir/CHANGELOG.md"                     # history
  echo 'build/' > "$dir/.gitignore"
  echo 'virtioblkd: gpt ok' > "$dir/build/uart.log"                         # ignored output
  git -C "$dir" add source policies docs tasks CHANGELOG.md .gitignore
  echo 'virtioblkd' > "$dir/new-untracked.sh"                               # hit (not yet added)
  hits="$(scan "$dir" | wc -l)"
  rm -rf "$dir"
  if [[ "$hits" -ne 5 ]]; then
    echo "[FAIL] retired-names scanner self-test: expected 5 hits on fixtures, got $hits" >&2
    exit 2
  fi
  echo "[ok]   retired-names scanner self-test (5 hits: every spelling, inside identifiers, tracked or new; the device type, history and ignored output skipped)"
}

self_test
[[ "${1:-}" == "--self-test" ]] && exit 0

found="$(scan .)"
if [[ -n "$found" ]]; then
  echo "$found" >&2
  echo "[FAIL] $(wc -l <<< "$found") line(s) name a retired name outside the dated records" >&2
  exit 1
fi
echo "[PASS] retired names: ${#RETIRED[@]} retired name(s), none in the living tree (history kept in ${HISTORY[*]:0:5})"
