---
title: TASK-0036 OTA A/B v2 (userspace): slot state machine + health multiplexer + rollback timer (soft-reboot proof)
status: Draft
owner: @runtime
created: 2025-12-22
updated: 2026-08-14
depends-on: []
follow-up-tasks: []
links:
  - Vision: docs/architecture/vision.md
  - Packaging/updates baseline: tasks/TASK-0007-updates-packaging-v1_1-userspace-ab-skeleton.md
  - Supply-chain baseline: tasks/TASK-0029-supply-chain-v1-sbom-repro-sign-policy.md
  - Persistence substrate: tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Audit/observability (optional): tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md
  - Testing contract: scripts/qemu-test.sh
  - Data formats rubric (JSON vs Cap'n Proto): docs/adr/0021-structured-data-formats-json-vs-capnp.md
---

## Rebase 2026-08-14 — what already shipped (do NOT re-implement)

Verified against the repo on 2026-08-14.

**Decision (ownership): TASK-0036 is THE owner of the A/B slot-state-machine
evolution.** TASK-0178 (bootctld stub) and TASK-0179 (updated v2 offline feed)
overlap with this task; both defer to TASK-0036 for the slot state machine,
health multiplexing, rollback, and deadline semantics, and must rebase against
this task's outcome before execution.

**Shipped — the slot/trial/rollback state machine core:**

- `userspace/updates/src/bootctrl.rs` implements the machine this ledger's
  Plan steps 1 and 3 describe: `stage` (:82), `switch` with `tries_left`
  (:88), `commit_health` (:102), `tick_boot_attempt` (:113, auto-rollback on
  exhausted tries), `rollback` (:127). Do NOT re-implement.
- Persistence is real: `source/services/updated/src/os_lite.rs:53`
  `BOOTCTRL_STATE_KEY = "/state/boot/bootctl.v1"` (note: the shipped path is
  `bootctl.v1`, not this ledger's `/state/boot/slot.nxs`), marker
  `updated: ready (statefs)` gated at `scripts/qemu-test.sh:482`.
- `updated` service side: `source/services/updated/src/os_lite.rs` (971 LOC)
  with `handle_stage` :396, `handle_switch` :463, `handle_health_ok` :516,
  `handle_boot_attempt` :555.
- Host proof: `tests/updates_host/tests/ota_flow.rs` (11 tests, incl.
  `rollback_on_health_timeout` :105, `reject_mismatched_digest` :92).
- QEMU proof: full OTA ladder gated at `scripts/qemu-test.sh:536-540`
  (`SELFTEST: ota stage/switch/health/rollback ok`) plus negative proofs in
  `proof-manifest markers/ota.toml:39-54`.

**Honest residual scope (what v2 still adds):**

- (a) **Health multiplexer with quorum** — nothing named healthmux/quorum
  exists anywhere in the repo (grep: zero hits). Today health confirmation is
  a single `handle_health_ok` call.
- (b) **Wall-clock `deadline_ns` alongside tries** — the shipped machine is
  tries-based only (`tries_left` / `tick_boot_attempt`); there is no
  wall-clock deadline path.
- (c) **`healthd`** — does not exist. Also decide whether `bootargd` is still
  needed at all, given the boot-chain task moved TASK-0037 → TASK-0289.
- (d) **Soft-reboot simulation proof** — the "new init cycle uses slot B"
  simulated-boot marker lane is not built.

## Context

We want robust A/B OTA behavior:

- stage into inactive slot,
- schedule a trial boot,
- confirm health via a health multiplexer,
- auto-rollback on timeout/degradation.

Repo reality (superseded by the Rebase 2026-08-14 section above — kept for
history):

- `updated` is a persistent 971-LOC service with a QEMU-gated
  stage/switch/health/rollback ladder (see Rebase section); v2 adds health mux
  quorum, wall-clock deadline, and soft-reboot proof on top.
- “Boot slot via SBI/bootargs” cannot be *truly* proven without boot chain/kernel/firmware integration.

This task focuses on the **userspace state machine** and provides **honest proof** via a soft-reboot simulation.

## Goal

Deliver a userspace A/B OTA v2 state machine with:

- durable slot state under `/state/boot/slot.nxs` (Cap'n Proto snapshot; canonical),
- atomic stage/commit semantics (inactive slot),
- a health multiplexer (quorum + timeouts),
- rollback timer (boots-left and/or deadline),
- deterministic host tests and OS selftest markers using a **soft reboot simulation**.

## Non-Goals

- Real OpenSBI bootargs wiring (separate blocked task).
- Real `.nxs` system set staging (owned by TASK-0035; tooling and `updated` exist now).
- Kernel changes.

## Constraints / invariants (hard requirements)

- Kernel untouched.
- No fake success: “booted slot B” is only claimed after the simulated new init cycle uses slot B configuration.
- Deterministic tests: injectable clock, bounded timeouts, stable markers.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **RED**:
  - Without a boot chain, “bootargs” cannot be validated. Proof is limited to **soft-reboot** simulation.
- ~~**YELLOW**: health signal sources `execd/metricsd/logd/statefs` are
  "planned not implemented".~~ **CORRECTED 2026-08-14**: all four services
  are shipped and marker-gated. The health mux can draw on real sources from
  day one; the open work is the multiplexer/quorum itself (see Rebase
  section), not the sources.

## Contract sources (single source of truth)

- Supply-chain verification expectations: TASK-0029
- Persistence: TASK-0009
- QEMU marker contract: `scripts/qemu-test.sh`

## Stop conditions (Definition of Done)

### Proof (Host) — required

Note (2026-08-14): the tries-based stage/switch/health/rollback flows are
already covered by `tests/updates_host/tests/ota_flow.rs` (11 tests) — do not
duplicate them. New v2 tests cover only the residual scope:

- wall-clock `deadline_ns`: no health confirmation before deadline → rollback scheduled to last_good
- health-mux quorum: confirmation within grace → promote and clear trial
- degradation path: repeated “critical restart” events triggers unhealthy decision
- soft-reboot simulation drives the above deterministically (injectable clock)

### Proof (OS / QEMU)

Already shipped and gated (do not re-add): `SELFTEST: ota stage/switch/health/rollback ok`
at `scripts/qemu-test.sh:536-540` + negatives in `proof-manifest markers/ota.toml:39-54`.

New v2 markers (residual scope only):

- `SELFTEST: ota simulated boot ok (slot=B)`
- `SELFTEST: ota deadline rollback ok`
- `SELFTEST: ota health quorum ok`

## Touched paths (allowlist)

- `source/services/`:
  - `bootargd` (only if still needed — decide first, given TASK-0037 → TASK-0289; see Rebase section)
  - `healthd` (health multiplexer; minimal sources first)
  - `updated` (exists — extend `source/services/updated/`, do not introduce a parallel orchestrator)
- `userspace/ota/` (`slotstate`, `healthmux` libs)
- `source/apps/selftest-client/`
- `tests/`
- `docs/updates/ab-ota.md`
- `scripts/qemu-test.sh` (gated)

## Plan (small PRs)

1. **Slot state model** — ✅ **largely SHIPPED** (Rebase 2026-08-14; do NOT
   re-implement): `userspace/updates/src/bootctrl.rs` + persistence at
   `/state/boot/bootctl.v1` (`os_lite.rs:53`). Residual: add the wall-clock
   `deadline_ns` field alongside the existing tries-based machinery.

2. **Health multiplexer (`healthd`)**
   - Minimal quorum that can work in early OS:
     - “core services ready” markers (or direct RPC probes)
     - statefs read/write probe
   - Optional sources if available later:
     - logd (fatal repeats), metrics counters, execd restart counts.
   - Deterministic clock injection for tests.

3. **Rollback controller** — ✅ **tries-based path SHIPPED** (Rebase
   2026-08-14; do NOT re-implement): `tick_boot_attempt` (bootctrl.rs:113)
   decrements tries and auto-rolls-back; `commit_health` (:102) promotes.
   Residual: the `now>deadline_ns` wall-clock branch.

4. **Soft-reboot proof**
   - Define a test-only mechanism to simulate a “new init cycle”:
     - e.g., re-run a minimal “init-lite boot sequence” inside selftest, or restart key services and re-read `slotstate`.
   - Markers must reflect this truth:
     - `... simulated boot ok (slot=B)` only after the new cycle uses B.

5. **Docs**
   - Document this as OTA v2 userspace state machine with “bootchain integration pending”.
