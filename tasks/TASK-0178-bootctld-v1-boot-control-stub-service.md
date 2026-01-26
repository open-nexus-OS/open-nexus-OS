---
title: TASK-0178 Boot control v1 (OS/QEMU): bootctld stub service + slot/trial state + markers (no real reboot)
status: Draft
owner: @runtime
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Updates A/B skeleton baseline: tasks/TASK-0007-updates-packaging-v1_1-userspace-ab-skeleton.md
  - OTA v2 state machine baseline: tasks/TASK-0036-ota-ab-v2-userspace-healthmux-rollback-softreboot.md
  - Persistence (/state): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Testing contract: scripts/qemu-test.sh
  - Data formats rubric (JSON vs Cap'n Proto): docs/adr/0021-structured-data-formats-json-vs-capnp.md
---

## Context

Current updates tasks reference a file-based “bootctl.json” slot selector. For deterministic, auditable orchestration,
we want an explicit service boundary:

- `bootctld` owns the slot state model (active/inactive/trial/triesLeft),
- `updated` (and later recovery tools) talk to `bootctld` via a small IDL,
- QEMU proof remains honest: `reboot()` is a marker-only stub unless/ until a real boot chain contract exists.

This task introduces the service and state model only. Updater orchestration and health/rollback logic remains in `TASK-0179` / `TASK-0036`.

## Goal

Create `source/services/bootctld` with an IDL (`bootctl.capnp`) and a deterministic state machine:

- `state() -> BootState`
- `setActive(slot, trial, triesLeft)`
- `confirm()` (mark current slot good; clear trial)
- `markBad(slot)` (deny boot until re-imaged; state is sticky)
- `reboot()` (QEMU stub: emits marker only)

Persist under `state:/bootctl/state.nxs` (Cap'n Proto snapshot; canonical) when `/state` exists; otherwise store in RAM and emit explicit `stub/placeholder` markers.

Markers:

- `bootctld: ready`
- `bootctl: setActive slot=<a|b> trial=<true|false> tries=<n>`
- `bootctl: confirm`
- `bootctl: markBad slot=<a|b>`
- `bootctl: reboot (stub)`

## Non-Goals

- Kernel changes.
- Real reboot / boot-chain integration (blocked; see `TASK-0037`).
- Updating images or bundles (updated/updater logic).

## Constraints / invariants (hard requirements)

- Deterministic transitions and stable marker strings.
- No fake persistence: if `/state` is not real yet, do not claim state survived a reboot.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points (track explicitly)

- **RED (reboot semantics)**:
  - `reboot()` is marker-only until boot-chain integration exists. Any “booted slot B” claims must be based on a soft-reboot simulation.

- **RED (/state gating)**:
  - Persistence requires `TASK-0009`. Without it, bootctl state must be labeled as non-persistent.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - `cargo test -p bootctld_host -- --nocapture` (new tiny test crate) proving:
    - setActive/confirm/markBad determinism
    - serialization roundtrip stability for `state.nxs` (byte-stable)
    - optional derived/debug view export to JSON is deterministic (if implemented)

- **Proof (QEMU)**:
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=120s ./scripts/qemu-test.sh`
  - Required markers:
    - `bootctld: ready`
    - `SELFTEST: bootctl api ok`

## Touched paths (allowlist)

- `source/services/bootctld/` (new)
- `source/services/bootctld/idl/bootctl.capnp` (new)
- `source/apps/selftest-client/` (extend with a minimal bootctl RPC smoke test)
- `tests/bootctld_host/` (new)
- `scripts/qemu-test.sh` (marker contract update)
- `docs/updates/bootctl.md` (optional; or extend an existing updates doc)

## Plan (small PRs)

1. IDL + service skeleton + deterministic state model + markers
2. host tests for state transitions + JSON roundtrip golden
3. OS selftest marker + docs note about stub reboot

## Acceptance criteria (behavioral)

- `bootctld` API is stable and deterministic; QEMU shows readiness and a bootctl API smoke-test marker.
