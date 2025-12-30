---
title: TASK-0036 OTA A/B v2 (userspace): slot state machine + health multiplexer + rollback timer (soft-reboot proof)
status: Draft
owner: @runtime
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - Packaging/updates baseline: tasks/TASK-0007-updates-packaging-v1_1-userspace-ab-skeleton.md
  - Supply-chain baseline: tasks/TASK-0029-supply-chain-v1-sbom-repro-sign-policy.md
  - Persistence substrate: tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Audit/observability (optional): tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We want robust A/B OTA behavior:

- stage into inactive slot,
- schedule a trial boot,
- confirm health via a health multiplexer,
- auto-rollback on timeout/degradation.

Repo reality today:

- There is no `updated` service in-tree yet; OTA orchestration is still a plan.
- “Boot slot via SBI/bootargs” cannot be *truly* proven without boot chain/kernel/firmware integration.

This task focuses on the **userspace state machine** and provides **honest proof** via a soft-reboot simulation.

## Goal

Deliver a userspace A/B OTA v2 state machine with:

- durable slot state under `/state/boot/slot.json`,
- atomic stage/commit semantics (inactive slot),
- a health multiplexer (quorum + timeouts),
- rollback timer (boots-left and/or deadline),
- deterministic host tests and OS selftest markers using a **soft reboot simulation**.

## Non-Goals

- Real OpenSBI bootargs wiring (separate blocked task).
- Real `.nxs` system set staging (blocked until tooling and `updated` exist).
- Kernel changes.

## Constraints / invariants (hard requirements)

- Kernel untouched.
- No fake success: “booted slot B” is only claimed after the simulated new init cycle uses slot B configuration.
- Deterministic tests: injectable clock, bounded timeouts, stable markers.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **RED**:
  - Without a boot chain, “bootargs” cannot be validated. Proof is limited to **soft-reboot** simulation.
- **YELLOW**:
  - Health signal sources: in v2 we want `execd/metricsd/logd/statefs`, but some are planned not implemented.
    v2 must allow optional sources and keep a minimal quorum that works today.

## Contract sources (single source of truth)

- Supply-chain verification expectations: TASK-0029
- Persistence: TASK-0009
- QEMU marker contract: `scripts/qemu-test.sh`

## Stop conditions (Definition of Done)

### Proof (Host) — required

Add deterministic tests (`tests/ota_ab_v2_host/`):

- stage+commit schedules trial slot B with boots-left and deadline
- no health confirmation before deadline → rollback scheduled to last_good
- health confirmation within grace → promote current to B and clear trial
- degradation path: repeated “critical restart” events triggers unhealthy decision

### Proof (OS / QEMU) — gated on statefs + minimal services

Once statefs exists, selftest proves:

- `SELFTEST: ota stage ok`
- `SELFTEST: ota commit scheduled ok`
- `SELFTEST: ota simulated boot ok (slot=B)`
- `SELFTEST: ota healthy confirm ok`
- `SELFTEST: ota rollback scheduled ok`

## Touched paths (allowlist)

- `source/services/`:
  - `bootargd` (userspace slot selector service; writes “next slot” state)
  - `healthd` (health multiplexer; minimal sources first)
  - `updated` (if/when it exists; otherwise a minimal OTA orchestrator service can be introduced)
- `userspace/ota/` (`slotstate`, `healthmux` libs)
- `source/apps/selftest-client/`
- `tests/`
- `docs/updates/ab-ota.md`
- `scripts/qemu-test.sh` (gated)

## Plan (small PRs)

1. **Slot state model (`slotstate`)**
   - Persist at `/state/boot/slot.json`:
     - `current`, `next`, `last_good`
     - `trial`, `boots_left`, `deadline_ns`
   - Provide atomic update helpers (statefs put_atomic once available).

2. **Health multiplexer (`healthd`)**
   - Minimal quorum that can work in early OS:
     - “core services ready” markers (or direct RPC probes)
     - statefs read/write probe
   - Optional sources if available later:
     - logd (fatal repeats), metrics counters, execd restart counts.
   - Deterministic clock injection for tests.

3. **Rollback controller**
   - On each “boot cycle”, if `trial=true`:
     - decrement `boots_left`
     - if `boots_left==0` or `now>deadline_ns` and not confirmed healthy → schedule rollback to `last_good`.
   - If health confirmed within grace → promote and clear trial.

4. **Soft-reboot proof**
   - Define a test-only mechanism to simulate a “new init cycle”:
     - e.g., re-run a minimal “init-lite boot sequence” inside selftest, or restart key services and re-read `slotstate`.
   - Markers must reflect this truth:
     - `... simulated boot ok (slot=B)` only after the new cycle uses B.

5. **Docs**
   - Document this as OTA v2 userspace state machine with “bootchain integration pending”.

