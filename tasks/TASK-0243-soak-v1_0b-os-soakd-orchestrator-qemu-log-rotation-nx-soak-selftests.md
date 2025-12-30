---
title: TASK-0243 Soak & Flake-Hunter v1.0b (OS/QEMU): soakd orchestrator + qemu.log rotation extension + nx-soak CLI + selftests
status: Draft
owner: @runtime
created: 2025-12-29
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Soak core (host-first): tasks/TASK-0242-soak-v1_0a-host-repro-recorder-deterministic-retry-gates.md
  - Testing contract: scripts/qemu-test.sh
  - State persistence (/state): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
---

## Context

We need OS/QEMU orchestration for Soak & Flake-Hunter v1.0:

- `soakd` orchestrator for long-run test execution,
- qemu.log rotation extension (builds on existing `trim_log`),
- `nx soak` CLI for start/status/export,
- convenience `just` recipes.

The prompt proposes `soakd` as a new service. Existing scripts (`scripts/qemu-test.sh`, `scripts/run-qemu-rv64.sh`) already have `trim_log` for post-run trimming. This task extends that with **rotation during run** and integrates with `soakd` orchestration.

## Goal

On OS/QEMU:

1. **soakd service** (`source/services/soakd/`):
   - generates deterministic stream of cases from fixed PRNG seeded by plan seed
   - executes cases through repro-recorder (from `TASK-0242`)
   - on failure, performs deterministic retries up to `retry_flake.attempts`, nudging seed by `seed_delta`
   - marks case as **flake** if any retry passes
   - writes run summary `state:/soak/summary/<run>.json`
   - API (`soak.capnp`): `start`, `status`, `list`, `last`, `export`
   - markers: `soakd: ready`, `soakd: start run=<id>`, `soakd: case id=… result=pass|fail|flake retries=n`, `soakd: export bytes=…`
2. **qemu.log rotation extension**:
   - extend `scripts/qemu-test.sh` and `scripts/run-qemu-rv64.sh`:
     - enforce file size budget from schema (`qemu_bytes`); on exceed, **rotate** to `uart.log.1`, keep `rotate_keep` generations
     - emit marker `runner: uart rotated` on rotation
   - add config knob `SOAK_UART_VERBOSE=0|1` to reduce noise during soak
   - markers: `runner: uart budget reached size=…`, `runner: uart rotated gen=…`
3. **nx soak CLI** (subcommand of `nx`):
   - `start --minutes 90 --max-cases 80 --seed 1337`
   - `status [--last]`
   - `export --last --out state:/exports/soak_<ts>.tgz`
   - `quick` (alias: 15 min, 20 cases for PRs)
   - markers: `nx: soak start run=<id>`, `nx: soak export out=…`
4. **Convenience recipes** (`justfile`):
   - `just soak` (default: 60 min, 128 cases)
   - `just soak-quick` (15 min, 20 cases)
5. **OS selftests + postflight**.

## Non-Goals

- Kernel changes.
- Real network/Store backend (offline fixtures only).

## Constraints / invariants (hard requirements)

- **No duplicate test orchestrator**: `soakd` is the single authority for long-run test orchestration. Do not create parallel test runners.
- **Determinism**: case generation, retry logic, and rotation must be stable given the same inputs.
- **Bounded resources**: log rotation enforces size budgets; recordings are size-bounded.
- **`/state` gating**: persistence is only real when `TASK-0009` exists.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points

- **YELLOW (existing log rotation)**:
  - Existing scripts already have `trim_log` for post-run trimming. This task extends it with **rotation during run** (not just post-run). Document the difference explicitly.
- **YELLOW (soakd vs existing test harness)**:
  - `soakd` orchestrates long-run tests, while `scripts/qemu-test.sh` handles single-run selftests. Document the relationship explicitly.

## Contract sources (single source of truth)

- QEMU marker contract: `scripts/qemu-test.sh`
- Soak core: `TASK-0242`
- Existing log rotation: `scripts/qemu-test.sh` (trim_log)

## Stop conditions (Definition of Done)

### Proof (OS/QEMU) — gated

UART markers:

- `soakd: ready`
- `soakd: start run=<id>`
- `soakd: case id=… result=pass|fail|flake retries=n`
- `soakd: export bytes=…`
- `runner: uart budget reached size=…`
- `runner: uart rotated gen=…`
- `SELFTEST: soak quick run ok`
- `SELFTEST: soak export ok`
- `SELFTEST: soak uart rotate ok`

## Touched paths (allowlist)

- `source/services/soakd/` (new)
- `scripts/qemu-test.sh` (extend: rotation during run)
- `scripts/run-qemu-rv64.sh` (extend: rotation during run)
- `tools/nx/` (extend: `nx soak ...` subcommands)
- `justfile` (extend: `soak`, `soak-quick` recipes)
- `source/apps/selftest-client/` (markers)
- `docs/soak/overview.md` (new)
- `docs/tools/nx-soak.md` (new)
- `tools/postflight-soak-v1_0.sh` (new)

## Plan (small PRs)

1. **soakd service**
   - case generation from PRNG
   - execution loop with repro-recorder integration
   - retry gates + flake detection
   - run summary + export
   - markers

2. **qemu.log rotation extension**
   - extend existing `trim_log` with rotation during run
   - size budget enforcement
   - verbosity knob
   - markers

3. **nx soak CLI + just recipes**
   - CLI: start/status/export/quick
   - just recipes
   - markers

4. **OS selftests + postflight**
   - quick soak test
   - export test
   - rotation test
   - postflight

## Acceptance criteria (behavioral)

- `soakd` orchestrates long-run tests correctly.
- qemu.log rotation works during run (not just post-run).
- `nx soak` CLI works correctly.
- All three OS selftest markers are emitted.
