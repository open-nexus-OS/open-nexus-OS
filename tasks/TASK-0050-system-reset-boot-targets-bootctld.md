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

1. ✅ **PR-1 (2026-08-20) — bootctld skeleton + record v2 + host tests**:
   new `source/services/bootctld/` (src/ + tests/ per service layout). The
   RFC-0012 machine RELOCATED verbatim into `machine.rs` and extended with
   the target axis (`boot_target`, one-shot `next_boot` via
   `take_next_boot()`) plus `restore(..)` — the v2 record (9-byte payload,
   subject `bootctld`, same key `/state/boot/bootctl.v1`) persists EVERY
   field including the rollback slot, killing the v1-era stage/switch
   replay-reconstruction hack. Reads migrate v1 payloads (rollback derives
   as `active.other()` — the only switch the machine can make) and
   pre-envelope legacy bytes; unknown target bytes are Corrupted (fail
   closed — a newer image's target must never silently boot `normal`).
   13 host tests incl. the 5 machine flows ported from
   `tests/updates_host/ota_flow.rs` (relocation cannot drift). OS slice is
   READ-ONLY by design: serve = GET_STATUS/GET_TARGET, every mutating op
   answers UNSUPPORTED — updated stays the record's single writer until
   the client flip (one-writer invariant during the transition). Boot
   integration: ServiceId::Bootctld (28), declarative spec (reply inbox +
   statefsd route), supervision tier critical-boot/Always (the SSOT test
   forces Always on that tier), `bootctld = ["ipc.core", "statefs.boot"]`
   (the /state/boot/* prefix gates reads AND writes on statefs.boot),
   spawned LAST (no boot-layout shift for existing services). Markers:
   `bootctld: ready` + `bootctld: target=normal next=none` gated; the
   "defaults" fallback markers are FATAL in proof boots.
   Wire constants live in `bootctld::wire` (cfg-free; ops 1..=5 mirror the
   updated wire so the PR-2 client conversion is a target swap).
   **Harness recut (approved zones)**: the reliability-spine proofs grew
   the ladder past the old wall-clock caps — outer `RUN_TIMEOUT` 90s→180s
   and launcher ready-grace 90s→150s (both re-measured; early-stop still
   ends green runs ~2s after their final marker, so only hanging runs pay
   the wider window). The 90s cap cut the restart storm's third cycle
   deterministically and looked exactly like an init hang — recorded here
   because the next person WILL hit it when the ladder grows again.
2. ✅ **PR-2 (2026-08-24) — updated → bootctld client; OTA ladder
   re-proven**: bootctld is the single WRITER now (mutations sender-gated
   on the kernel-attributed id: OTA ops `updated`-only, boot-attempt also
   init; deterministic `STATUS_DENIED`, no forgeable probe surface). Every
   mutation is both-or-neither: snapshot → mutate → persist (relocated
   read-modify-write incl. the one-retry rollback-race rule) — a failed
   persist restores the snapshot, so RAM and disk can never diverge (the
   old updated writer mutated first and persisted second). updated keeps
   the OTA fassade (U,D wire, SystemSet verification, audit markers —
   ladder markers unchanged) and delegates via `bootctl_client` (CAP_MOVE
   onto its own inbox, foreign frames skipped; `OP_ROLLBACK` added for the
   bundlemgrd-failure compensation). `updates::bootctrl` and updated's
   `bootctl_state`/persist path are DELETED (relocation complete; machine
   flows live in bootctld's host tests). init's boot-attempt handshake now
   calls bootctld DIRECTLY over the pre-minted init-owned request endpoint
   — the responder is not serving yet at that point, so bootctld is
   BESPOKE-wired with FIXED slots (inbox 5/6, statefsd send 7, metricsd
   convention; `provision_bootctld_fixed_slots`) and loads the record
   eagerly. The boot-attempt reply already carries
   `[rolled_back, next_boot]` — one-shot consumption rides the same
   persisted commit; init materializes targets in PR-5.
   Marker recut (contract): `updated: ready (statefs)` →
   `updated: ready (bootctl client)` (updated no longer reads the record);
   selftest's persist check accepts payload v1 AND v2.
   **Trap recorded**: `distribute_server_pairs` pre-grants server slots
   3/4 for every service with a minted pair — a bespoke arm must GUARD on
   `chan.recv(id).is_none()` before transferring again, or the pair shifts
   to 5/6 and fixed-slot transfers collide (cost one red boot).
3. SRST syscall (after approval) + reset proof decision (RED above).
4. Target fields + one-shot semantics + policy gating.
5. Init stage-graph selection + recovery/safe graphs + full proofs;
   `just test-all`.
