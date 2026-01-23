<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# TASK-0285: RFC-0014 Phase 2 — QEMU harness phased failure output + phase early-exit

Status: Complete
Owners: @tools-team, @runtime
Created: 2026-01-23

Links:
- RFC: `docs/rfcs/RFC-0014-testing-contracts-and-qemu-phases-v1.md` (Phase 2)
- Testing guide: `docs/testing/index.md`

## Context

The canonical QEMU smoke harness (`scripts/qemu-test.sh`) currently validates a strict UART marker
sequence and trims logs, but it fails with only a missing marker string. This makes QEMU failures
hard to triage because the output does not name which high-level phase regressed.

RFC-0014 Phase 2 requires:

- phase-first failure naming in harness output,
- optional early-exit by phase,
- bounded, phase-scoped log excerpts on failure.

## Goals

- When QEMU smoke fails, print the **first failed phase** name (e.g. `bring-up`, `routing`, `policy`, `logd`, `ota`, `vfs`, `exec`).
- Support optional early-exit by phase using an explicit knob (e.g. `RUN_PHASE=<phase>`), without weakening ordering enforcement.
- On failure, print a **bounded** excerpt of UART/QEMU logs scoped to the failed phase markers.

## Non-goals

- Rewriting the QEMU harness architecture.
- Adding new success markers that could become fake-green.
- Moving marker truth out of `scripts/qemu-test.sh`.

## Touched paths (planned)

**PROTECTED (requires explicit approval):**

- `scripts/qemu-test.sh`
- `scripts/run-qemu-rv64.sh`

**Docs (if workflow changes):**

- `docs/testing/index.md`
- `docs/rfcs/RFC-0014-testing-contracts-and-qemu-phases-v1.md` (Phase 2 checklist/proofs)

## Stop conditions

- `RUN_UNTIL_MARKER=1 just test-os` failure output includes:
  - `first_failed_phase=<name>`
  - `missing_marker='<marker>'`
  - a bounded excerpt of `uart.log` around the missing marker’s phase window
- `RUN_PHASE=bring-up just test-os` exits successfully after the bring-up phase completes (marker-based).
- `RUN_PHASE=policy just test-os` exits successfully after policy phase completes (marker-based).
- Default behavior (no `RUN_PHASE`) remains unchanged except for improved failure messages/excerpts.

## Proof commands

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os
cd /home/jenning/open-nexus-OS && RUN_PHASE=bring-up RUN_TIMEOUT=90s just test-os
cd /home/jenning/open-nexus-OS && RUN_PHASE=policy RUN_TIMEOUT=190s just test-os
```

## Security considerations

- Do not print secrets in phase excerpts.
- Excerpts must be bounded and deterministic (no huge dumps, no timestamps required for correctness).
