# Current Handoff: TASK-0029 (Supply-Chain v1) — Closure Remediation

**Date**: 2026-04-22
**Active task**: `tasks/TASK-0029-supply-chain-v1-sbom-repro-sign-policy.md` — `In Review`
**Contract seed (RFC)**: `docs/rfcs/RFC-0039-supply-chain-v1-bundle-sbom-repro-sign-policy.md` — `Done`
**Tier**: `production-grade` BASELINE for the Updates / Packaging / Recovery group (per `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`). Full closure of that tier is reached only at `TASK-0197 + TASK-0198 + TASK-0289`; v1 must stay on that trajectory without locking the wrong contract (10 explicit hard gates in TASK-0029 §"Production-grade tier").

## Status — implemented vs. remaining closure deltas

### Implemented (green evidence already present)

- C-01..C-08 implementation landed across `tools/{sbom,repro,nxb-pack}`, `keystored`, `policyd`, `bundlemgrd`, proof-manifest markers/profiles, docs, and status sync files.
- Host proof commands are green for SBOM/repro/enforcement/reject suites.
- QEMU profile run `just test-os supply-chain` is green with `verify-uart` clean.
- `just dep-gate`, `just diag-os`, and `just diag-host` are green.

### Remaining closure deltas (task-level)

- No RFC-level closure delta remains (`RFC-0039` is `Done`).
- `TASK-0029` remains intentionally at `In Review` pending explicit task-level finalization.

## Immediate execution order (current chat)

1. Keep RFC/task/checklist text synchronized (`RFC-0039` = `Done`, `TASK-0029` = `In Review`).
2. Fix contract deltas in code (manifest digests, authority boundary, sender identity, bounded inputs).
3. Clear full quality gate set (`dep-gate`, `diag-os`, `diag-host`, `fmt-check`, `lint`, `arch-gate`).
4. Re-run canonical host + QEMU proofs and capture clean evidence pointers.
5. Re-issue closure delta report and only then consider `Done/Complete` flips.

## Working-tree state at handoff

- `M .cursor/current_state.md`
- `M .cursor/handoff/current.md`
- `M .cursor/next_task_prep.md`
- `M docs/rfcs/RFC-0038-...md` (Phase-6 status sync from prior commit)
- `M tasks/TASK-0023B-...md` (status flip prep)
- `M uart.log` (test artifact only; **do not commit** outside a task closure — `.gitignore` documents the policy)
- New (uncommitted, prepared this session):
  - `tasks/TASK-0029-supply-chain-v1-sbom-repro-sign-policy.md` (audited / extended)
  - `docs/rfcs/RFC-0039-supply-chain-v1-bundle-sbom-repro-sign-policy.md` (new)
  - `docs/rfcs/README.md` (RFC-0039 index entry)
  - `.cursor/handoff/archive/TASK-0023B-...md` (snapshot copy of prior `current.md`)

## Phase-6 evidence kept on disk (do not delete)

- `.cursor/replay-dev-a.json` — native dev replay, `trace_diff.status == exact_match`.
- `.cursor/replay-ci-like.json` — containerized CI-like replay, `trace_diff.status == exact_match`.
- `.cursor/replay-synthetic-bad.json` — synthetic tamper, exit 1, `missing_marker[0].marker == "SYNTHETIC: tamper probe"`.
- `.cursor/bisect-good-drift-regress.json` — 3-commit smoke, `first_bad_commit: c2cccccc`, `drift_commits: [c1bbbbbb]`.

These four JSONs are the Phase-6 proof-floor evidence cited from `RFC-0038` and `tasks/TASK-0023B-...`. They stay until the external CI artifact lands and the closure mirror commit ships.
