---
title: TASK-0269 Boot gates v1 (OS/QEMU): readiness contract + spawn failure reasons + resource/leak sentinel
status: Complete
owner: @runtime @kernel-team @tools-team
created: 2026-01-16
links:
  - RFC: docs/rfcs/RFC-0013-boot-gates-readiness-spawn-resource-v1.md
  - Logging discipline: docs/rfcs/RFC-0003-unified-logging.md
  - IPC/caps contract: docs/rfcs/RFC-0005-kernel-ipc-capability-model.md
  - OS dependency hygiene: docs/rfcs/RFC-0009-no-std-dependency-hygiene-v1.md
  - logd readiness contract: docs/rfcs/RFC-0011-logd-journal-crash-v1.md
  - Testing methodology: docs/testing/index.md
  - QEMU marker contract: scripts/qemu-test.sh
  - Related: tasks/TASK-0228-oomd-v1-deterministic-watchdog-cooperative-memstat-samgr-kill.md
---

## Context

We need deterministic, early gates that turn “mysterious boot regressions” into actionable failures:

- `init: up <svc>` observed, but `<svc>: ready` never arrives (partial init / hang).
- `spawn` fails late (`abi:spawn-failed`) without a stable reason classification (OOM vs cap quota vs endpoint quota).
- Resource leaks (caps/endpoints) only surface after more services are added.

This task executes RFC-0013 and provides the proof gates.

## Scope / slices (aligned to RFC-0013)

### Slice A (Readiness gate contract)

- Ensure docs and harness semantics distinguish `init: up <svc>` from `<svc>: ready`.
- Make missing readiness produce an explicit and stable failure output in the QEMU harness.

### Slice B (Spawn failure reasons)

- Implement SpawnFailReason v1 taxonomy (RFC-0013) in the kernel spawn path.
- Surface reason token deterministically (markers + error mapping).
- Add a kernel selftest proving at least the minimum reason set is reachable and correctly classified.

### Slice C (Resource/leak sentinel)

- Add a deterministic, bounded sentinel test that exercises:
  - cap lifecycle churn (clone/transfer/close),
  - endpoint quota pressure and release,
  - spawn pressure + cleanup,
  - and asserts “no leaks across failure paths”.
- Gate it via a stable marker (RFC-0013).

## Non-goals

- Implementing a production OOM killer or kernel RSS accounting (see TASK-0228).
- Broad scheduler changes.

## Constraints / invariants

- Deterministic markers, bounded loops, no fake success.
- Do not log secrets; do not emit raw capability values in markers.

## Touched paths (allowlist)

- `docs/testing/index.md` (doc alignment for readiness semantics)
- `scripts/qemu-test.sh` (failure output clarity and gate coupling)
- `source/kernel/neuron/**` (spawn reason taxonomy + selftests)
- `source/libs/nexus-abi/**` (error mapping, if required by taxonomy)

## Stop conditions (Definition of Done)

### Stop conditions: Slice A (Readiness)

- OS/QEMU harness failure on missing readiness is explicit and attributable:
  - `init: up logd` but missing `logd: ready` fails with a stable message that names the missing marker.
- Documentation clearly states that `init: up` is not “ready”.

### Stop conditions: Slice B (Spawn failure reasons)

- Kernel provides stable SpawnFailReason v1 classification for spawn failures.
- QEMU kernel selftest markers prove:
  - each minimum reason token is reachable and correctly reported (exact marker names are stable and listed in RFC-0013).

### Stop conditions: Slice C (Resource/leak sentinel)

- Deterministic sentinel marker exists and is green on QEMU:
  - `KSELFTEST: resource sentinel ok`
- Sentinel failures emit a stable first-failure marker with a reason token.

## Proof commands (canonical)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && just diag-host
```

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && just dep-gate
cd /home/jenning/open-nexus-OS && just diag-os
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os
```

## Notes (anti-drift)

- RFC-0013 is the contract; this task holds progress and proof commands.
- Any expansion beyond readiness/spawn reasons/resource sentinel requires a follow-up RFC (don’t extend RFC-0013 into a backlog).
