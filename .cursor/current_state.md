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
- **last_decision**: complete `TASK-0016B` plus address-profile governance sync (matrix + ADR + code-alignment + proof reruns)
- **rationale**:
  - keep module boundaries reviewable while reducing duplication drift across handlers
  - enforce stronger type separation across handle families, loopback state, and reply-cap routing
  - preserve deterministic and honest proof markers while improving MMIO/net-failure triage
  - prevent future address/profile drift by defining one normative networking matrix and ADR-backed policy
- **active_constraints**:
  - No fake success markers (only emit `ok` after real behavior proven)
  - OS-lite feature gating (`--no-default-features --features os-lite`)
  - Preserve `netstackd` wire compatibility and marker intent unless explicitly revised in task/RFC evidence
  - Keep `netstackd` as the networking owner per `TASK-0003` / `RFC-0006`; no duplicate authority
  - Loop/retry ownership must stay explicit and bounded; no hidden unbounded helper loops
  - QEMU proofs remain sequential and deterministic (no parallel smoke / 2-VM runs)
  - Network/distributed debugging procedures remain SSOT in `docs/testing/network-distributed-debugging.md`
  - Address/subnet/profile choices are governed by `docs/architecture/network-address-matrix.md` and `docs/adr/0026-network-address-profiles-and-validation.md`

## Current focus (execution)

- **active_task**: `tasks/TASK-0016B-netstackd-refactor-v1-modular-os-daemon-structure.md` (Complete)
- **seed_contract**: `docs/rfcs/RFC-0029-netstackd-modular-daemon-structure-v1.md` (Complete)
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
  - `docs/adr/0025-qemu-smoke-proof-gating.md`
  - `docs/adr/0026-network-address-profiles-and-validation.md`
  - `docs/architecture/network-address-matrix.md`
  - `docs/testing/index.md`
  - `docs/testing/network-distributed-debugging.md`
  - `scripts/qemu-test.sh`
  - `tools/os2vm.sh`
- **phase_now**: task-complete optimization + address-governance sync landed and fully re-proven; prepare follow-on networking tasks
- **baseline_commit**: local working tree after `TASK-0016B` implementation (uncommitted)
- **next_task_slice**: hand off stabilized netstackd seams to follow-ons (`TASK-0194`, `TASK-0196`, `TASK-0249`)
- **proof_commands**:
  - `cargo test -p netstackd --tests -- --nocapture`
  - `cargo test -p dsoftbusd --tests -- --nocapture`
  - `just dep-gate`
  - `just diag-os`
  - `just test-os-dhcp-strict`
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
  - `RUN_OS2VM=1 RUN_TIMEOUT=180s OS2VM_PROFILE=ci RUN_PHASE=end tools/os2vm.sh`
- **last_completed**: `TASK-0016B` implementation + optimization + address-governance sync + full proof gates
  - Outcome:
    - `main.rs` reduced to entry/wiring
    - runtime logic split into `src/os/facade/{runtime,dispatch,state,handlers/*}.rs` with UDP submodule split (`handlers/udp/{bind,send_to,recv_from}.rs`)
    - IPC parse/reply helpers consolidated and extended for payload replies (`src/os/ipc/{parse,reply}.rs`)
    - typed ownership tightened (`StreamId` for loopback peer/pending, `ReplyCapSlot` newtype)
    - malformed-op quirk fixes landed for read/write parse paths
    - DNS selftest marker made honest-green; deterministic MMIO/net fail-codes + halt-reason markers added
    - address-profile constants centralized across `netstackd` and `dsoftbusd` hot paths
    - semantic DNS proof acceptance aligned to protocol attributes (port/QR/TXID) instead of fixed source IP
    - governance/docs aligned via `docs/architecture/network-address-matrix.md` + `docs/adr/0026-network-address-profiles-and-validation.md`
    - host tests extended (`ipc_parse_reply.rs`, `handler_rejects.rs`, `runtime_steps.rs`, `p0_unit.rs`)
    - proof gates green (`cargo test`, `dep-gate`, `diag-os`, `test-os-dhcp-strict`, `os2vm`)

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
- Keep likely follow-ons explicit (`TASK-0194`, `TASK-0196`, `TASK-0249`); no scope pull-in.
- Optional future cleanup: reduce remaining dead-code warnings in shared helper modules without regressing test-path `#[path]` includes.
- Keep address/profile changes centralized through matrix + ADR updates (no ad-hoc literals in follow-on tasks).

## Known risks / hazards
- Residual edge-case drift could still hide in malformed/nonce reply branches despite broad proof coverage.
- Over-splitting by hypothetical future networking features could create unstable seams.
- QEMU proofs must remain sequential and deterministic; avoid shared-artifact races.
- Remaining larger handler files increase review burden and should be split only with strict parity checks.

## DON'T DO (session-local)
- DON'T emit `ready` or `ok` markers for stub/placeholder paths
- DON'T add `parking_lot` or `getrandom` to OS service dependencies
- DON'T change `netstackd` marker/wire contracts without corresponding task/contract evidence updates
- DON'T hide bounded retry ownership inside generic unbounded helper loops
- DON'T introduce a second network-stack authority or MMIO bypass path
- DON'T pull future feature work (`devnet`, `fetchd`, new public networking surface) into `TASK-0016B`
