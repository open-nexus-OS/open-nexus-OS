---
title: TASK-0179 Updated v2 (offline, deterministic): pkg://updates feed + manifest verify (trust store) + full/delta apply + trial/confirm/rollback + selftests/docs
status: Draft
owner: @runtime
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Updates A/B skeleton baseline (updated v1.1): tasks/TASK-0007-updates-packaging-v1_1-userspace-ab-skeleton.md
  - OTA v2 state machine + health mux: tasks/TASK-0036-ota-ab-v2-userspace-healthmux-rollback-softreboot.md
  - Boot control service stub: tasks/TASK-0178-bootctld-v1-boot-control-stub-service.md
  - Delta updates format/tooling baseline (.nxdelta): tasks/TASK-0034-delta-updates-v1-bundle-nxdelta.md
  - Trust store unification (updated consumes trust): tasks/TASK-0160-identity-keystore-v1_1-os-attestd-trust-unification-selftests.md
  - Persistence (/state slots/history): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Updates UI/CLI (nx update): tasks/TASK-0140-updates-v1-ui-cli-settings-offline.md
  - Signing policy: docs/security/signing-and-policy.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We want an offline, deterministic updater with:

- A/B slot lifecycle (trial/confirm/rollback),
- signed manifests verified via the unified trust store,
- full-image apply and delta apply,
- a deterministic offline feed under `pkg://updates/`,
- QEMU proof via markers and a **soft-reboot simulation** (no real reboot).

Repo reality:

- Updates A/B skeleton exists as a task (`TASK-0007`) but is still draft.
- OTA v2 health/rollback state machine is tracked as `TASK-0036`.
- Delta tooling exists as a plan (`TASK-0034`) but is bundle-focused; we must not invent a second delta format.
- Trust store unification for `updated` is tracked in `TASK-0160`.

This task is an integration slice: it wires offline feed + verification + apply + trial/confirm/rollback using existing contracts.

Prompt mapping note (avoid drift from older plans):

- Some older prompts propose a “NUB” (JSON manifest + tar payload + detached signature) and a new `nx-update pack` pipeline.
- In this repo, `updated` must **not** invent a second payload contract:
  - verification must align with the canonical packaging direction from `TASK-0007` (bundle/system-set contracts),
  - trust roots must come from the unified trust store (`TASK-0160`),
  - and any “pack” tooling belongs to packaging tools (`nxb-pack`/system-set pack), not `nx update`.

## Goal

Deliver `source/services/updated` v2 behavior (service name remains `updated` to avoid drift) with:

1. Offline feed:
   - `pkg://updates/feeds/stable.nxf` lists manifest URIs deterministically (Cap'n Proto; canonical)
     - optional authoring/fixture input: `stable.json` compiled to `stable.nxf` at build time
   - deterministic “current version” reporting
   - marker: `updates: feed loaded items=<n>`
2. Manifest verification:
   - manifest includes `sha256/size` for artifacts and an Ed25519 signature
   - verify using unified trust store (`TASK-0160`) and deterministic error codes
   - marker: `update: manifest ok ver=<v> signer=<s>`
3. Apply to inactive slot:
   - write artifacts under `state:/slots/<a|b>/root.img` (or a documented slot path)
   - full apply: deterministic copy with hash verification
   - delta apply: reuse the existing delta format contract (`.nxdelta` from `TASK-0034`)
     - do **not** introduce “NBD1” as a new long-term format name
   - marker: `update: apply ok slot=<a|b> kind=<full|delta>`
4. Trial switch + soft reboot:
   - call `bootctld.setActive(inactive, trial=true, tries=3)`
   - `bootctld.reboot()` is marker-only
   - selftest must simulate the next “boot cycle” honestly (re-run init-lite or restart key services and re-read slot state)
   - marker: `update: await confirm`
5. Health check + rollback:
   - implement a minimal `healthd` (or selftest-driven health hook) consistent with `TASK-0036`:
     - check a small, stable quorum of “core ready” markers or RPC probes
     - if trial and healthy within timeout: confirm
     - if trial and unhealthy/timeout: rollback to previous slot
   - markers:
     - `health: trial ok -> confirm`
     - `health: fail -> rollback`
6. History:
   - append stable JSONL entries to `state:/updater/history.jsonl` (gated on `/state`)
7. OS selftests:
   - `SELFTEST: update apply ok`
   - `SELFTEST: update confirm ok`
   - `SELFTEST: update rollback ok`

## Non-Goals

- Kernel/bootloader changes.
- Real “slot B actually booted” via bootargs/OpenSBI (`TASK-0037`).
- Online networking feeds (devnet is a separate follow-up once networking is real).
- UI polish (Settings UX remains `TASK-0140`).

## Constraints / invariants (hard requirements)

- Offline & deterministic: `pkg://` only; no system time dependence.
- No fake success: trial/confirm/rollback markers only after real state transitions and verification happened.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points (track explicitly)

- **RED (reboot truth)**:
  - `bootctld.reboot()` cannot reboot; proof must be a soft-reboot simulation (same as `TASK-0036`).

- **RED (/state gating)**:
  - slot images and history persistence require `TASK-0009`. Without it:
    - apply can be demonstrated only as in-RAM placeholder and must not claim persistence across runs.

- **YELLOW (delta scope mismatch)**:
  - `TASK-0034` is bundle-delta. If we need a system-image delta, we must either:
    - generalize the `.nxdelta` library to support “raw image” kind, or
    - define a system-set delta task (`TASK-0035`) and keep v2 using full images only.
  - This task must pick **one** approach and document it explicitly.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p updater_v2_host -- --nocapture`
  - Required tests:
    - manifest verify ok + tamper fail
    - apply full + hash verify
    - delta apply path (only if `.nxdelta` supports raw-image kind; otherwise explicit “not in this step”)
    - slot switching calls `bootctld.setActive(...trial...)`
    - stable history JSONL lines

- **Proof (QEMU)**:
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=210s ./scripts/qemu-test.sh`
  - Required markers:
    - `bootctld: ready`
    - `updated: ready`
    - `SELFTEST: update apply ok`
    - `SELFTEST: update confirm ok`
    - `SELFTEST: update rollback ok`

## Touched paths (allowlist)

- `source/services/updated/` (new or extend)
- `source/services/bootctld/` (integration; from `TASK-0178`)
- `userspace/ota/` (slotstate/healthmux libs, if this task contributes)
- `tests/updater_v2_host/` (new)
- `source/apps/selftest-client/` (extend)
- `pkg://updates/` (fixtures feed/manifests/artifacts; exact repo path to be chosen)
- `docs/update/` (overview/feed/delta/cli/testing)
- `scripts/qemu-test.sh`

## Plan (small PRs)

1. Define offline feed + manifest schema + verification (trust store)
2. Full-image apply to inactive slot + slot state + markers
3. Health check integration + soft-reboot simulation + rollback path
4. Delta apply decision (generalize `.nxdelta` or defer) + host tests
5. OS selftests + docs + marker contract update

## Acceptance criteria (behavioral)

- Host tests and QEMU markers prove deterministic offline staging, trial scheduling, confirm, and rollback behavior.
