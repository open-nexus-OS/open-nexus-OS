---
title: TASK-0140 Updates v1 UI/CLI (offline): Settings page + nx update CLI + local payload selection + OS selftests/postflight/docs
status: Draft
owner: @ui
created: 2025-12-25
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Updates & Packaging v1.1 skeleton (updated + bootctl): tasks/TASK-0007-updates-packaging-v1_1-userspace-ab-skeleton.md
  - OTA A/B v2 state machine (health/rollback semantics): tasks/TASK-0036-ota-ab-v2-userspace-healthmux-rollback-softreboot.md
  - Boot control service stub (bootctld): tasks/TASK-0178-bootctld-v1-boot-control-stub-service.md
  - Updated v2 offline feed/delta/health integration: tasks/TASK-0179-updated-v2-offline-feed-delta-health-rollback.md
  - Persistence (/state bootctl): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Policy gates (updates.manage): tasks/TASK-0136-policy-v1-capability-matrix-foreground-adapters-audit.md
  - SystemUI→DSL Settings baseline: tasks/TASK-0121-systemui-dsl-migration-phase2a-settings-notifs-host.md
---

## Context

The repo already tracks offline A/B update mechanics under:

- `TASK-0007`: userspace-only A/B skeleton with `updated` and `bootctl.json` state transitions
- `TASK-0036`: more robust OTA v2 state machine and healthmux semantics (soft-reboot proof)

This task adds the **user-facing and developer-facing** control surfaces:

- a Settings → Updates page,
- a `nx update` CLI,
- and bounded OS selftests/postflight markers that prove the existing update machinery in QEMU.

Critically, we do **not** introduce a new `.nup` zip container or `manifest.json` contract here.
Update payloads must use the repo’s chosen system-set direction (e.g. `.nxs`/system index as per `TASK-0007`).

## Goal

Deliver:

1. Settings → Updates (DSL) page:
   - shows:
     - active slot (A/B), pending/trial state, triesLeft, current version (as reported by updated/bootctl)
   - actions (offline/local only):
     - pick local update payload via picker (`content://` or `pkg://selftest`)
     - verify (signature + digests) via `updated.Verify`
     - stage to inactive slot via `updated.Stage`
     - schedule switch (trial) via `updated.Switch`
     - show status/progress
   - markers:
     - `settings:updates open`
     - `updates: verify ok`
     - `updates: stage ok`
     - `updates: switch next`
2. `nx update` CLI:
   - `nx update verify <uri>`
   - `nx update stage <uri>`
   - `nx update switch`
   - `nx update status`
   - optional dev-only helpers if they already exist in updated (`mark-success`, `abort`)
   - stable output lines designed for deterministic parsing
3. Policy integration:
   - operations require `policyd.require("updates.manage")` (system-only default)
4. OS selftests + postflight:
   - use the existing canonical harness (`scripts/qemu-test.sh`) marker contract
   - add/extend selftest steps only once `updated` exists in QEMU:
     - `SELFTEST: ota stage ok`
     - `SELFTEST: ota switch ok`
     - `SELFTEST: ota rollback ok`
5. Docs:
   - extend `docs/updates/ab-skeleton.md` with UI/CLI workflow
   - add `docs/updates/tools.md` for `nx update`

## Non-Goals

- Kernel changes.
- Networking-based updates.
- Introducing a new update payload format (zip `.nup` + manifest.json). If we ever want that, it must be an explicit format decision task.

## Constraints / invariants (hard requirements)

- Offline-only: payload is local (`pkg://` or `content://`), no network fetch.
- No fake success: UI buttons/CLI must reflect real verification/state transitions.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

- CLI argument parsing + stable output tests (goldens) if needed.

### Proof (OS/QEMU) — gated

- Settings page opens and can drive verify/stage/switch against the real `updated` service.
- UART shows the existing OTA markers in `scripts/qemu-test.sh`.

## Touched paths (allowlist)

- `userspace/systemui/dsl/pages/settings/Updates.nx` (new)
- `userspace/systemui/dsl_bridge/` (extend: updated/bootctl adapters)
- `tools/nx-update/` (new)
- `source/apps/selftest-client/` (gated)
- `docs/updates/`

## Plan (small PRs)

1. DSL Updates page (host-first) + bridge stubs
2. nx-update CLI + stable output
3. OS wiring + selftests/postflight + docs (gated on `updated`)
