---
title: TASK-0051 Reliability v1e: recovery operations surface — statefsd fsck op + bootctld slot/target ops + nx diagnose (one bundle)
status: Draft
owner: @reliability
created: 2025-12-23
updated: 2026-08-18
depends-on:
  - TASK-0026 # fsck engine (Done — exposed here, NOT rebuilt)
  - TASK-0050 # bootctld + targets (ops act on its record/graph)
  - TASK-0049C # persistent evidence (diagnose reads it)
follow-up-tasks:
  - TASK-0053 # .nxra tokens gate the mutating ops
links:
  - Reliability contract: docs/rfcs/RFC-0087-reliability-failure-model-v1.md
  - Boot-state authority: docs/adr/0055-bootctld-single-boot-state-authority.md
  - fsck engine (SSOT): userspace/statefs/src/fsck.rs + tools/fsck-statefs/
  - Keystone Gate 6 (nx = only diagnostics CLI): tasks/TRACK-KEYSTONE-GATES.md
  - Diag-bundle sibling (host pipeline): tasks/TASK-0227-diagnostics-v1-bugreport-bundles-nx-diagnose-offline-deterministic.md
  - Testing contract: scripts/qemu-test.sh
---

## Rewrite 2026-08-18 (was: "Recovery v1b: safe tools + recovery-sh built-ins + nx recovery CLI")

Re-cut against repo reality:

- **The planned `statefsd Check()/Repair()` API + `fsck state` shell built-in
  would rebuild what TASK-0026 shipped 2026-08-18**: validate/repair core in
  `userspace/statefs/src/fsck.rs` (append-only repair, encrypted-image verify
  from 0027) + host CLI `tools/fsck-statefs` with a 16-case outcome matrix and
  stable exit codes. This task now *exposes* that engine as an OS op — one
  repair semantics, one exit-code contract, host CLI stays the twin.
- **Slot ops target `bootctld`** (ADR-0055), not a parallel slot surface; the
  machine relocated in TASK-0050.
- **`recovery-sh` built-ins are gone** (TASK-0050B, Deferred). Operations are
  policy-gated IPC ops + `nx` subcommands — Keystone Gate 6: `nx` is the only
  diagnostics CLI; the three competing diag shapes (recovery-sh `diag`, 0261
  `nx-diag`, 0227 `nx diagnose`) collapse into **`nx diagnose`**.

## Context

RFC-0087 Phase 4a. With targets (0050) and evidence (0049C) in place, recovery
needs an *operations surface*: check/repair the boot-critical store, inspect
and schedule slot/target changes, and pull one deterministic diagnostic bundle
— all without a shell, all deny-by-default.

## Goal

1. **statefsd fsck op**: new wire ops `OP_FSCK_CHECK` / `OP_FSCK_REPAIR`
   running the 0026 engine against the mounted store (bounded write budget on
   repair; append-only semantics preserved; `--enc` sealed-value verify
   included). Enabled only in `recovery` target or via explicit policy grant;
   results map 1:1 to the fsck-statefs exit-code contract.
