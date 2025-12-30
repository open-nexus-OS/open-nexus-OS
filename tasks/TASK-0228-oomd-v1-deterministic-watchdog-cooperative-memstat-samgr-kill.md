---
title: TASK-0228 OOM watchdog v1 (OS): deterministic memory watchdog (`oomd`) with cooperative memstat + samgr/execd kill + selftests
status: Draft
owner: @runtime @reliability
created: 2025-12-29
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Observability (logd): tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md
  - Execd (spawner/supervision): tasks/TASK-0001-runtime-roles-and-boundaries.md
  - Crash pipeline (optional correlation): tasks/TASK-0049-crashdump-v2b-os-crashd-retention-correlation-policy.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

We need a deterministic, QEMU-proof way to detect and mitigate memory blow-ups without kernel changes.

The prompt suggests “RSS polling from proc metadata”. Today we do **not** have a stable kernel ABI for
per-process RSS export, and adding one would be kernel work (out of scope for this v1).

So v1 must be **cooperative and bounded**:

- processes/services publish a small, deterministic “memstat” signal (bytes allocated in the process allocator),
- `oomd` watches these signals and requests termination through the existing process authority (execd/samgr integration),
- events are logged to `logd` and selftests prove the behavior.

## Goal

On OS/QEMU:

- Provide an `oomd` service that enforces per-subject memory limits deterministically using a cooperative memstat feed.
- Provide a deterministic, test-only injection path to trigger an OOM kill in QEMU.
- Emit stable UART markers and `logd` events for audits/debugging.

## Non-Goals

- Kernel changes (no RSS syscall/procfs required for v1).
- Global system OOM handling (kernel OOM killer, page reclaim, cgroup-like policies).
- “Perfect” memory attribution across shared mappings.
- Any feature that can only be proven via flaky timing (no “it usually kills within 5s”).

## Constraints / invariants (hard requirements)

- **Determinism**:
  - sampling uses a deterministic tick source (or a bounded fixed polling loop), not wall-clock timestamps in markers,
  - decisions are deterministic given the same memstat sequence.
- **Bounded overhead**:
  - `oomd` keeps bounded in-memory event history (ring buffer),
  - memstat payload is bounded and rate-limited.
- **Single authority for kills**:
  - `oomd` does not directly “kill by PID” unless there is a canonical ABI for it.
  - It must request termination via the canonical process authority (execd or samgr-managed lifecycle).
- **No fake success**:
  - `oomd: kill ...` marker only after the termination request is acknowledged and the process is observed to exit.

## Red flags / decision points

- **RED (missing lifecycle API)**:
  - If there is no usable “terminate service/app” control path, this task must first create it (separate subtask),
    rather than inventing a second kill authority.
- **YELLOW (memstat truth)**:
  - Cooperative memstat is not true RSS; document this explicitly and ensure policy language does not claim otherwise.
  - A future “real RSS” path requires a kernel ABI task.

## Contract sources (single source of truth)

- QEMU marker contract: `scripts/qemu-test.sh`
- Process authority boundaries: `TASK-0001`
- log event sink: `TASK-0006`

## Stop conditions (Definition of Done)

### Proof (Host) — required

- `cargo test -p oomd_host` green (new)
  - Deterministic state machine tests:
    - exceeding `hardBytes` triggers a kill decision once,
    - event ordering stable,
    - rate limiting stable.

### Proof (OS/QEMU)

- `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=120s ./scripts/qemu-test.sh`
- Required markers:
  - `oomd: ready`
  - `SELFTEST: oom kill ok`

## API sketch (v1)

We can document an IDL (Cap’n Proto) for developer clarity, but the **authoritative OS contract**
must remain compatible with OS-lite transport constraints (small, versioned frames) unless there is
an explicit repo-wide decision to make Cap’n Proto the on-wire contract.

Minimum operations:

- **set_policy**: `(softBytes, hardBytes, sampleMs)` (bounded fields)
- **events**: `(sinceSeq, limit)` → bounded list
- **inject (test-only)**: `(appId, bytes)` → triggers a deterministic hog path

## Selftest strategy (QEMU-proof)

- Add a minimal “hog” app/service that:
  - publishes memstat growth deterministically (bounded arena allocations),
  - does not rely on allocator randomness.
- Selftest flow:
  - set a small `hardBytes`,
  - invoke `inject` (or launch hog app),
  - observe:
    - `oomd: kill app=<...> bytes=<...>` marker,
    - the target exits,
    - `SELFTEST: oom kill ok`.

## Touched paths (allowlist)

- `source/services/oomd/` (new)
- `userspace/libs/` (new: tiny memstat publisher helper; must be optional and bounded)
- `source/apps/selftest-client/` (drive test + marker)
- `userspace/apps/` (hog fixture)
- `scripts/qemu-test.sh`
- `docs/reliability/` (OOM policy + limitations)
- `tests/oomd_host/` (new)

## Plan (small PRs)

1. Define the v1 memstat signal shape (cooperative, bounded).
2. Implement `oomd` host-first logic + tests (state machine).
3. Implement OS service + deterministic selftest hog fixture.
4. Wire termination request through the canonical process authority; prove no duplicate “kill authority”.
5. Docs: what v1 guarantees vs what requires kernel RSS ABI later.

## Acceptance criteria (behavioral)

- `oomd` deterministically terminates a misbehaving subject using cooperative memstat, with stable markers and bounded history.
- Selftests prove the kill path in QEMU without relying on timing flukes.
