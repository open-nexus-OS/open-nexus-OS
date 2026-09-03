---
title: TASK-0035 Delta updates v1b (system sets): nxs delta container + updated orchestration
status: Draft
owner: @runtime
created: 2025-12-22
updated: 2026-09-03
depends-on:
  - TASK-0034
  - TASK-0321
follow-up-tasks: []
links:
  - Vision: docs/architecture/vision.md
  - Depends-on (bundle deltas): tasks/TASK-0034-delta-updates-v1-bundle-nxdelta.md
  - Depends-on (updates service): tasks/TASK-0007-updates-packaging-v1_1-userspace-ab-skeleton.md
  - Depends-on (supply-chain policy): tasks/TASK-0029-supply-chain-v1-sbom-repro-sign-policy.md
  - Signing policy: docs/security/signing-and-policy.md
---

## Parked 2026-09-03 — executes after TASK-0321 (Phase B seam)

The Phase-B seam this task's residual scope sits on now has a ledger:
`tasks/TASK-0321-ota-phase-b-verified-system-volume-bundle-set.md` (system
volume + `bundle` components + reuse index). This task starts when 0321 P3
(`bundle` apply) is green; until then nothing here is buildable without
faking the bundle population.

## Rebase note 2026-08-25 (RFC-0089: no aggregate delta container — component kinds instead)

RFC-0089 §3/§11 dissolves this ledger's core artifact: there is no separate
"system-set delta container". The `.nxs` v2 COMPONENT MANIFEST already carries
typed components, so a delta update is simply a container whose components are
`boot-image-delta` (TASK-0034, v1 lane) and — after the Phase-B bundle-set seam
(RFC-0089 §12) — `bundle-delta` entries with unchanged-bundle reuse from the
active system volume. The residual scope of THIS task collapses to the
`updated`-side multi-component delta orchestration for Phase B (apply order,
per-component resume, reuse index); it executes after TASK-0034's
`boot-image-delta` kind and the Phase-B seam exist. depends-on unchanged
(TASK-0034); the 2026-08-14 note below remains accurate about what shipped but
its "aggregate delta container" framing is superseded.

## Rebase 2026-08-14 — unblocked (verified repo reality)

All three original blockers are gone; status moves `Blocked` → `Draft` with
`depends-on: [TASK-0034]` only (the `.nxdelta` bundle-delta format this task
aggregates).

- **`.nxs` tooling exists and is live end-to-end**: `tools/nxs-pack` ships;
  `source/apps/selftest-client/build.rs:53` generates `system-test.nxs` for
  every OS build, parsed by `userspace/updates/src/system_set.rs` (441 LOC).
  Do NOT re-implement `.nxs` packing/parsing.
- **`updated` exists persistently** (no longer a non-persistent skeleton):
  `source/services/updated/src/os_lite.rs` (971 LOC), bootctl state persisted
  at `/state/boot/bootctl.v1` (`os_lite.rs:53`), marker
  `updated: ready (statefs)` gated at `scripts/qemu-test.sh:482`.
- **The boot-chain blocker does not apply to this task's DoD**: that item was
  TASK-0037, which is being superseded by TASK-0289. This task's own DoD only
  requires staging to the inactive slot — which `handle_stage`
  (`os_lite.rs:396`) already does, QEMU-gated via
  `SELFTEST: ota stage ok` (`scripts/qemu-test.sh:536-540`).

Honest residual scope: the aggregate system-set delta container (list of
per-bundle `.nxdelta` patches + integrity index) and the `updated`-side
orchestration on top of the already-shipped stage/switch machinery. This
cannot start before TASK-0034's residual `.nxdelta` lane lands.

## Context

We eventually want system-set (`.nxs`) delta updates that apply a set of bundle deltas and stage an A/B update.

## Goal

Once unblocked, deliver:

- an aggregate delta container for system sets (list of per-bundle patches + integrity index),
- updated-side orchestration:
  - apply per-bundle deltas via bundlemgrd,
  - verify supply-chain policy for all bundles,
  - stage atomically to the target slot,
  - persist checkpoints for resume.

## Stop conditions (Definition of Done)

- Host tests: system delta container make/apply matches expected system set digest.
- OS/QEMU: markers for system delta start/verify/staged and selftest proofs.

## Red flags / decision points

- **RED**: cannot start until TASK-0034's residual `.nxdelta` lane lands
  (the format this task aggregates). The former blockers — `.nxs` tooling,
  persistent `updated`, boot-chain proof — are resolved (see Rebase
  2026-08-14).
