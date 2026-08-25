---
title: TASK-0036 OTA A/B v2: health-commit v2 (record v3 + quorum + deadline) and BSB projection — bootctld machine evolution
status: Draft
owner: @runtime
created: 2025-12-22
updated: 2026-08-25
depends-on:
  - TASK-0050
follow-up-tasks:
  - TASK-0179
links:
  - Contract: docs/rfcs/RFC-0089-ota-v2-component-manifest-ab-boot-images-nxboot-bsb.md (§6 BSB, §13 record v3)
  - BSB actor discipline: docs/adr/0058-boot-selection-block-dual-actor-discipline.md
  - Authority: docs/adr/0055-bootctld-single-boot-state-authority.md
  - Health vocabulary: docs/rfcs/RFC-0087-reliability-failure-model-v1.md (§2) + docs/rfcs/RFC-0013-boot-gates-readiness-spawn-resource-v1.md (`ready`, never `init: up`)
  - Substrate for Phase B: tasks/TASK-0315-block-topology-consolidation-gpt-virtioblkd-sole-owner.md
  - Testing contract: scripts/qemu-test.sh
---

## REWRITE 2026-08-25 (RFC-0089 lane recut — supersedes the whole pre-rewrite body)

This ledger was written 2025-12-22 for a world without bootctld and carried
`healthd`/`bootargd`/soft-reboot/`/state/boot/slot.nxs` remnants that its own
2026-08-18 amendment had already declared dead. The rewrite cuts it to the two
bootctld-machine evolutions RFC-0089 assigns it. Decisions inherited and final:

- Machine home = bootctld (ADR-0055); this task evolves THAT machine through
  bootctld ops. `updated` stays a client. No `healthd`, no `bootargd`, no
  `userspace/ota/` crates, no soft-reboot simulation (real SBI reset since 0050).
- Shipped and NOT re-implemented: the tries-based machine
  (`source/services/bootctld/src/machine.rs`), record v2 persistence with
  snapshot-restore (`record.rs`/`persist_os.rs`, 13 host tests), the gated OTA
  ladder (`SELFTEST: ota stage/switch/health/rollback ok`), the reset lane.

## Context

Health-commit today is ONE unauthenticated-in-substance call: whoever reaches
`OP_HEALTH_OK` first commits the trial slot. There is no quorum over the services
that actually prove the boot, and no wall-clock bound — a trial slot that boots
but never advances can sit pending forever (tries only decrement on reset).
Additionally, RFC-0089 introduces the loader-readable BSB projection; bootctld —
the single boot-state authority — is its runtime writer.

## Goal

Two phases, separately provable:

**Phase A — health-commit v2 (no storage dependency; lands early):**

- Record v3 per RFC-0089 §13: v2 + `rollback_min_index u32` +
  `health_quorum_mask u8` + `commit_deadline_ns u64`; versioned decode
  (v3/v2/v1/legacy), writes always v3, snapshot-restore unchanged. The
  `rollback_min_index` FIELD lands now (raised by TASK-0179's commit path later)
  so the record never migrates twice.
- Quorum multiplexer inside bootctld: a bounded reporter set declared in the
  init service topology (RFC-0069 ServiceSpec — data, not code); reporters
  confirm via the existing health op; commit fires only when the mask completes.
  Duplicate confirmations are idempotent; unknown reporters are rejected.
- Wall-clock deadline: `commit_deadline_ns` armed at switch (time via the boot-
  proven `nsec` source, injectable in tests); expiry without quorum ⇒ rollback
  scheduled through the existing machine semantics (consumed by the next reset —
  no new transition mechanism).

**Phase B — BSB projection (after TASK-0315 provides the `bsb` partition):**

- bootctld gains a blockproto client to the `bsb` partition (init-provisioned
  route, deny-by-default elsewhere) and projects the record after EVERY committed
  mutation per the ADR-0058 write matrix: record commits FIRST, BSB second,
  `seq+1`, alternate-block write.
- Startup reconciliation: absorb loader actuator effects (tries decrement,
  exhaustion flip) into the record via the existing `OP_BOOT_ATTEMPT`/rollback
  semantics, then re-project idempotently (`bootctld: bsb resync` when differing).

## Non-Goals

- The loader itself and its BSB actuator writes (TASK-0289-A).
- Staging/apply, floor RAISING on commit, feed (TASK-0179).
- Any new daemon, any second boot-state store, kernel changes.

## Constraints / invariants (hard requirements)

- Quorum reporters are identified by kernel-attributed sender ids, never payload
  strings; the reporter set is declarative topology data.
- Deadline decisions use injectable time in tests; bounded everything (mask is a
  u8 — max 8 reporters, deliberate).
- Projection is a pure function of the record; no BSB-only state. Ordering:
  record → BSB, crash between healed by resync.
- No `unwrap/expect` on wire input; markers only after real behavior.

## Stop conditions (Definition of Done)

### Proof (Host) — required

Extends the existing 13 bootctld machine/record tests (do not duplicate them):

- `test_record_v2_to_v3_migration` (and v1/legacy chain stays green)
- `test_quorum_partial_no_commit` / `test_quorum_complete_commits`
- `test_reject_unknown_reporter` / `test_duplicate_reporter_idempotent`
- `test_deadline_expiry_schedules_rollback` (injected clock)
- `test_bsb_projection_golden` (record → 512-B block bytes, seq/CRC)
- `test_bsb_resync_after_crash_window` / `test_actuator_absorption`
  (tries-decremented/flipped BSB reconciles into the record, then re-projects)

### Proof (OS / QEMU)

Shipped ladder stays green untouched. New markers (headless ladder):

- Phase A: `bootctld: health quorum ok (n/n)`, `bootctld: commit deadline armed`,
  `SELFTEST: bootctl quorum ok`
- Phase B: `bootctld: bsb sync (seq=<n> active=<s> next=<s|none> tries=<n>)`,
  `bootctld: bsb resync` (resync lane), `SELFTEST: bootctl bsb ok`
- Reset profile three-boot lane stays byte-green (regression signal), plus
  `SELFTEST: bootctl persist ok` unchanged.

Marker doctrine: `scripts/qemu-test.sh` + `tools/nx/chains/markers.txt` +
proof-manifest TOMLs updated together.

## Touched paths (allowlist)

- `source/services/bootctld/` (machine.rs, record.rs, persist_os.rs, os_lite.rs,
  new bsb.rs)
- `source/init/nexus-init/` (reporter set in service topology; bsb route)
- `source/apps/selftest-client/` + `proof-manifest/markers/`
- `policies/base.toml` (bsb partition capability)
- `tests/` (bootctld host tests)
- `scripts/qemu-test.sh` (gated)
- `docs/` sweep per repo workflow

## Plan (small PRs)

1. **A1**: record v3 codec + migration + host tests.
2. **A2**: quorum multiplexer + declarative reporter set + deadline arm/expiry;
   selftest reporter wiring; QEMU markers.
3. **B1** (after 0315): bsb.rs projection writer + golden tests; init route +
   policy cap.
4. **B2**: startup reconciliation + resync lane; QEMU markers; docs sweep.
