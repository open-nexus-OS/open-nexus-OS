---
title: TASK-0037 OTA A/B v2b: real boot slot decided by the boot chain (delivered by TASK-0289 nxboot + measured handoff)
status: Done (2026-09-09 — reconciled: goal delivered by TASK-0289 nxboot + measured boot handoff)
owner: @runtime
created: 2025-12-22
depends-on:
  - TASK-0036
follow-up-tasks: []
links:
  - Vision: docs/architecture/vision.md
  - Depends-on: tasks/TASK-0036-ota-ab-v2-userspace-healthmux-rollback-softreboot.md
---

## Closure 2026-09-09 (reconciliation — Done, delivered by TASK-0289)

**Goal of this ledger:** the booted A/B slot is decided by the boot chain at boot time, read
during early init, and a scheduled rollback affects the *next real boot* — not a simulation.

**Actual solution (code ground truth, verified 2026-09-09):** delivered by TASK-0289 (Done
2026-08-31, Phases A+B gated in `just test-all`) with a better mechanism than bootargs — a
measured, CRC-protected handoff record instead of a free-text command line:

- First-stage loader `nxboot/` selects the slot from the projected BSB (`nxboot: bsb ok
  (slot=a seq=1)` → `verify ok (… rbidx=n)` → `jump slot=a`), with tries/fallback lanes
  (`nxboot: tries 2->1`, `fallback (slot=b exhausted) -> slot=a`).
- Kernel consumes the record: `source/kernel/neuron/src/core/boot_handoff.rs` (`BootHandoff`,
  `KSELFTEST: boot handoff ok (measured)`, honest `invalid (crc)` / `absent (direct kernel)`).
- Userspace slot authority: `source/services/bootctld` (`bootctld: commit ok (slot=…)`,
  `rollback observed`), `updated` as client; proven end to end by the OTA lanes
  (`SELFTEST: ota fallback ok`, ci-os-ota-flip / -fallback / -backstops).
- Contracts: ADR-0058 (BSB write matrix), ADR-0059 (boot chain + measured handoff), RFC-0089.

**Not delivered / not needed:** an OpenSBI bootargs channel — rejected in favour of the
measured handoff page (tamper-evident, no string parsing in early boot).

## Context

OTA A/B v2 wants an unambiguous “booted slot” determined at boot time. The prompt proposes
bootargs via OpenSBI/SBI handoff. With **kernel unchanged** and without an owned boot chain path,
this cannot be proven today.

This task exists to prevent drift: it documents the real boot integration work as a separate,
explicitly blocked deliverable.

## Goal

Once unblocked, prove:

- the selected slot is passed via bootargs at boot time (A/B),
- the OS reads it during early init and uses it to mount/select the correct system set,
- rollback scheduling actually affects the *next* real boot, not just a soft simulation.

## Red flags / decision points

- **RED**: blocked until boot chain integration exists (bootloader/OpenSBI/firmware handoff path).
