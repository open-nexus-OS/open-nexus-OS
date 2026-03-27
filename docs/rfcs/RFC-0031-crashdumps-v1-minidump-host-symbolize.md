# RFC-0031: Crashdumps v1 - deterministic in-process minidumps + host symbolization

- Status: Complete
- Owners: @runtime @reliability @tools-team
- Created: 2026-03-26
- Last Updated: 2026-03-26
- Links:
  - Tasks: `tasks/TASK-0018-crashdumps-v1-minidump-host-symbolize.md` (execution + proof)
  - Related RFCs:
    - `docs/rfcs/RFC-0011-logd-journal-crash-v1.md`
    - `docs/rfcs/RFC-0018-statefs-journal-format-v1.md`
  - Related tasks:
    - `tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md`
    - `tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md`
    - `tasks/TASK-0048-crashdump-v2a-host-pipeline-nxsym-nx-crash.md`
    - `tasks/TASK-0049-crashdump-v2b-os-crashd-retention-correlation-policy.md`

## Status at a Glance

- **Phase 0 (v1 contract and host format proof)**: ✅
- **Phase 1 (OS in-process capture + event path)**: ✅
- **Phase 2 (hardening + follow-on handoff boundaries)**: ✅
- **Phase 3 (strict child-owned write proof + drift lock vs follow-ups)**: ✅
- **Phase 4 (final identity/report hardening + explicit negative E2E rejects)**: ✅

Definition:

- "Complete" means the crashdump v1 contract is defined and the task proof gates are green.

## Scope boundaries (anti-drift)

This RFC is a design seed and contract for crashdump v1.

- **This RFC owns**:
  - v1 crashdump contract: bounded in-process capture, deterministic dump path, host-first symbolization.
  - crash event contract extension in observability path (`build_id`, `dump_path` fields in the existing crash event envelope).
  - marker semantics for honest-green evidence.
- **This RFC does NOT own**:
  - ptrace-style cross-process post-mortem capture (kernel ABI work).
  - on-device DWARF symbolization.
  - crash retention/export UX and bundle orchestration (`TASK-0049`, `TASK-0141`, `TASK-0227`).
  - new dump container families beyond v1 scope.

### Relationship to tasks (single execution truth)

- `tasks/TASK-0018-crashdumps-v1-minidump-host-symbolize.md` defines stop conditions and proof commands.
- This RFC remains contract-authoritative when task and RFC contract text diverge.

## Context

Open Nexus OS needs deterministic crash artifacts without kernel changes. Current platform constraints do not allow `execd` to read dead child registers/stack reliably after process exit. v1 therefore relies on bounded in-process capture/publish from trusted runtime context and host-side symbolization while still emitting auditable crash events in the OS flow.

## Goals

- Define a minimal, deterministic v1 crashdump contract that works without kernel changes.
- Persist bounded artifacts under `/state/crash/...` and emit structured crash event metadata.
- Prove symbolization deterministically on host for matching build-id binaries.

## Non-Goals

- Cross-process register/memory capture after process death.
- On-device DWARF symbolization in v1.
- Full core dumps, unbounded stack snapshots, or large crash archives.

## Constraints / invariants (hard requirements)

- **Determinism**: fixed framing and bounded payload sizes; stable marker semantics.
- **No fake success**: `execd: minidump written` and `SELFTEST: minidump ok` only after real write success.
- **Bounded resources**: explicit limits for stack/code previews and total dump frame.
- **Security floor**: no secret logging, no untrusted path escapes, no untrusted identity source for crash events.
- **Kernel untouched**: all behavior remains in userspace services/libs/tools.

## Proposed design

### Contract / interface (normative)

- **Capture model**: in-process capture on controlled crash paths (panic/abort) writes a bounded v1 minidump artifact.
- **Storage model**: artifact path is normalized under `/state/crash/<ts>.<pid>.<name>.nmd`.
- **Event model**: `execd` crash event includes at least `event=crash.v1`, `pid`, `code`, `name`, and extends with `build_id`, `dump_path` when available.
- **Symbolization model**: host-side DWARF symbolization maps PCs from dump artifacts to `fn/file:line` for matching build-id binaries.
- **Versioning**: v1 dump format is version-tagged; incompatible extensions move to follow-on tasks/RFCs.

### Implemented v1 shape (2026-03-26)

- Deterministic dump framing implemented in `userspace/crash` (`NMD1`, bounded caps, reject-path tests).
- Deterministic path normalization implemented under `/state/crash/<ts>.<pid>.<name>.nmd`.
- Crash metadata publish path hardened in `execd`:
  - fail-closed unauthenticated publish reject,
  - crash event fields include `event=crash.v1`, `pid`, `code`, `name`, `recent_count`, `recent_window_nsec`,
  - `build_id`/`dump_path` included when available.
