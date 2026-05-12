# Current State — Open Nexus OS

Last updated: 2026-05-11 (TASK-0056C Done)

## What changed

TASK-0056C (UI v2a present/input perf latency coalescing) is Done. All 22 host tests pass, dep-gate and selftest-arch gates pass, clippy is clean.

The task landed deterministic pointer-motion coalescing, explicit no-damage / no-visible-change skip rules, and idle-cheap / wakeup-collapse / stable counter expectations in `windowd`. The embedded runtime/reactor floor across `inputd -> windowd -> fbdevd` is now contract-backed by RFC-0055 (Complete).

## Key decisions

- Coalescing only applies to pointer-motion bursts (bounded batch + latest-wins). Click, focus transfer, wheel, and keyboard edges are never coalesced.
- No-damage skip (frame-level hash match) can skip up to 3 consecutive frames, then forced present on 4th — proven by `test_no_visible_change_skip_unbounded_accumulation_prevented`.
- No-visible-state-change skip (semantic state) allowed after at least 1 frame shown, bounded counter — proven by `test_no_visible_state_change_skip`.
- All skip decisions check both damage and visible-state before skipping; if either is true, present proceeds.
- Authority boundaries unchanged: `inputd` normalizes, `windowd` decides, `fbdevd` cadence/scanout.

## Proof state

- 22/22 `tests/ui_v2c_host` tests pass (host-first, zero warnings)
- `just dep-gate` passes
- `scripts/check-selftest-arch.sh` passes
- `clippy` clean for `windowd` + `ui_v2c_host`

## Known risks / DON'T DO

- DON'T coalesce click, focus-transfer, wheel, or keyboard edges — these must stay individually observable.
- DON'T skip compose/present when there IS damage or visible-state change.
- DON'T emit `ok`/`ready`/`latency ok` markers before real visible update or proven no-damage/no-visible-change.
- DON'T reintroduce unbounded "drain/yield" loops in inputd/windowd/fbdevd.
- DON'T add a separate runtime/platform subsystem beside inputd/windowd/fbdevd.
- DON'T reopen TASK-0253 or back-claim perf closure into it.

## Open threads

- QEMU marker ladder (56C perf markers) and `just diag-os` RISC-V build check pending.
- Perf counter vocabulary is provisional; may be hardened in follow-up tasks.
- Downstream tasks TASK-0059 (scroll/effects), TASK-0062 (animation/runtime), TASK-0063 (virtualized list), TASK-0064 (window management) will extend this floor.
