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
- **last_decision**: close `tasks/TASK-0015-dsoftbusd-refactor-v1-modular-os-daemon-structure.md` as Done and start preparation for `tasks/TASK-0016-dsoftbus-remote-packagefs-ro.md`
- **rationale**:
  - `TASK-0015` delivered the needed modular seams in `dsoftbusd` and removed monolithic orchestration risk from `main.rs`
  - security-negative seam tests and full proof gate are green, so follow-on DSoftBus tasks can build on a stable baseline
  - `TASK-0016` now carries the next functional increment (remote packagefs RO) with security and boundedness as primary constraints
- **active_constraints**:
  - No fake success markers (only emit `ok` after real behavior proven)
  - OS-lite feature gating (`--no-default-features --features os-lite`)
  - Preserve existing DSoftBus wire formats, marker semantics, and retry/timeout budgets
  - Keep remote proxy deny-by-default and keep nonce-correlated shared-inbox handling fail-closed
  - For `TASK-0016`: read-only packagefs surface only (`stat/open/read/close`), no write-like opcodes
  - Kernel remains minimal; no kernel or netstack ownership changes
  - Follow `docs/testing/index.md` for deterministic, sequential proof execution (single-VM then cross-VM)

## Current focus (execution)

- **active_task**: `tasks/TASK-0016-dsoftbus-remote-packagefs-ro.md` (active kickoff)
- **seed_contract**: `docs/rfcs/RFC-0027-dsoftbusd-modular-daemon-structure-v1.md` (completed baseline contract)
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
- **phase_now**: `TASK-0016` prep/kickoff (task doc + `.cursor` working state aligned)
- **baseline_commit**: `4f84e1d` (latest dsoftbusd clippy gate fix after TASK-0015 closure)
- **next_task_slice**: implement bounded remote packagefs RO handler path on existing `dsoftbusd` seams
- **proof_commands**:
  - `cargo clippy -p dsoftbusd --tests -- -D warnings`
  - `cargo test -p dsoftbusd -- --nocapture`
  - `cargo test -p remote_e2e -- --nocapture`
  - `just dep-gate`
  - `just diag-os`
  - `just diag-host`
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
  - `RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh`
- **last_completed**: `tasks/TASK-0015-dsoftbusd-refactor-v1-modular-os-daemon-structure.md`
  - Outcome: Done; RFC-0027 complete; modular seams stabilized; full gate green

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
- `TASK-0016` must keep RO-only and bounded request behavior while reusing gateway/session seams.
- `TASK-0017`/`TASK-0020`/`TASK-0021`/`TASK-0022` remain explicit follow-ons; no scope borrowing into `TASK-0016`.
- Keep canonical proof discipline (host-first, then sequential single-VM/2-VM) for DSoftBus transport/session/gateway changes.
- If byte-frame shortcuts become limiting, record migration boundaries explicitly for `TASK-0020/0021`.

## Known risks / hazards
- Path normalization mistakes could allow traversal outside packagefs namespace.
- Unauthenticated/stale-session requests might bypass intended fail-closed checks if handler boundaries are blurred.
- Oversize read/path or handle exhaustion can regress boundedness guarantees if limits are not enforced at ingress.
- QEMU proofs must still run sequentially; no parallel smoke or 2-VM runs on shared artifacts.
- Harness-level drift can mask service regressions; keep harness parity as part of proof hygiene.

## DON'T DO (session-local)
- DON'T emit `ready` or `ok` markers for stub/placeholder paths
- DON'T add `parking_lot` or `getrandom` to OS service dependencies
- DON'T add write-like packagefs operations in `TASK-0016`
- DON'T accept non-`pkg:/` or non-`/packages/` paths
- DON'T change DSoftBus marker/wire contracts without corresponding task/contract evidence updates
- DON'T pull `TASK-0022` shared-core extraction into `TASK-0016` scope
