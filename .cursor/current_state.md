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
- **last_decision**: start `TASK-0016B` as the next networking-structure task after closing out the `TASK-0016` handoff state
- **rationale**:
  - `source/services/netstackd/src/main.rs` is now the next high-risk monolith on the networking path
  - later networking/devnet work should extend explicit seams, not reopen a 2.4k-line daemon file
  - the new task must follow the `TASK-0015` pattern: contract-first, behavior-preserving structure first, then bounded hardening
- **active_constraints**:
  - No fake success markers (only emit `ok` after real behavior proven)
  - OS-lite feature gating (`--no-default-features --features os-lite`)
  - Preserve `netstackd` wire compatibility and marker intent unless explicitly revised in task/RFC evidence
  - Keep `netstackd` as the networking owner per `TASK-0003` / `RFC-0006`; no duplicate authority
  - Loop/retry ownership must stay explicit and bounded; no hidden unbounded helper loops
  - QEMU proofs remain sequential and deterministic (no parallel smoke / 2-VM runs)
  - Network/distributed debugging procedures remain SSOT in `docs/testing/network-distributed-debugging.md`

## Current focus (execution)

- **active_task**: `tasks/TASK-0016B-netstackd-refactor-v1-modular-os-daemon-structure.md` (In Progress; planning/docs seeded, ready for Phase 0 execution)
- **seed_contract**: `docs/rfcs/RFC-0029-netstackd-modular-daemon-structure-v1.md` (In Progress)
- **contract_dependencies**:
  - `tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md`
  - `tasks/TASK-0010-device-mmio-access-model.md`
  - `tasks/TASK-0249-bringup-rv-virt-v1_2b-os-virtionetd-netstackd-fetchd-echod-selftests.md`
  - `tasks/TASK-0194-networking-v1b-os-devnet-gated-real-connect.md`
  - `tasks/TASK-0196-dsoftbus-v1_1b-devnet-udp-discovery-gated.md`
  - `docs/rfcs/RFC-0006-userspace-networking-v1.md`
  - `docs/rfcs/RFC-0017-device-mmio-access-model-v1.md`
  - `docs/rfcs/RFC-0029-netstackd-modular-daemon-structure-v1.md`
  - `docs/adr/0005-dsoftbus-architecture.md`
  - `docs/testing/index.md`
  - `docs/testing/network-distributed-debugging.md`
  - `scripts/qemu-test.sh`
  - `tools/os2vm.sh`
- **phase_now**: `TASK-0016B` definition + RFC seed complete; ready for Phase 0 implementation planning/execution
- **baseline_commit**: `main` working tree (new task kickoff after `TASK-0016` handoff archival)
- **next_task_slice**: create the `netstackd` internal `src/os/` scaffold and reduce `main.rs` to entry/wiring boundaries only
- **proof_commands**:
  - `cargo test -p netstackd --tests -- --nocapture`
  - `just dep-gate`
  - `just diag-os`
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
  - `RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh`
- **last_completed**: `TASK-0016` handoff archival + `TASK-0016B` / `RFC-0029` seed creation
  - Outcome: new task/RFC/SSOT chain created for `netstackd` modularization and hardening

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
- Implement `TASK-0016B` Phase 0 without widening scope into new networking features.
- Keep likely follow-ons explicit (`TASK-0194`, `TASK-0196`, `TASK-0249`); no scope pull-in.
- Decide after Phase 0 whether any remaining observability/debug labels need a tiny cleanup follow-up rather than bundling into the structural task.
- Reuse the `TASK-0015` discipline: structure first, then bounded hardening, then proofs/docs sync.

## Known risks / hazards
- Refactor drift could silently change `netstackd` marker timing or IPC frame behavior.
- Over-splitting by hypothetical future networking features could create unstable seams.
- Loop cleanup could accidentally hide explicit halt/failure policy behind generic helpers.
- QEMU proofs must still run sequentially; no parallel smoke or 2-VM runs on shared artifacts.
- `netstackd` currently lacks a real host test suite, so test extraction must create narrow seams before claiming hardening coverage.

## DON'T DO (session-local)
- DON'T emit `ready` or `ok` markers for stub/placeholder paths
- DON'T add `parking_lot` or `getrandom` to OS service dependencies
- DON'T change `netstackd` marker/wire contracts without corresponding task/contract evidence updates
- DON'T hide bounded retry ownership inside generic unbounded helper loops
- DON'T introduce a second network-stack authority or MMIO bypass path
- DON'T pull future feature work (`devnet`, `fetchd`, new public networking surface) into `TASK-0016B`
