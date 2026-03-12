# Cursor Current State (SSOT)

<!--
CONTEXT
This file is the single source of truth for the *current* system state.
It is intentionally compact and overwritten after each completed task.

Rules:
- Prefer structured bullets over prose.
- Include "why" (decision rationale), not implementation narration.
- Reference tasks/RFCs/ADRs with relative paths.
-->

## Current architecture state
- **last_decision**: `tasks/TASK-0015-dsoftbusd-refactor-v1-modular-os-daemon-structure.md` selected as the next execution slice after `TASK-0014`
- **rationale**:
  - `dsoftbusd` currently concentrates transport IPC, discovery, session lifecycle, handshake, gateway, and observability in one OS-specific `main.rs`
  - Upcoming DSoftBus work (`TASK-0016`, `TASK-0020`, `TASK-0021`, `TASK-0022`) needs clearer internal seams before new behavior is added
  - This refactor should reduce review risk and make future changes narrower without reopening transport contracts
- **active_constraints**:
  - No fake success markers (only emit `ok` after real behavior proven)
  - OS-lite feature gating (`--no-default-features --features os-lite`)
  - Preserve existing DSoftBus wire formats, marker semantics, and retry/timeout budgets
  - Keep remote proxy deny-by-default and keep nonce-correlated shared-inbox handling fail-closed
  - No new DSoftBus features in this task; refactor only
  - Kernel remains minimal; no kernel or netstack ownership changes

## Current focus (execution)

- **active_task**: `tasks/TASK-0015-dsoftbusd-refactor-v1-modular-os-daemon-structure.md`
- **seed_contract**: `docs/rfcs/RFC-0027-dsoftbusd-modular-daemon-structure-v1.md`
- **contract_dependencies**:
  - `tasks/TASK-0015-dsoftbusd-refactor-v1-modular-os-daemon-structure.md`
  - `docs/rfcs/RFC-0027-dsoftbusd-modular-daemon-structure-v1.md`
  - `docs/adr/0005-dsoftbus-architecture.md`
  - `docs/distributed/dsoftbus-lite.md`
  - `scripts/qemu-test.sh`
  - `tools/os2vm.sh`
- **phase_now**: task is in progress; first implementation slice should extract internal daemon seams without behavior change
- **baseline_commit**: unknown (not captured in this prep slice)
- **next_task_slice**: thin `main.rs`, create `os/` module tree, centralize transport/discovery/session/gateway seams
- **proof_commands**:
  - `cargo test -p dsoftbusd -- --nocapture`
  - `just dep-gate`
  - `just diag-os`
  - `just diag-host`
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
  - `RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh`
- **last_completed**: `tasks/TASK-0014-observability-v2-metrics-tracing.md`
  - Outcome: observability v2 closed; task tracking and handoff are now advanced to `TASK-0015`

## Active invariants (must hold)
- **security**
  - Secrets never logged (device keys, credentials, tokens)
  - Identity from kernel IPC (`sender_service_id`), never payload strings
  - Bounded input sizes; validate before parse; no `unwrap/expect` on untrusted data
  - Policy enforcement via `policyd` (deny-by-default + audit)
  - MMIO mappings are USER|RW and NEVER executable (W^X enforced at page table)
  - Device capabilities require explicit grant (no ambient MMIO access)
  - Per-device windows bounded to exact BAR/window (no overmap)
- **determinism**
  - Marker strings stable and non-random
  - Tests bounded (no infinite/unbounded waits)
  - UART output deterministic for CI verification
  - QEMU runs bounded by RUN_TIMEOUT + early exit on markers
- **build hygiene**
  - OS services use `--no-default-features --features os-lite`
  - Forbidden crates: `parking_lot`, `parking_lot_core`, `getrandom`
  - `just dep-gate` MUST pass before OS commits
  - `just diag-os` verifies OS services compile for riscv64

## Open threads / follow-ups
- `TASK-0015` should prepare, but not subsume, `TASK-0022`:
  - extract seams inside `dsoftbusd`
  - do not move logic into shared crates unless the task scope is updated explicitly
- Keep `source/services/dsoftbusd/src/main.rs` behavior-stable while splitting:
  - netstack IPC adapter
  - discovery state
  - session FSM / handshake
  - gateway / local IPC
  - observability helpers
- If the refactor reveals a missing external contract, stop and decide whether a new RFC/ADR is required before continuing.

## Known risks / hazards
- Refactor may accidentally change marker timing or the bounded retry semantics relied on by QEMU proofs.
- Cross-VM path is especially regression-prone because transport helpers, session lifecycle, and gateway logic are interleaved in one file today.
- Duplicate helper extraction across single-VM and cross-VM flows can drift if shared seams are chosen poorly.
- QEMU proofs must still run sequentially; no parallel smoke or 2-VM runs on shared artifacts.

## DON'T DO (session-local)
- DON'T emit `ready` or `ok` markers for stub/placeholder paths
- DON'T add `parking_lot` or `getrandom` to OS service dependencies
- DON'T change DSoftBus wire formats, marker strings, or remote-proxy policy surface in this task
- DON'T pull `TASK-0022` shared-core extraction into `TASK-0015` without updating the task scope first
