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
- **last_decision**: implement `OS2VM Debugging + SSOT Consolidation` while continuing `tasks/TASK-0016-dsoftbus-remote-packagefs-ro.md`
- **rationale**:
  - cross-VM failures were previously timeout-heavy with weak first-failure localization
  - deterministic modern virtio-mmio proofs need stronger run evidence (success and failure), not marker-only outcomes
  - network/distributed debugging guidance was duplicated across testing docs and drift-prone
- **active_constraints**:
  - No fake success markers (only emit `ok` after real behavior proven)
  - OS-lite feature gating (`--no-default-features --features os-lite`)
  - Preserve DSoftBus wire compatibility and marker intent unless explicitly revised in task/RFC evidence
  - Keep remote proxy deny-by-default and nonce-correlated shared-inbox handling fail-closed
  - For `TASK-0016`: remote packagefs remains read-only (`stat/open/read/close`) with bounded ingress
  - Cross-VM proof remains sequential and deterministic (no parallel QEMU runs)
  - Network/distributed debugging procedures are SSOT in `docs/testing/network-distributed-debugging.md`

## Current focus (execution)

- **active_task**: `tasks/TASK-0016-dsoftbus-remote-packagefs-ro.md` (active; proof hardening + verification loop)
- **seed_contract**: `docs/rfcs/RFC-0028-dsoftbus-remote-packagefs-ro-v1.md` (active)
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
  - `docs/testing/network-distributed-debugging.md`
  - `scripts/qemu-test.sh`
  - `tools/os2vm.sh`
- **phase_now**: `TASK-0016` runtime-proof hardening complete; continue functional close-out with new `os2vm` diagnostics
- **baseline_commit**: `main` working tree (local staged evolution during TASK-0016 debug loop)
- **next_task_slice**: verify remote markers with new typed `os2vm` summaries and finalize RFC/task evidence
- **proof_commands**:
  - `cargo clippy -p dsoftbusd --tests -- -D warnings`
  - `cargo test -p dsoftbusd -- --nocapture`
  - `cargo test -p remote_e2e -- --nocapture`
  - `just dep-gate`
  - `just diag-os`
  - `just diag-host`
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
  - `RUN_OS2VM=1 RUN_TIMEOUT=180s RUN_PHASE=build OS2VM_SKIP_BUILD=1 tools/os2vm.sh`
  - `RUN_OS2VM=1 RUN_TIMEOUT=180s RUN_PHASE=session OS2VM_PCAP=on OS2VM_EXIT_CODE_MODE=typed tools/os2vm.sh`
  - `RUN_OS2VM=1 RUN_TIMEOUT=180s RUN_PHASE=remote OS2VM_PCAP=auto tools/os2vm.sh`
- **last_completed**: `OS2VM Debugging + SSOT Consolidation` implementation slice
  - Outcome: phase-gated `os2vm`, typed error matrix, structured summaries, packet correlation, SSOT docs sync

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
- Finish green end-to-end `TASK-0016` marker proof using new `os2vm` summaries as evidence input.
- If `OS2VM_E_SESSION_NO_SYN`/`OS2VM_E_SESSION_NO_SYNACK` recurs, keep fixes transport-local and evidence-driven.
- Keep follow-ons explicit (`TASK-0017`, `TASK-0020`, `TASK-0021`, `TASK-0022`); no scope pull-in.
- Evaluate CI usage of `OS2VM_EXIT_CODE_MODE=typed` after local stability.

## Known risks / hazards
- Path normalization mistakes could allow traversal outside packagefs namespace.
- Unauthenticated/stale-session requests might bypass intended fail-closed checks if handler boundaries are blurred.
- Oversize read/path or handle exhaustion can regress boundedness guarantees if limits are not enforced at ingress.
- QEMU proofs must still run sequentially; no parallel smoke or 2-VM runs on shared artifacts.
- Typed harness diagnostics can still misclassify if packet capture is unavailable; validate with UART + marker evidence together.

## DON'T DO (session-local)
- DON'T emit `ready` or `ok` markers for stub/placeholder paths
- DON'T add `parking_lot` or `getrandom` to OS service dependencies
- DON'T add write-like packagefs operations in `TASK-0016`
- DON'T accept non-`pkg:/` or non-`/packages/` paths
- DON'T change DSoftBus marker/wire contracts without corresponding task/contract evidence updates
- DON'T bypass `os2vm` phase summaries when classifying failures
- DON'T pull `TASK-0022` shared-core extraction into `TASK-0016` scope
