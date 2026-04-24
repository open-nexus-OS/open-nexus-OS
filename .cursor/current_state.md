# Cursor Current State (SSOT)

## Current architecture state

- **last_decision (2026-04-24)**: `TASK-0039` closed as done; active execution focus moved to `TASK-0045`.
- **active boundary**: `TASK-0045` is host-first DevX tooling only (no kernel/runtime behavior changes).
- **gate tier**: Gate J (`production-floor`) per `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`.

## Active focus (execution)

- **active_task**: `tasks/TASK-0045-devx-nx-cli-v1.md` — `Draft`
- **active_contract**: `docs/rfcs/RFC-0043-devx-nx-cli-v1-host-first-production-floor-seed.md` — `Draft`
- **active_proof_command (target)**:
  - `cd /home/jenning/open-nexus-OS && cargo test -p nx -- --nocapture`

## Active constraints (TASK-0045)

- Single canonical entrypoint must be `tools/nx` (no `nx-*` binary drift).
- No fake success: delegated subprocess exit code is authoritative.
- Security fail-closed on untrusted input:
  - reject traversal/absolute write targets in scaffolding,
  - reject unknown `postflight` topics,
  - never construct shell commands from user-controlled strings.
- Deterministic output contract:
  - stable exit-code classes,
  - stable structured output (`--json`) and bounded tails/log snippets.
- v1 does not auto-edit workspace manifests; print deterministic manual follow-up instructions.

## Execution gates (TASK-0045)

- **Gate A (CLI baseline + deterministic UX)**: PENDING
  - `tools/nx` exists and command surface is stable (`new`, `inspect`, `idl`, `postflight`, `doctor`, `dsl`).
- **Gate B (security fail-closed)**: PENDING
  - reject-path tests for traversal/absolute path and unknown topics exist + pass.
- **Gate C (proof quality)**: PENDING
  - tests assert Soll behavior (exit code, filesystem effects, structured output), not log-marker greps.
- **Gate D (extension/no-drift)**: PENDING
  - follow-up tasks extend as subcommands under `tools/nx` without separate CLIs.
- **Gate E (dsl wrapper floor)**: PENDING
  - deterministic delegate or explicit unsupported classification.

## Required reject proofs (minimum floor)

- `new`: rejects path traversal and absolute path escapes.
- `postflight`: rejects unknown topics and propagates delegate failure.
- `doctor`: returns non-zero when required tools are missing.
- `dsl`: fails closed when backend absent and never reports false success.

## Follow-up split (preserve scope)

- `TASK-0046`: config semantics / `nx config`.
- `TASK-0047`: policy engine semantics / `nx policy`.
- `TASK-0048`: crash pipeline / `nx crash`.
- `TASK-0163`: IDL codegen policy and canonical outputs.
- `TASK-0164`, `TASK-0165`, `TASK-0227`, `TASK-0230`, `TASK-0268`: extension tracks under the same CLI.

## Carry-over note

- `TASK-0039` and `RFC-0042` are closed and archived; no reopen implied by `TASK-0045` work.
