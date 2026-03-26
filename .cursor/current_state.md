# Cursor Current State (SSOT)

<!--
CONTEXT
This file is the single source of truth for the current system state.
Keep it compact, explicit, and contract-oriented.
-->

## Current architecture state
- **last_decision**: move execution focus from `TASK-0017` closeout to `TASK-0018` contract preparation; harden task scope first, then seed RFC-0031 before implementation.
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
- **active_task**: `tasks/TASK-0018-crashdumps-v1-minidump-host-symbolize.md` (Draft, hardened)
- **seed_contract**:
  - `tasks/TASK-0018-crashdumps-v1-minidump-host-symbolize.md`
  - `docs/rfcs/RFC-0031-crashdumps-v1-minidump-host-symbolize.md` (Draft)
  - `docs/rfcs/RFC-0011-logd-journal-crash-v1.md`
  - `docs/rfcs/RFC-0018-statefs-journal-format-v1.md`
- **contract_dependencies**:
  - `tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md`
  - `tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md`
  - `scripts/qemu-test.sh`
  - `docs/testing/index.md`
- **phase_now**: task hardening complete, RFC seed created, SSOT switched to TASK-0018 prep
- **baseline_commit**: `01287ac` (latest pre-TASK-0018 setup commit)
- **next_task_slice**:
  - finalize plan/proof mapping for TASK-0018,
  - start implementation only inside TASK-0018 touched-path allowlist.

## Last completed
- `TASK-0017` was archived for handoff continuity:
  - archive: `.cursor/handoff/archive/TASK-0017-dsoftbus-remote-statefs-rw.md`
  - status: in review with completed proofs and commits.

## Proof baseline currently green
- `just test-all`
- `just dep-gate`
- `just diag-os`
- `just test-dsoftbus-2vm`
- `make initial-setup`

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
