#!/usr/bin/env bash
# Copyright 2026 Open Nexus OS Contributors
# SPDX-License-Identifier: Apache-2.0
#
# CONTEXT: CI contract test — windowd image-size budget (TASK-0076B follow-up).
# Service images allocate from the kernel VMO pool at spawn; exhaustion is a
# silent service death. This gate makes windowd growth VISIBLE long before the
# pool wall: it fails with values (have/budget) instead of a dead boot.
#
# The budget is a deliberate contract, not a guess: raise it CONSCIOUSLY
# (with a note in tasks/TASK-0076B ledger) when windowd legitimately grows.
# Pool math (2026-07-06): pool=160MB; windowd is by far the largest image
# (~6.6MB incl. 4MB wallpaper rodata + 2MB heap BSS). Budget 8MB = current
# size + ~20% headroom.
set -euo pipefail

BUDGET_BYTES=$((8 * 1024 * 1024))
ELF="target/riscv64imac-unknown-none-elf/release/windowd"

if [[ ! -f "$ELF" ]]; then
    echo "contract-windowd-size: building windowd (riscv os-lite release)…"
    cargo build -q -p windowd --release \
        --target riscv64imac-unknown-none-elf --features os-lite --no-default-features
fi

# dec = text + data + bss (what the spawn-time pool allocation must back).
DEC=$(size "$ELF" | awk 'NR==2 {print $4}')

if [[ -z "$DEC" ]]; then
    echo "contract-windowd-size: FAIL — could not read size of $ELF" >&2
    exit 2
fi

if (( DEC > BUDGET_BYTES )); then
    echo "contract-windowd-size: FAIL — windowd image ${DEC} bytes exceeds the ${BUDGET_BYTES}-byte budget" >&2
    echo "  (the image allocates from the kernel VMO pool at spawn; growth must be conscious —" >&2
    echo "   trim the image or raise the budget WITH a ledger note in tasks/TASK-0076B)" >&2
    exit 1
fi

echo "contract-windowd-size: OK — windowd image ${DEC} / ${BUDGET_BYTES} bytes ($(( DEC * 100 / BUDGET_BYTES ))% of budget)"
