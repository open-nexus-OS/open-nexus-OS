---
title: TASK-0050 Reliability v1d: system reset (SBI SRST) + boot targets — bootctld as the single boot-state authority
status: Draft
owner: @reliability @runtime
created: 2025-12-23
updated: 2026-08-18
depends-on:
  - TASK-0009 # statefs (Done — record substrate)
  - TASK-0007 # A/B skeleton (Done — slot machine this task relocates)
follow-up-tasks:
  - TASK-0051 # recovery operations surface on top of targets
  - TASK-0050B # deferred bringup console
links:
  - Boot-state decision: docs/adr/0055-bootctld-single-boot-state-authority.md
  - Reliability contract: docs/rfcs/RFC-0087-reliability-failure-model-v1.md (§4 boot targets)
  - Init manifest/stages: docs/rfcs/RFC-0069-init-declarative-service-manifest-slot-discipline-boot-stages.md
  - Slot machine origin: docs/rfcs/RFC-0012-updates-packaging-ab-skeleton-v1.md
  - Authority registry: tasks/TRACK-AUTHORITY-NAMING.md
  - OTA v2 residual (client): tasks/TASK-0036-ota-ab-v2-userspace-healthmux-rollback-softreboot.md
  - Boot trust floor (builds on this record): tasks/TASK-0289-boot-trust-floor-v1-verified-boot-rollback-measurement.md
  - Testing contract: scripts/qemu-test.sh
---

## Rewrite 2026-08-18 (was: "Recovery v1a: boot target + recovery-init + recovery-sh + diag")

Re-cut against repo reality and ADR-0055:

- **`recovery-init` + `recovery-sh` binaries are dropped.** Recovery is a
  *declarative stage graph* in the init manifest, not a second init and not a
  UART REPL. The console idea survives as TASK-0050B (Deferred, bringup tool).
- **The `nexus.target` boot-arg parser in init is dropped.** Boot target lives
  in the persisted bootctld record; fw_cfg/boot-args select nothing (they
  remain selftest-profile plumbing only).
- **Diag bundle moves to TASK-0051** (`nx diagnose`, single format).
- The old ledger also predates the repo layout (`source/apps/` does not exist)
  and the reset primitive (none exists repo-wide) — both corrected here.

## Context

RFC-0087 Phase 3. Two verified holes:

1. **No reset path**: no SBI SRST caller anywhere — "reboot into recovery",
   soft-reboot rollback proof (0036) and crash-loop escalation (0049B) all
   dead-end without it.
2. **Boot-state authority drift**: four paper candidates (`updated` owns the
   shipped record, init/`nexus.target`, `rebootd` in 0261, `bootctld` in
   0178/0289). ADR-0055 decides: **bootctld**, one record, one owner; the
   proven machine RELOCATES — `userspace/updates/src/bootctrl.rs`
   (stage/switch/commit_health/tick_boot_attempt/rollback) +
   `source/services/updated/src/bootctl_state.rs` (Integrity envelope at
   `/state/boot/bootctl.v1`, monotonic seq) — it is not reimplemented.

## Goal

1. **`bootctld` service** (new, `source/services/bootctld/`): owns the boot
   record = active slot, `tries_left`, rollback index, boot-attempt counter
   (all existing) + `boot_target ∈ {normal|recovery|safe}` and one-shot
   `next_boot` (new fields, same envelope record, seq-monotonic).
   `updated` becomes a client for stage/switch/health/rollback; the QEMU OTA
   ladder (`SELFTEST: ota stage|switch|health|rollback ok`,
   `SELFTEST: bootctl persist ok`) stays green through the move.
2. **System reset** (kernel, approval zone): SBI SRST wrapper syscall —
   reboot / poweroff. Only bootctld may request reset with a target handoff
   (policy-gated); the kernel primitive itself stays dumb.
3. **Boot targets select stage graphs**: init reads the record at boot,
   consumes `next_boot` exactly once (one-shot), and materializes the target's
   declarative service set via the existing RFC-0069 stage machinery —
   `recovery` = minimal graph (statefsd, logd, policyd, bootctld, execd + ops
   surface), `safe` = session graph minus non-essential services. No second
   init path; the SAME orchestrator walks a narrower spec set.
4. **Escalation edge**: 0049B's critical-boot escalation
   (`init: escalation pending (no reset path)`) becomes real —
   crash-loop-exhausted critical-boot tier ⇒ `next_boot=safe` + reset.