- Marker honesty hardening:
  - `selftest-client` verifies dump write via read-back + decode before asserting `SELFTEST: minidump ok`,
  - `execd` validates reported `build_id`/`dump_path` against decoded dump frame bytes before emitting `execd: minidump written`.
- QEMU marker ladder includes:
  - `execd: minidump written`
  - `SELFTEST: minidump ok`
  - `SELFTEST: minidump no-artifact metadata rejected`
  - `SELFTEST: minidump mismatched build_id rejected`
- Host symbolization proof implemented in `tools/minidump-host` (`PC -> fn/file:line`).

### Phases / milestones (contract-level)

- **Phase 0**: lock v1 artifact/event contract and host deterministic format checks.
- **Phase 1**: implement OS in-process capture + event path + honest-green markers.
- **Phase 2**: harden reject paths and freeze handoff boundaries to v2 tasks.
- **Phase 3**: prove strict child-owned dump write path and re-check follow-up boundaries for no-drift.
- **Phase 4**: harden identity/report verification with explicit policy/verify helpers and fail-closed negative E2E coverage.

### Phase 4 drift lock (2026-03-26)

Before finalizing Phase 3, follow-up task contracts were cross-checked:

- `TASK-0048`: no host-v2 symbol/index/container scope absorbed.
- `TASK-0049`: no `crashd`/retention/OS symbolization scope absorbed.
- `TASK-0141`: no export/redaction/notification API scope absorbed.
- `TASK-0142`: no Problem Reporter UI scope absorbed.
- `TASK-0227`: no offline bundle orchestration scope absorbed.

This keeps RFC-0031 bounded as a v1 contract and avoids silent migration into follow-up backlogs.

## Security considerations

- **Threat model**:
  - Sensitive data exposure from stack/code previews.
  - Crash metadata spoofing or malformed publish payloads.
  - Resource exhaustion via oversized dump inputs.
- **Mitigations**:
  - strict size caps and reject-on-overflow behavior,
  - normalized path constraints under `/state/crash`,
  - trusted metadata sourcing (runtime/service identity only),
  - explicit negative tests for malformed and oversize paths.
- **Open risks**:
  - page-fault style uncontrolled crash capture remains limited without kernel support.
  - redaction/export policy integration is deferred to follow-on tasks.

## Failure model (normative)

- Oversized or malformed dump payloads are rejected deterministically.
- Invalid dump paths (escape or non-`/state/crash` destination) are rejected deterministically.
- Missing artifact write means no success marker is emitted.
- Invalid or unverifiable reported minidump metadata is rejected fail-closed and does not emit success markers.
- If crash event publication fails, service emits failure status deterministically and does not claim success.
- No silent fallback from bounded format to unbounded dumps.

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p crash -- --nocapture
cd /home/jenning/open-nexus-OS && cargo test -p minidump-host -- --nocapture
cd /home/jenning/open-nexus-OS && cargo test -p execd -- --nocapture
```

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh
```

### Deterministic markers

- `execd: minidump written`
- `SELFTEST: minidump ok`

## Alternatives considered

- `execd` post-mortem child capture: rejected for v1 (requires new kernel debug ABI).
- On-device DWARF symbolization: rejected for v1 (cost and complexity too high for early OS slice).

## Open questions

- Whether v2 should require strict child-owned dump writes for all crash paths or keep trusted publish mediation for constrained paths.

## Implementation Checklist

**This section tracks implementation progress. Update as phases complete.**

- [x] **Phase 0**: Freeze v1 dump/event contract and host deterministic checks — proof: `cargo test -p crash -- --nocapture` and `cargo test -p minidump-host -- --nocapture`
- [x] **Phase 1**: OS in-process capture + crash event path + honest-green markers — proof: `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
- [x] **Phase 2**: Reject-path hardening + follow-on boundary lock — proof: `cargo test -p execd -- --nocapture` and required `test_reject_*` coverage
- [x] **Phase 3**: Strict child-owned write proof + follow-up drift re-check — proof: `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh` with `child: minidump start`, `execd: minidump written`, `SELFTEST: minidump ok`, `SELFTEST: minidump forged metadata rejected`
- [x] **Phase 4**: Identity/report hardening + explicit negative E2E rejects — proof: `cargo test -p statefsd -- --nocapture`, `cargo test -p execd -- --nocapture`, and `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh` with `SELFTEST: minidump no-artifact metadata rejected` and `SELFTEST: minidump mismatched build_id rejected`
- [x] Task(s) linked with stop conditions + proof commands.
- [x] QEMU markers (if any) appear in `scripts/qemu-test.sh` and pass.
- [x] Security-relevant negative tests exist (`test_reject_*`).
