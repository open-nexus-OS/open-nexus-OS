---
title: TASK-0051 Reliability v1e: recovery operations surface — statefsd fsck op + bootctld slot/target ops + nx diagnose (one bundle)
status: Done
owner: @reliability
created: 2025-12-23
updated: 2026-08-24
completed: 2026-08-24
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
  - **DECIDED 2026-08-24**: no drain — both ops REJECT with `STATUS_BUSY`
    while any txn is open (fsck re-opens from the raw device and would drop
    RAM-staged txns; racing = the silent-loss class RFC-0087 forbids). The
    probe retries bounded (5×, 200ms) against short-lived core txns (logd
    spill). QEMU-proven: `statefsd: fsck busy (open txns)`.
- **YELLOW (0227 seam)**: 0227 is Draft and lists `crashd` inputs that do not
  exist. Rebase 0227 to consume THIS task's query edges; bundle format stays
  0227-owned. No second bundle assembler.
  - **DECIDED 2026-08-18/24**: rebase note sits in 0227; `nx diagnose`
    delivered here (host assembly over statefs/logd/bootctld SSOT decoders).
    Bundle is a plain deterministic ustar `.tar` (NOT tar.zst — a
    compression layer buys nothing at these sizes and every zstd knob is a
    determinism hazard); recorded as the bundle-format decision for 0227.

## Execution recuts (documented DoD deltas, Option C)

- **Write budget = structural bound.** No separate repair budget knob:
  repair appends at most one ABORT per orphan and `MAX_OPEN_TXNS` (8) caps
  concurrent orphans — the bound is enforced by the engine's own limits
  (pinned by `test_repair_bound_is_structural`).
- **Sealed-value verify runs on the host twin only.** `EncContext` is
  engine-owned and non-clonable, so the OS op passes `enc=None`; the
  post-fsck re-open replay AEAD-verifies enrolled records anyway, and
  `fsck-statefs --enc-…` is the keyed verification path. The op still
  REPORTS enc counts and maps `enc_failures>0` to integrity-violation.
- **Induced corruption = orphan-across-reset (no external knob).** The DoD's
  "keep-blk image corrupted between runs" is delivered INSIDE the reset
  lane: boot 1 leaves a txn open (PREPARE/PAYLOAD journaled at append, RAM
  stage dies with the reset) → boot 2 proves `OP_FSCK_REPAIR` end to end
  (`statefsd: fsck repaired (n=1)` required). `fsck fail (unrecoverable)`
  stays a fatal signature, not a driven case — an unrecoverable journal
  cannot be induced without corrupting committed data mid-image, which the
  engine matrix covers on host (18 cases + 2 streaming-window cases; the
  DoD's "16-case" count predates the 0027 enc cases).
- **Finding (out of scope, follow-up): store-image naming is swapped.**
  virtio-mmio slots enumerate in reverse device order — the statefs
  journal lands in `build/data.img`, nxfs in `build/blk.img` (the
  launcher comment claims the opposite; keep-blk lanes preserve both, so
  no proof ever noticed). `nx diagnose` docs point at `data.img`; the
  rename/re-wire belongs to the storage lane, not here.
- **Engine touch WITH matrix (allowed by constraints):** the fsck scan now
  STREAMS the region through a bounded window (`fsck_window.rs`) instead of
  materializing it — the old whole-region `vec![0u8; …]` was a 64 MiB
  `alloc_zeroed` on statefsd's 1 MiB heap (killed the service mid-op).
  Pure memory-shape change, zero repair-semantics change: the full engine
  matrix + CLI twin stay green, plus two new window-spanning cases.

## Contract sources (single source of truth)

- fsck semantics/exit codes: `userspace/statefs/src/fsck.rs` +
  `tools/fsck-statefs/tests/cli.rs`.
- Boot record ops: ADR-0055 / TASK-0050 wire.
- Bundle format: TASK-0227 (rebased).

## Stop conditions (Definition of Done)

### Proof (Host) — required

- `cargo test -p statefsd`: op layer over engine — outcome matrix mirrored
  (16 cases via mock store), quiesce/Busy reject, budget cap, rejects.
  ✅ 2026-08-24 `tests/fsck_op_contract.rs` (8 tests: outcome mapping,
  orphan report/repair, quiesce gate, structural bound, wire rejects);
  engine matrix `cargo test -p statefs` (55 unit + 18 matrix) +
  `cargo test -p fsck-statefs` (9 CLI) stay the semantics guard.
- `cargo test -p bootctld`: schedule-only semantics, commit-blocked reject,
  rejects (sender/grant/target). ✅ machine/wire host tests + commit-block
  gate keyed on the session graph (RAM one-shot, not the persisted target).
- `nx diagnose` host test: fixture inputs ⇒ deterministic archive with
  expected sections. ✅ 2026-08-24 `tools/nx/tests/diagnose_cli.rs`
  (byte-identical reruns, section list, exit-class rejects, absent-store
  honesty).

### Proof (OS/QEMU) — required

- `statefsd: fsck check ok (clean)` / `statefsd: fsck repaired (n=<k>)` —
  induced corruption = orphan-across-reset (see recuts);
  `statefsd: fsck fail (unrecoverable)` is a fatal signature (host-matrix
  case, not OS-driven). ✅ reset lane 2026-08-24: repaired (n=1) + busy +
  check clean, all required markers.
- `SELFTEST: recovery fsck ok` ✅
- `bootctld: switch scheduled (to=<slot>)` (required in the OTA ladder,
  emitted only after the record persisted) + `bootctld: commit blocked
  (target=recovery)` + `SELFTEST: recovery slot ok` ✅
- `SELFTEST: recovery ops deny ok` (ungranted mutating op → stable reject +
  audit record) ✅
- `nx diagnose` against a QEMU run produces the bundle (documented command
  in `docs/reliability/recovery-operations.md`; host-side check). ✅

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