## Non-Goals

- Verified boot / rollback-index *trust anchoring* (TASK-0289 builds on this
  record; this task keeps the bring-up trust model and says so).
- Recovery *operations* (fsck/slot/diag — TASK-0051).
- Interactive console (TASK-0050B, Deferred).
- Image-level A/B slots / real second image (0036 residual + storage ladder).
- flashd / provisioning (TASK-0260/0261 — rebased on this contract).

## Constraints / invariants (hard requirements)

- **Approval zones**: `source/kernel/**` (SRST syscall), `scripts/**`
  (qemu-test marker list + launcher reboot handling) — explicit approval first.
- One writer: only bootctld mutates the record; init only *consumes*
  `next_boot` (via bootctld op, not raw statefs access — statefs path
  namespacing stays enforceable).
- One-shot semantics: `next_boot` cleared in the SAME txn that acknowledges
  boot-attempt; a crash between reset and ack must not loop the target
  (both-or-neither via journal v2).
- Deterministic denial: target changes are policy-gated (deny-by-default),
  stable reject reasons, `test_reject_*` coverage.
- No fake green: reset proof = QEMU actually restarts and the harness observes
  the second boot's markers — never a simulated "would have rebooted" print.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **RED (harness reboot support)**: `scripts/qemu-test.sh` / launcher must
  tolerate a guest-initiated reboot in one run (QEMU `-no-reboot` today?
  verify). If the harness cannot observe reset-then-second-boot in one
  invocation, the proof composes the existing keep-blk double-boot lane
  (target set in run 1 → run 2 boots into target) — decide at execution
  start, record here; both variants are honest, a skipped reset is not.
- **YELLOW (record migration)**: adding fields to `bootctl.v1` — decide
  versioned record (`bootctl.v2` with migrate-on-first-write) vs additive
  fields; must survive existing-image double boot without wiping OTA state.
- **YELLOW (updated regression surface)**: the move must keep
  `updated: ready (statefs)` semantics and all 11 host tests
  (`tests/updates_host/`) green — port tests to the new client seam rather
  than deleting.

## Contract sources (single source of truth)

- Ownership: ADR-0055. Target semantics: RFC-0087 §4.
- Record + machine: `userspace/updates/src/bootctrl.rs` (relocates),
  `bootctl_state.rs` envelope discipline.
- Stage graphs: RFC-0069.

## Stop conditions (Definition of Done)

### Proof (Host) — required

- `cargo test -p bootctld` (new): record roundtrip incl. new fields, one-shot
  next_boot txn semantics, migration test, `test_reject_target_change_denied`.
- `tests/updates_host/` ported to client seam — all existing OTA flows green.

### Proof (OS/QEMU) — required

- `bootctld: ready` + `bootctld: target=normal next=none`
- Existing OTA ladder green (unchanged markers, relocated authority).
- `SELFTEST: boot target roundtrip ok` — set `next_boot=recovery` → reset (or
  keep-blk run 2) → `init: stage graph target=recovery` + minimal-graph-only
  services ready → target consumed (`bootctld: target=recovery next=none`).
- `SELFTEST: reset ok` — reset syscall observed as real QEMU restart (or
  documented double-boot composition per the RED decision).
- Safe-mode edge: `init: stage graph target=safe` reachable via escalation
  knob.

## Touched paths (allowlist)

- `source/services/bootctld/` (new; src/ + tests/ per service layout)
- `userspace/updates/` (bootctrl relocation seam)
- `source/services/updated/src/` (client conversion)
- `source/kernel/neuron/src/` (approval zone; SRST syscall)
- `source/libs/nexus-abi/` (approval zone; reset call + reject mapping)
- `source/init/nexus-init/src/` (next_boot consumption, target stage graphs)
- `source/apps/selftest-client/` + proof-manifest + `scripts/qemu-test.sh`
- `policies/` (target-change gating)
- `docs/reliability/` (boot targets section)

## Plan (small PRs)

1. bootctld skeleton + record relocation + host tests (updated still primary
   writer — flip only when green).
2. updated → client conversion; OTA ladder re-proof.
3. SRST syscall (after approval) + reset proof decision (RED above).
4. Target fields + one-shot semantics + policy gating.
5. Init stage-graph selection + recovery/safe graphs + full proofs;
   `just test-all`.
