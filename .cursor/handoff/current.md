# Current Handoff: TASK-0045 done (nx CLI v1)

**Date**: 2026-04-24  
**Active execution task**: `tasks/TASK-0045-devx-nx-cli-v1.md` — `Done`  
**Contract seed**: `docs/rfcs/RFC-0043-devx-nx-cli-v1-host-first-production-floor-seed.md` — `Done`  
**Tier policy**: `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md` (Gate J: DevX, Config & Tooling, `production-floor`)

## Baseline status

- `TASK-0039`/`RFC-0042` closure is complete and archived.
- Queue focus has moved to `TASK-0045` as single execution SSOT for nx CLI v1.
- `RFC-0043` is done and linked from task/index.

## TASK-0045 target behavior

- One canonical host CLI entrypoint at `tools/nx`.
- Deterministic command behavior and exit-code contract.
- Fail-closed security posture for input/path/topic handling.
- No fake-success outputs; delegated command exit code remains authoritative.

## Initial gate matrix (Go / No-Go)

- **Gate A (canonical entrypoint floor)**: GREEN
  - `tools/nx` crate exists with stable subcommand registry and dispatch.
- **Gate B (security fail-closed floor)**: GREEN
  - traversal/absolute-path rejects and unknown-topic rejects are implemented + tested.
- **Gate C (proof quality floor)**: GREEN
  - process-level CLI contract tests now assert exit classes + structured JSON + file effects.
- **Gate D (no-drift extension floor)**: GREEN
  - future topics (`nx config/policy/crash/sdk/diagnose/sec`) can extend in-place without `nx-*` binary drift.
- **Gate E (dsl wrapper floor)**: GREEN
  - `nx dsl fmt|lint|build` delegates deterministically or fails as explicit unsupported.

## Planned proof floor (host-first)

- Primary proof command:
  - `cd /home/jenning/open-nexus-OS && cargo test -p nx -- --nocapture`
- Latest run:
  - `cargo test -p nx -- --nocapture` -> 15 passed, 0 failed
- Runtime proof evidence:
  - `cargo run -q -p nx -- new service ../escape --json` returns structured JSON envelope with exit `3`.
- Required reject focus:
  - `new`: traversal + absolute path rejection
  - `postflight`: unknown-topic rejection + delegated failure passthrough
  - `doctor`: missing dependency classification + non-zero exit

## Guardrails

- Keep scope host-only for v1; no OS runtime claim and no QEMU closure requirement.
- Do not absorb follow-up semantics from `TASK-0046`, `TASK-0047`, `TASK-0048`, `TASK-0163+`.
- Preserve deterministic outputs (no random success markers, no grep-only closure evidence).
- Closure deltas resolved:
  - `--json` reject-path output behavior fixed,
  - process-level exit/output assertions added,
  - scaffolding templates now include CONTEXT headers.
