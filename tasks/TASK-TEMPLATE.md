---
title: TASK-XXXX Short title (big step)
status: Draft
owner: @runtime
created: YYYY-MM-DD
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  # Optional canonical contracts (fill as appropriate):
  # - RFC: docs/rfcs/RFC-0005-kernel-ipc-capability-model.md
  # - ADR: docs/adr/0001-runtime-roles-and-boundaries.md
---

## Context

- Why do we need this now?
- What is broken/missing today (concrete symptoms, markers, failing tests)?
- What *must not* change (boundaries/ABI/marker contracts)?

## Goal

- Define the *one* user-visible outcome this task must produce.
- Keep it phrased as behavior, not implementation details.

## Non-Goals

- Explicitly list what is out of scope (even if “obvious”).

## Constraints / invariants (hard requirements)

- **No fake success**: no `*: ready` / `SELFTEST: * ok` markers unless the real behavior happened.
- **Stubs are explicit**: stub paths must return deterministic `Unsupported/Placeholder` errors or emit markers containing `stub`/`placeholder` (never “ok/ready”).
- **Determinism**: markers must be stable strings; no timestamp/randomness in proof signals.
- **Security boundaries**: kernel stays minimal; policy/IDL/crypto/distributed stays in userland services.
- **Rust hygiene**: no `unwrap/expect` in daemons; avoid new `unsafe` (and justify if unavoidable).

## Red flags / decision points (track explicitly)

Use this section to surface anything that is **critical**, **blocking**, or requires an explicit decision.
Keep it short and actionable.

- **RED (blocking / must decide now)**:
  - …
- **YELLOW (risky / likely drift / needs follow-up)**:
  - …
- **GREEN (confirmed assumptions)**:
  - …

## Contract sources (single source of truth)

List the canonical files that define the contract for this change. The task must *reference* these
contracts, not duplicate them in prose.

- **QEMU marker contract**: `scripts/qemu-test.sh` (required markers + order checks)
- **ABI/layout contract** (if applicable): `source/libs/nexus-abi` layout tests + golden vectors
- **Kernel selftest contract** (if applicable): `source/kernel/neuron/src/selftest/*` markers

## Stop conditions (Definition of Done)

These are the only acceptable “done” signals for this task.

- **Proof (QEMU)**:
  - Command(s):
    - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
  - Required markers (must exist in `scripts/qemu-test.sh` expected list):
    - `SELFTEST: ...`
    - `KSELFTEST: ...` (if kernel work)
- **Proof (tests)**:
  - Command(s):
    - `cargo test -p <crate>`
  - Required tests:
    - `<test_name>`

Notes:

- **Postflight scripts are not proof** unless they only delegate to the canonical harness/tests and do
  not define their own “OK” semantics.

## Touched paths (allowlist)

Only these paths may be modified without opening a separate task/ADR:

- `source/...`
- `userspace/...`
- `scripts/...`
- `docs/...`

## Plan (small PRs)

Break the big step into reviewable slices. Each slice must end in a real proof.

1. ...
2. ...

## Acceptance criteria (behavioral)

Use bullet points that can be verified by markers/tests without interpretation.

- ...

## Evidence (to paste into PR)

- QEMU: attach `uart.log` tail showing the relevant markers (and the command used)
- Tests: paste the exact `cargo test ...` output summary

## RFC seeds (for later, when the step is complete)

Short bullets that will be turned into the RFC snapshot later (avoid long design prose here).

- Decisions made:
  - ...
- Open questions:
  - ...
- Stabilized contracts:
  - Link to exact tests/markers that enforce the contract
