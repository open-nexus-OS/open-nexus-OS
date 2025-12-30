---
title: TASK-0242 Soak & Flake-Hunter v1.0a (host-first): repro recorder + deterministic retry gates + case catalog + tests
status: Draft
owner: @runtime
created: 2025-12-29
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We need a deterministic long-run stability framework to catch flakes and generate reproducible recordings:

- repro recorder that captures inputs/markers/config,
- deterministic retry gates for flaky tests,
- case catalog with deterministic test mix.

The prompt proposes a repro-recorder and retry gates. This task delivers the **host-first core** (recorder library, retry logic, case catalog) before OS/QEMU orchestration.

## Goal

Deliver on host:

1. **Repro recorder library** (`userspace/libs/reprorec/`):
   - captures for each case:
     - **inputs**: executed commands with exact args/env, random seed, timestamps
     - **outputs**: selected service markers (filter via schema.markers), stdout/stderr of CLI, diff of relevant state paths
     - **system**: git rev, build id, schema snapshots
   - layout (per case): `case_<seq>_<id>/input.json`, `stdout.txt`, `stderr.txt`, `markers.ndjson`, `state/` (prefs, procs, ability states)
   - deterministic gzip (mtime/uid/gid fixed) when exporting TAR.GZ
   - markers: `reprorec: record path=… bytes=…`
2. **Deterministic retry gates**:
   - retry logic: up to `retry_flake.attempts` (default 3), nudging seed by `seed_delta` (default 7)
   - mark case as **flake** if any retry passes
   - deterministic PRNG seeding for retries
3. **Case catalog** (`pkg://fixtures/soak/cases.json`):
   - weighted cases with parametrized seeds:
     - `ability_fg_bg_flip` (uses `nx ability fg/bg` repeatedly)
     - `notif_burst_then_clear` (uses `nx notif send …` then cancel/clear)
     - `settings_toggle_locale` (switches locale EN↔DE)
     - `content_rw_cycle` (create/overwrite/delete with quota near threshold)
     - `privacy_prompt_once_then_while` (camera/mic request flow)
     - `power_wakelock_pulse` (acquire/release)
     - `update_install_then_rollback` (install v2 then deliberate rollback)
   - deterministic case generation from fixed PRNG seed
4. **Host tests** proving:
   - repro recorder captures expected files and stable hashes
   - retry gate correctly flags flakes (craft flaky demo case)
   - case catalog produces deterministic case sequences
   - export tar.gz produces deterministic gzip headers

## Non-Goals

- OS/QEMU orchestration (deferred to v1.0b).
- qemu.log rotation (handled by existing scripts; see v1.0b for extension).

## Constraints / invariants (hard requirements)

- **Determinism**: repro recordings, retry logic, and case generation must be stable given the same inputs.
- **Bounded resources**: recordings are size-bounded; case catalog is bounded.
- **Reproducible exports**: TAR.GZ exports must be deterministic (fixed mtime/uid/gid).
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **YELLOW (recording size)**:
  - Recordings can grow large. Ensure size budgets and compression are effective.

## Contract sources (single source of truth)

- Testing contract: `scripts/qemu-test.sh`

## Stop conditions (Definition of Done)

### Proof (Host) — required

`cargo test -p soak_v1_0_host` green (new):

- repro recorder: verify expected files present and stable hashes
- retry gate: craft flaky demo case (fails when seed%3==0); ensure at least one retry passes → marked `flake=true`
- case catalog: with fixed seed, first 10 cases are identical across runs (hash compare)
- export: export tar.gz for last run; size within expected envelope; gzip headers deterministic

## Touched paths (allowlist)

- `userspace/libs/reprorec/` (new)
- `pkg://fixtures/soak/cases.json` (new)
- `schemas/soak_v1_0.schema.json` (new)
- `tests/soak_v1_0_host/` (new)
- `docs/soak/recorder.md` (new, host-first sections)

## Plan (small PRs)

1. **Repro recorder library**
   - capture inputs/outputs/system state
   - deterministic export (TAR.GZ with fixed mtime/uid/gid)
   - host tests

2. **Retry gates + case catalog**
   - deterministic retry logic
   - case catalog with weighted cases
   - host tests

3. **Schema + docs**
   - `schemas/soak_v1_0.schema.json`
   - host-first docs

## Acceptance criteria (behavioral)

- Repro recorder captures expected files and stable hashes.
- Retry gate correctly flags flakes.
- Case catalog produces deterministic case sequences.
- Export produces deterministic TAR.GZ files.
