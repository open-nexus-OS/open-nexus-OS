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
- **last_decision**: complete `tasks/TASK-0015-dsoftbusd-refactor-v1-modular-os-daemon-structure.md` by finishing RFC-0027 Phase 3 orchestration flattening and rerunning the full sequential proof gate
- **rationale**:
  - `main.rs` was still orchestration-heavy after slices 1+2+3A and blocked maintainable follow-on work
  - Phase 3 extraction moved bootstrap, single-VM bring-up, selftest server path, and cross-VM orchestration into dedicated `src/os/session/*` and `src/os/entry.rs` runners
  - Final sequential gate is green (`cargo test`, `dep-gate`, `diag-os`, `diag-host`, single-VM QEMU, 2-VM QEMU)
  - `main.rs` is now a thin shell (~85 LOC), reducing regression surface for `TASK-0016`/`TASK-0017`/`TASK-0020`/`TASK-0021`/`TASK-0022`
- **active_constraints**:
  - No fake success markers (only emit `ok` after real behavior proven)
  - OS-lite feature gating (`--no-default-features --features os-lite`)
  - Preserve existing DSoftBus wire formats, marker semantics, and retry/timeout budgets
  - Keep remote proxy deny-by-default and keep nonce-correlated shared-inbox handling fail-closed
  - No new DSoftBus features in this task; refactor only
  - Kernel remains minimal; no kernel or netstack ownership changes
  - Follow `docs/testing/index.md` for deterministic, sequential proof execution (single-VM then cross-VM)

## Current focus (execution)

- **active_task**: `tasks/TASK-0016-dsoftbus-remote-packagefs-ro.md` (next)
- **seed_contract**: `docs/rfcs/RFC-0027-dsoftbusd-modular-daemon-structure-v1.md` (completed refactor contract used as baseline)
- **contract_dependencies**:
  - `tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md`
  - `tasks/TASK-0003B-dsoftbus-noise-xk-os.md`
  - `tasks/TASK-0003C-dsoftbus-udp-discovery-os.md`
  - `tasks/TASK-0004-networking-dhcp-icmp-dsoftbus-dual-node.md`
  - `tasks/TASK-0005-networking-cross-vm-dsoftbus-remote-proxy.md`
  - `tasks/TASK-0015-dsoftbusd-refactor-v1-modular-os-daemon-structure.md`
  - `tasks/TASK-0016-dsoftbus-remote-packagefs-ro.md`
  - `tasks/TASK-0017-dsoftbus-remote-statefs-rw.md`
  - `tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md`
  - `tasks/TASK-0021-dsoftbus-quic-v1-host-first-os-scaffold.md`
  - `tasks/TASK-0022-dsoftbus-core-no_std-transport-refactor.md`
  - `docs/rfcs/RFC-0027-dsoftbusd-modular-daemon-structure-v1.md`
  - `docs/adr/0005-dsoftbus-architecture.md`
  - `docs/distributed/dsoftbus-lite.md`
  - `docs/testing/index.md`
  - `scripts/qemu-test.sh`
  - `tools/os2vm.sh`
- **phase_now**: `TASK-0015` completed (RFC-0027 Phase 3 complete, completion gate green)
- **baseline_commit**: unknown (not captured in this prep slice)
- **next_task_slice**: prepare kickoff for `tasks/TASK-0016-dsoftbus-remote-packagefs-ro.md` (follow-on behavior on top of stabilized seams)
- **proof_commands**:
  - `cargo test -p dsoftbusd -- --nocapture`
  - `just dep-gate`
  - `just diag-os`
  - `just diag-host`
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
  - `RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh`
- **last_completed**: `tasks/TASK-0015-dsoftbusd-refactor-v1-modular-os-daemon-structure.md`
  - Outcome: Phase 3 complete; orchestration flattened out of `main.rs`; full sequential completion gate green

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
- `TASK-0016` should consume the stabilized daemon seams without reintroducing monolithic orchestration in `main.rs`.
- `TASK-0022` remains a separate scope boundary: do not move logic into shared crates without explicit task/RFC scope updates.
- Keep canonical proof discipline (host-first, then sequential single-VM/2-VM) for every DSoftBus follow-on touching transport/session/gateway paths.
- Security negative-test debt (`test_reject_*`) remains a follow-up item in RFC-0027 and should be closed in an appropriate follow-on task.

## Known risks / hazards
- Refactor may accidentally change marker timing or the bounded retry semantics relied on by QEMU proofs.
- Cross-VM path is especially regression-prone because transport helpers, session lifecycle, and gateway logic are interleaved in one file today.
- Duplicate helper extraction across single-VM and cross-VM flows can drift if shared seams are chosen poorly.
- QEMU proofs must still run sequentially; no parallel smoke or 2-VM runs on shared artifacts.
- Harness-level drift can mask service regressions (example: missing `metricsd` in `tools/os2vm.sh` service list); treat harness parity as part of proof hygiene.

## DON'T DO (session-local)
- DON'T emit `ready` or `ok` markers for stub/placeholder paths
- DON'T add `parking_lot` or `getrandom` to OS service dependencies
- DON'T change DSoftBus wire formats, marker strings, or remote-proxy policy surface in this task
- DON'T pull `TASK-0022` shared-core extraction into `TASK-0015` without updating the task scope first
