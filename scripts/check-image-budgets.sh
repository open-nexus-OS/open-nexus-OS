#!/usr/bin/env bash
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: CI contract test — per-service IMAGE BUDGETS (TASK-0076B, widened
# from the windowd-only check in TASK-0305).
#
# Every service image is allocated out of the kernel's user VMO arena at spawn
# (`USER_VMO_ARENA_LEN`, source/kernel/neuron/src/mm/mod.rs). Exhausting that
# arena is not a clean error — it is a silent service death, days of debugging
# later. This gate makes image growth VISIBLE with numbers, long before the
# arena wall.
#
# WHY A TABLE AND NOT ONE NUMBER: a budget on a single service is not a
# contract, it is a tripwire someone happened to install. The arena is shared,
# so the budget belongs to every image that draws from it. `OTHER_BUDGET` is
# the catch-all so a service nobody thought to list still cannot quietly
# become the next 10 MB.
#
# RAISING A BUDGET IS A DECISION, NOT A CHORE. Do it consciously, with a note
# in the tasks/TASK-0076B ledger saying what grew and why. The gate prints
# usage percentages on every run precisely so the trend is readable before a
# number has to move.
set -euo pipefail

TARGET_DIR="target/riscv64imac-unknown-none-elf/release"
KERNEL_MM="source/kernel/neuron/src/mm/mod.rs"

# ── Budgets (bytes). Measured size + headroom, rounded to a clean number. ────
# Arena is 224 MB. These five images are ~64 MB of it at ceiling; the rest of
# the arena carries the RUNTIME VMOs (surfaces, wallpaper planes, the shared
# glyph atlas, per-app-host heaps), which is the larger and more dynamic half.
#
# NOTE ON NESTING: init-lite EMBEDS execd which embeds app-host, so those three
# numbers overlap — they are not additive against the arena. Each is still
# budgeted on its own because each is separately spawned and separately capable
# of running away.
#
#   service      budget      measured 2026-07-26   note
declare -A BUDGETS=(
    [init-lite]=$((24 * 1024 * 1024))   # 16.5 MB — carries the embed chain
    [app-host]=$((14 * 1024 * 1024))    # 10.0 MB — DSL runtime + widget kit
    [windowd]=$((14 * 1024 * 1024))     #  9.1 MB — see the atlas note below
    [execd]=$((10 * 1024 * 1024))       #  6.4 MB — embeds app-host
    [gpud]=$((2 * 1024 * 1024))         #  0.9 MB
)
# Any service image not named above. Deliberately tight: every service that
# is not one of the five big ones sits at 0.4-0.9 MB today, so 2 MB is a real
# tripwire rather than a formality.
OTHER_BUDGET=$((2 * 1024 * 1024))

# Not arena-allocated service images — excluded WITH a reason, never silently.
declare -A NOT_A_SERVICE=(
    [neuron-boot]="the kernel image itself — loaded by the bootloader, not spawned from the arena"
    [recv-wake-probe]="a kernel IPC probe binary, not a service"
)

# windowd note (TASK-0305): ~4.4 MB of its image is the shared glyph atlas,
# linked in via `nexus-text-baked`'s `embedded-atlas` feature — HALF the image.
# RFC-0080 already built the mechanism to map that atlas as one shared RO VMO
# instead (an app-host does exactly this). windowd never became a consumer of
# it. Doing so takes 4.4 MB straight back out and is the real fix; the budget
# here is headroom, not a licence.

fail=0
printf '%-14s %10s %10s %6s\n' "image" "size" "budget" "used"
printf '%-14s %10s %10s %6s\n' "--------------" "----------" "----------" "------"

check_one() {
    local name=$1 elf=$2 budget=$3
    # dec = text + data + bss — what the spawn-time arena allocation backs.
    local dec
    dec=$(size "$elf" | awk 'NR==2 {print $4}')
    if [[ -z "$dec" ]]; then
        echo "check-image-budgets: FAIL — could not read size of $elf" >&2
        fail=1
        return
    fi
    local pct=$((dec * 100 / budget))
    printf '%-14s %10d %10d %5d%%%s\n' "$name" "$dec" "$budget" "$pct" \
        "$( ((dec > budget)) && echo '  OVER' || true)"
    if ((dec > budget)); then
        fail=1
    fi
}

if [[ ! -d "$TARGET_DIR" ]]; then
    echo "check-image-budgets: FAIL — $TARGET_DIR missing (build the OS first)" >&2
    exit 2
fi

shopt -s nullglob
for elf in "$TARGET_DIR"/*; do
    [[ -f "$elf" && -x "$elf" ]] || continue
    name=$(basename "$elf")
    # Skip build artefacts that are not spawned service images.
    [[ "$name" == *.* ]] && continue
    [[ -n "${NOT_A_SERVICE[$name]:-}" ]] && continue
    budget=${BUDGETS[$name]:-$OTHER_BUDGET}
    check_one "$name" "$elf" "$budget"
done

for name in "${!NOT_A_SERVICE[@]}"; do
    printf '%-14s %10s %10s %6s  (%s)\n' "$name" "-" "-" "n/a" "${NOT_A_SERVICE[$name]}"
done

arena=$(grep -oP 'USER_VMO_ARENA_LEN: usize = \K[0-9]+ \* 1024 \* 1024' "$KERNEL_MM" 2>/dev/null | head -1 || true)
[[ -n "$arena" ]] && echo "  (kernel user VMO arena: ${arena% \* 1024 \* 1024} MB — $KERNEL_MM)"

if ((fail)); then
    cat >&2 <<'EOF'

check-image-budgets: FAIL — an image is over budget.
  Service images allocate from the kernel VMO arena at spawn; exhaustion is a
  SILENT service death, not an error you get to read. Either trim the image or
  raise its budget CONSCIOUSLY, with a note in tasks/TASK-0076B saying what
  grew and why.
EOF
    exit 1
fi

echo "check-image-budgets: OK"
