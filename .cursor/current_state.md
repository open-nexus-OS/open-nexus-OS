# Cursor Current State (SSOT)

<!--
CONTEXT
This file is the single source of truth for the current system state.
Keep it compact, explicit, and contract-oriented.
-->

## Current architecture state
- **last_decision**: complete `TASK-0018` Finalphase (Phase 4) with identity/report hardening, explicit negative E2E rejects, and follow-up drift lock before final closeout.
- **rationale**:
  - crashdump v1 must remain kernel-unchanged and deterministic,
  - v1 must avoid drift with existing crash follow-ons (`TASK-0048`, `TASK-0049`, `TASK-0141`, `TASK-0227`),
  - proofs must stay honest-green (artifact/event path in OS, symbolization on host).
- **active_constraints**:
  - kernel untouched in this slice,
  - in-process capture only for v1 (no ptrace-like post-mortem path),
  - bounded dump sizes and deterministic path normalization under `/state/crash/...`,
  - no fake-success markers (`minidump written` only after real write success),
  - host-first symbolization; on-device DWARF remains out of v1 scope.

## Current focus (execution)
- **active_task**: `tasks/TASK-0018-crashdumps-v1-minidump-host-symbolize.md` (In Review, implementation + proofs complete)
- **seed_contract**:
  - `tasks/TASK-0018-crashdumps-v1-minidump-host-symbolize.md`
  - `docs/rfcs/RFC-0031-crashdumps-v1-minidump-host-symbolize.md` (In Review)
  - `docs/rfcs/RFC-0011-logd-journal-crash-v1.md`
  - `docs/rfcs/RFC-0018-statefs-journal-format-v1.md`
- **contract_dependencies**:
  - `tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md`
  - `tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md`
  - `scripts/qemu-test.sh`
  - `docs/testing/index.md`
- **phase_now**: TASK-0018 Phase 4 complete; identity/report hardening + explicit no-artifact/mismatched metadata rejects documented in task/RFC/SSOT.
- **baseline_commit**: `cb706e4` (prep commit noted in task kickoff)
- **next_task_slice**:
  - perform final TASK-0018 gap/stop-condition check and prepare commit proposal,
  - open dedicated follow-up for removing the temporary child sender-id canonicalization once spawn-time child service identity is available,
  - keep crash follow-ons (`TASK-0048`/`TASK-0049`/`TASK-0141`/`TASK-0142`/`TASK-0227`) as separate scope.

## Last completed
- `TASK-0017` was archived for handoff continuity:
  - archive: `.cursor/handoff/archive/TASK-0017-dsoftbus-remote-statefs-rw.md`
  - status: in review with completed proofs and commits.

## Proof baseline currently green
- `cargo test -p crash -- --nocapture`
- `cargo test -p minidump-host -- --nocapture`
- `cargo test -p execd -- --nocapture`
- `just dep-gate`
- `just diag-os`
- `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`

## Active invariants (must hold)
- **security**
  - no secret leakage in markers/events/dumps,
  - reject malformed and oversized crashdump inputs deterministically,
  - no cross-process capture claims in v1.
- **determinism**
  - stable marker strings and bounded capture payloads,
  - no unbounded dump reads or implicit fallback paths.
- **scope hygiene**
  - keep v1 contract separate from v2 pipeline and export/bundle tasks.

## Open threads / follow-ups
- TASK-0018 follow-on boundaries (do not absorb now):
  - `tasks/TASK-0048-crashdump-v2a-host-pipeline-nxsym-nx-crash.md`
  - `tasks/TASK-0049-crashdump-v2b-os-crashd-retention-correlation-policy.md`
  - `tasks/TASK-0141-crash-v1-export-redaction-notify.md`
  - `tasks/TASK-0142-ui-problem-reporter-v1.md`
  - `tasks/TASK-0227-diagnostics-v1-bugreport-bundles-nx-diagnose-offline-deterministic.md`

## DON'T DO (session-local)
- DON'T reintroduce ptrace-like requirements into TASK-0018 v1 scope.
- DON'T claim symbolization is proven on OS in v1; keep that proof host-first.
- DON'T emit success markers before real artifact write + bounded validation.
- DON'T silently expand into `TASK-0048/0049/0141/0227` execution scope.