2. **bootctld ops**: `slot status` (read), `slot switch` / `target set`
   (schedule-only — writes `next_boot`/pending switch; commits are the normal
   boot path's job). Outside `recovery`, mutating ops additionally require the
   policy grant; `updated: commit blocked in recovery` semantics move here as
   `bootctld: commit blocked (target=recovery)`.
3. **`nx diagnose`**: one bundle format (deterministic tar.zst: evidence
   journal export via logd `persisted` scope, boot record snapshot, fsck
   report, version/build info). Host-side assembly over existing query
   surfaces — no new on-device bundler daemon. Coordinate scope with
   TASK-0227 (that ledger keeps the *host pipeline/format* ownership; this
   task delivers the on-device query edges it needs — no duplicate bundle
   format).
4. **Policy**: all mutating ops deny-by-default (`policyd`), stable reject
   reasons, audit records to logd (evidence class).

## Non-Goals

- Interactive shell (TASK-0050B Deferred).
- `.nxra` token *format/verification* (TASK-0053 — this task leaves an
  enforcement hook: mutating ops carry an optional token field that 0053
  turns mandatory per policy).
- OTA staging/feed (TASK-0036/0179 lane).
- nxfs fsck op (same pattern later, `/data` lane — statefs is the
  boot-critical store and goes first).
- Remote diagnostics (TASK-0040 lane).

## Constraints / invariants (hard requirements)

- ONE repair semantics: the engine in `userspace/statefs/src/fsck.rs` is the
  SSOT; the op layer adds transport + gating only. Any repair-behavior change
  happens in the engine + its 16-case matrix, never in the op layer.
- Repair is bounded (write budget) and audited; check is read-only.
- fsck-on-mounted-store discipline: ops run only while statefsd holds the
  store quiesced (no concurrent txns during repair — reject with
  `Busy`-class error instead of racing).
- Deny-by-default; identity = kernel-attributed sender id; `test_reject_*`
  for every mutating op (wrong sender, no grant, wrong target, busy).
- Deterministic bundle: same inputs ⇒ byte-identical `nx diagnose` output
  (no timestamps inside entries beyond record-carried ones).
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **YELLOW (quiesce semantics)**: repair on the live store needs a quiesce
  gate in statefsd (pause txn intake, drain, run engine, resume). If drain
  proves risky in bring-up, the honest fallback is check-online /
  repair-only-in-recovery-target — decide at execution, record here.
- **YELLOW (0227 seam)**: 0227 is Draft and lists `crashd` inputs that do not
  exist. Rebase 0227 to consume THIS task's query edges; bundle format stays
  0227-owned. No second bundle assembler.

## Contract sources (single source of truth)

- fsck semantics/exit codes: `userspace/statefs/src/fsck.rs` +
  `tools/fsck-statefs/tests/cli.rs`.
- Boot record ops: ADR-0055 / TASK-0050 wire.
- Bundle format: TASK-0227 (rebased).

## Stop conditions (Definition of Done)

### Proof (Host) — required

- `cargo test -p statefsd`: op layer over engine — outcome matrix mirrored
  (16 cases via mock store), quiesce/Busy reject, budget cap, rejects.
- `cargo test -p bootctld`: schedule-only semantics, commit-blocked reject,
  rejects (sender/grant/target).
- `nx diagnose` host test: fixture inputs ⇒ deterministic archive with
  expected sections.

### Proof (OS/QEMU) — required

- `statefsd: fsck check ok (clean)` / `statefsd: fsck repaired (n=<k>)` /
  `statefsd: fsck fail (unrecoverable)` — induced-corruption knob
  (keep-blk image corrupted between runs, same technique as the 0026 ladder)
- `SELFTEST: recovery fsck ok`
- `bootctld: switch scheduled (to=<slot>)` + `bootctld: commit blocked
  (target=recovery)` + `SELFTEST: recovery slot ok`
- `SELFTEST: recovery ops deny ok` (ungranted mutating op → stable reject +
  audit record)
- `nx diagnose` against a QEMU run produces the bundle (documented command in
  docs; host-side check).

## Touched paths (allowlist)

- `source/services/statefsd/src/` (fsck ops + quiesce)
- `source/services/bootctld/src/` (slot/target ops)
- `userspace/statefs/` (client additions only; engine untouched unless the
  matrix grows WITH tests)
- `tools/nx/` (`nx diagnose`, `nx recovery …` verbs)
- `policies/` (grants), `source/apps/selftest-client/` + proof-manifest +
  `scripts/qemu-test.sh`
- `docs/reliability/` (operations section)

## Plan (small PRs)

1. statefsd fsck op host-first (mock store matrix + quiesce + rejects).
2. bootctld ops host-first (schedule-only + rejects).
3. OS wiring + induced-corruption QEMU proof.
4. `nx diagnose` + 0227 rebase note + docs; `just test-all`.
