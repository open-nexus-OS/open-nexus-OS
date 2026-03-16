---
title: TASK-0015 DSoftBusd refactor v1: modular OS daemon structure without behavior change
status: Done
owner: @runtime
created: 2026-03-12
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - RFC: docs/rfcs/RFC-0027-dsoftbusd-modular-daemon-structure-v1.md
  - ADR: docs/adr/0005-dsoftbus-architecture.md
  - Docs: docs/distributed/dsoftbus-lite.md
  - Docs: docs/testing/index.md
  - Depends-on: tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md
  - Depends-on: tasks/TASK-0003B-dsoftbus-noise-xk-os.md
  - Depends-on: tasks/TASK-0003C-dsoftbus-udp-discovery-os.md
  - Depends-on: tasks/TASK-0004-networking-dhcp-icmp-dsoftbus-dual-node.md
  - Depends-on: tasks/TASK-0005-networking-cross-vm-dsoftbus-remote-proxy.md
  - Follow-on: tasks/TASK-0016-dsoftbus-remote-packagefs-ro.md
  - Follow-on: tasks/TASK-0017-dsoftbus-remote-statefs-rw.md
  - Follow-on: tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md
  - Follow-on: tasks/TASK-0021-dsoftbus-quic-v1-host-first-os-scaffold.md
  - Follow-on: tasks/TASK-0022-dsoftbus-core-no_std-transport-refactor.md
---

## Context

- `source/services/dsoftbusd/src/main.rs` currently mixes too many responsibilities in one OS-specific file:
  - entry/wiring,
  - netstackd IPC wire protocol + nonce-correlated reply handling,
  - UDP discovery state,
  - session lifecycle / reconnect FSM,
  - Noise XK handshake orchestration,
  - cross-VM remote proxy handling,
  - local IPC API for selftests,
  - metrics / logd helpers.
- This is now a maintenance problem, not just a style issue: the file is hard to review safely, hard to test in narrow slices, and hard to extend for upcoming DSoftBus work (`TASK-0016`, `TASK-0017`, `TASK-0020`, `TASK-0021`, `TASK-0022`) without re-opening the whole daemon each time.
- We need a preparatory refactor that improves internal structure **without** changing the existing transport contracts, marker semantics, or proof behavior.

## Goal

- Refactor `dsoftbusd` into a small set of internal modules with explicit boundaries so the daemon stays behavior-compatible today, but becomes safe to extend in later DSoftBus tasks.

## Current progress snapshot (2026-03-12, Done: Phase 3 + test expansion complete)

- **Completed in this task (slice 1 + slice 2 + slice 3A + Phase 3 orchestration flattening)**:
  - internal `src/os/` scaffold added (`mod.rs`, `entry.rs`, `observability.rs`, `service_clients.rs`),
  - netstack seam extracted into `src/os/netstack/` (`mod.rs`, `ids.rs`, `rpc.rs`, `stream_io.rs`),
  - session seam extracted into `src/os/session/` (`mod.rs`, `fsm.rs`, `handshake.rs`, `records.rs`),
  - orchestration runners extracted into `src/os/session/` (`single_vm.rs`, `selftest_server.rs`, `cross_vm.rs`),
  - discovery/gateway seams extracted into `src/os/discovery/` and `src/os/gateway/`,
  - remote-proxy and local-ipc long loops moved from `main.rs` into `os/gateway` modules,
  - bootstrap/warmup loops and remaining heavy single-VM helper blocks (`rpc_nonce`, slot wait, local-ip resolution, UDP bind, listen retry, peer-ip map helpers, deterministic test-key derivation, connect/accept/read/write helpers) delegated from `main.rs` to `os/entry.rs`,
  - pure, host-testable seams added (`src/os/entry_pure.rs`, `src/os/netstack/validate.rs`, `src/os/session/steps.rs`) and wired into runtime paths without behavior drift,
  - host tests added under `source/services/dsoftbusd/tests/` (`p0_unit.rs`, `reject_transport_validation.rs`, `session_steps.rs`),
  - `dbg:h10..dbg:h13` instrumentation removed/replaced with stable `dbg:dsoftbusd:*` labels.
- **Proof state (final completion gate, sequentially executed)**:
  - `cargo test -p dsoftbusd -- --nocapture`,
  - `cargo test -p remote_e2e -- --nocapture`,
  - `just dep-gate`,
  - `just diag-os`,
  - `just diag-host`,
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`,
  - `RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh`.
- **Structure outcome**:
  - `source/services/dsoftbusd/src/main.rs` reduced from `2699` to `85` LOC and now acts as entry/wiring + high-level routing only,
  - large inline single-VM and cross-VM domain loops moved behind `src/os/**` orchestration surfaces.

## Non-Goals

- No new DSoftBus features.
- No protocol or ABI changes.
- No new marker strings unless a proof regression forces a narrowly justified fix.
- No migration of OS logic into `userspace/dsoftbus` in this task.
- No QUIC, mux, remote-fs, or policy-surface expansion.
- No kernel or `netstackd` behavior changes.

## Constraints / invariants (hard requirements)

- **No fake success**: existing `dsoftbusd:*` and `SELFTEST:*` markers must keep their current semantics.
- **Behavior-preserving refactor**: single-VM and cross-VM proof paths must remain functionally equivalent after the split.
- **Security boundaries preserved**:
  - remote proxy remains deny-by-default,
  - authenticated session flow stays unchanged,
  - reply correlation/nonces stay intact,
  - no secrets or session material in logs.
- **Determinism preserved**:
  - keep bounded retry/would-block behavior,
  - keep deterministic marker order and bounded loops,
  - do not replace explicit budgets with unbounded helper loops.
- **Rust hygiene**:
  - no new `unwrap/expect` in daemon paths,
  - no new `unsafe`,
  - no new dependencies unless clearly necessary.
- **No premature feature architecture**: module boundaries should fit current behavior and known follow-on work, but must not hard-code speculative future features.

## Red flags / decision points (track explicitly)

- **RED (blocking / must decide now)**:
  - Refactoring must not silently change marker timing, retry budgets, or on-wire frame shapes relied on by `scripts/qemu-test.sh` and `tools/os2vm.sh`.
- **YELLOW (risky / likely drift / needs follow-up)**:
  - Over-splitting by hypothetical future features (`quic`, `mux`, `packagefs`, `statefs`) would create churn instead of reducing it.
  - Forcing a full shared-core extraction now would overlap with `TASK-0022` and increase architectural drift.
- **GREEN (confirmed assumptions)**:
  - Clear extraction seams already exist today: netstack RPC helpers, typed IDs/FSM, discovery state, gateway/local IPC, and observability helpers.

## Security considerations

### Threat model
- Refactor regressions accidentally weaken authenticated-session gating.
- Refactor regressions break nonce correlation and allow stale reply misassociation.
- Refactor regressions widen the remote-proxy surface or weaken deny-by-default behavior.

### Security invariants (MUST hold)
- `dsoftbusd: auth ok` must still only appear after a real authenticated handshake.
- Remote proxy behavior must remain deny-by-default and bounded.
- Shared-inbox reply correlation must remain nonce-bound and fail-closed.
- No secrets, private keys, or session material may be logged.

### DON'T DO (explicit prohibitions)
- DON'T change wire formats as part of the refactor.
- DON'T change allowlisted remote services as part of the refactor.
- DON'T introduce fallback loops that hide reply-correlation or session-lifecycle bugs.

### Attack surface impact
- Intended impact: none.
- Regression risk: significant, because the touched code handles network/auth/IPC boundaries.

### Mitigations
- Keep proof commands unchanged and rerun both single-VM and cross-VM contracts.
- Add focused unit tests where extraction creates stable seams (FSM / reply correlation / bounded parsing).

## Security proof

### Audit tests (negative cases / attack simulation)
- Command(s):
  - `cargo test -p dsoftbusd -- --nocapture`
- Implemented coverage:
  - `test_reject_nonce_mismatch_response`
  - `test_reject_unexpected_response_opcode`
  - `test_reject_zero_length_status_ok_read_frame`
  - `test_reject_oversized_udp_payload`
  - `test_reject_identity_binding_mismatch`
  - `test_reconnect_path_closes_old_session_and_advances_retry_state`
  - `test_parse_helpers_cover_status_and_nonce_extraction`
  - `test_identity_binding_absent_mapping_is_nonfatal`
  - `test_discovery_step_cadence_rules`
  - `test_fsm_phase_setters_are_exercised`
  - full seam suites in `source/services/dsoftbusd/tests/p0_unit.rs`, `source/services/dsoftbusd/tests/reject_transport_validation.rs`, `source/services/dsoftbusd/tests/session_steps.rs`

### Hardening markers (QEMU, if applicable)
- Existing marker semantics remain unchanged:
  - `dsoftbusd: auth ok`
  - `dsoftbusd: remote proxy denied (service=unknown)`
  - `SELFTEST: dsoftbus ping ok`
  - `SELFTEST: remote resolve ok`

## Contract sources (single source of truth)

- **Single-VM marker contract**: `scripts/qemu-test.sh`
- **Cross-VM proof contract**: `tools/os2vm.sh`
- **Architecture boundary**: `docs/adr/0005-dsoftbus-architecture.md`
- **Current daemon implementation**: `source/services/dsoftbusd/src/main.rs`
- **Follow-on reuse boundary**: `tasks/TASK-0022-dsoftbus-core-no_std-transport-refactor.md`

## Stop conditions (Definition of Done)

- **Structure**:
  - `dsoftbusd` no longer concentrates its OS daemon logic in one 3k-line `main.rs`.
  - `main.rs` becomes a thin entry/wiring file.
  - Internal responsibilities are split into cohesive modules with boundaries roughly covering:
    - entry/runtime wiring,
    - netstack IPC adapter,
    - discovery state/announce/peer learning,
    - session lifecycle + handshake,
    - local IPC / remote gateway,
    - observability helpers.
- **Behavior**:
  - Existing single-VM DSoftBus behavior remains unchanged.
  - Existing cross-VM remote-proxy behavior remains unchanged.
  - Existing marker names and proof semantics remain unchanged.
- **Proof (build)**:
  - Command(s):
    - `just dep-gate`
    - `just diag-os`
    - `just diag-host`
- **Proof (host-first regression)**:
  - Command(s):
    - `cargo test -p dsoftbusd -- --nocapture`
    - `cargo test -p remote_e2e -- --nocapture`
- **Proof (single VM / canonical)**:
  - Command(s):
    - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
  - Required existing markers remain green, including:
    - `dsoftbusd: auth ok`
    - `SELFTEST: dsoftbus ping ok`
- **Proof (cross-VM / canonical opt-in)**:
  - Command(s):
    - `RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh`
  - Required existing markers remain green, including:
    - `dsoftbusd: discovery cross-vm up`
    - `dsoftbusd: cross-vm session ok`
    - `SELFTEST: remote resolve ok`
    - `SELFTEST: remote query ok`
- **Execution discipline**:
  - single-VM and cross-VM QEMU proofs are run sequentially (never in parallel).

## Erfuellt-Bedingung (normative completion gate)

Per `docs/testing/index.md` (host-first, OS-last), this task is only considered fulfilled when all of the following are green and marker semantics remain unchanged:

1. Host seam/regression checks:
   - `cargo test -p dsoftbusd -- --nocapture`
   - `cargo test -p remote_e2e -- --nocapture`
2. Build hygiene:
   - `just dep-gate`
   - `just diag-os`
   - `just diag-host`
3. OS smoke / proof:
   - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
   - `RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh`
4. Execution discipline:
   - QEMU proofs executed sequentially (single-VM, then 2-VM).

## Touched paths (allowlist)

Only these paths may be modified without opening a separate task/ADR:

- `source/services/dsoftbusd/**`
- `docs/distributed/dsoftbus-lite.md`
- `docs/testing/index.md`
- `tools/os2vm.sh` (harness-only sync; no marker/wire contract drift)

## Plan (small PRs)

1. **Create the module skeleton**
   - Reduce `src/main.rs` to environment selection + entry wiring.
   - Introduce an `os/` module tree for internal daemon organization.
   - Suggested target shape (names may vary slightly if a better equivalent emerges during implementation):

   ```text
   source/services/dsoftbusd/src/
     main.rs
     os/
       mod.rs
       entry.rs
       observability.rs
       netstack/
         mod.rs
         rpc.rs
         ids.rs
         stream_io.rs
       discovery/
         mod.rs
         state.rs
       session/
         mod.rs
         fsm.rs
         handshake.rs
         records.rs
       gateway/
         mod.rs
         local_ipc.rs
         remote_proxy.rs
         service_clients.rs
   ```

2. **Extract the netstackd adapter boundary**
   - Centralize wire constants, typed socket/listener/session IDs, nonce correlation, and bounded `tcp_*` / `udp_*` / `stream_*` helpers.
   - Remove duplicate inline protocol helper blocks where practical.

3. **Extract domain logic**
   - Move discovery peer-state handling into a dedicated module.
   - Move session FSM + reconnect ownership into a dedicated module.
   - Move Noise handshake orchestration and encrypted record handling into dedicated modules.

4. **Extract API surfaces**
   - Separate local IPC server handling from the remote proxy server path.
   - Isolate service client access (`samgrd`, `bundlemgrd`, `logd`) from transport/session logic.

5. **Add narrow tests where extraction exposes stable seams**
   - Prefer small unit tests over new integration scope.
   - Keep tests deterministic and bounded.

6. **Proof + docs sync**
   - Re-run existing canonical proofs.
   - Update docs only where the internal daemon structure or developer guidance would otherwise drift.

## Task map alignment (program-level sequencing)

- **P0 foundations (already completed prerequisites)**:
  - `TASK-0003`, `TASK-0003B`, `TASK-0003C`, `TASK-0004`, `TASK-0005`
- **P1 immediate consumers of this refactor seam**:
  - `TASK-0016` (remote packagefs RO)
  - `TASK-0017` (remote statefs RW)
- **P2 transport/evolution follow-ons that depend on clean seams**:
  - `TASK-0020` (streams v2 mux/flow-control)
  - `TASK-0021` (QUIC v1 host-first OS scaffold)
  - `TASK-0022` (shared no_std transport-core refactor)

## Acceptance criteria (behavioral)

- `main.rs` is a thin wrapper instead of the execution truth for the whole daemon.
- Netstack IPC helper logic is centralized behind one internal adapter boundary.
- Session lifecycle code is no longer embedded directly inside the cross-VM entry loop.
- Gateway/local IPC handling is no longer interleaved with transport/session code in one giant loop body.
- Existing DSoftBus marker semantics stay intact.
- The refactor prepares later DSoftBus tasks without pre-committing to QUIC/mux/packagefs-specific modules.

## Evidence (to paste into PR)

- Build:
  - `just dep-gate`
  - `just diag-os`
  - `just diag-host`
- Host-first regression:
  - `cargo test -p dsoftbusd -- --nocapture`
  - `cargo test -p remote_e2e -- --nocapture`
- Single VM:
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
- Cross-VM:
  - `RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh`
- Discipline:
  - single-VM and 2-VM proofs run sequentially
- Tests:
  - `source/services/dsoftbusd/tests/p0_unit.rs`
  - `source/services/dsoftbusd/tests/reject_transport_validation.rs`
  - `source/services/dsoftbusd/tests/session_steps.rs`
  - summary of host and OS proof outputs above

## RFC seeds (for later, when the step is complete)

- Decisions made:
  - final internal module boundaries for `dsoftbusd`
  - extraction seam between daemon-local transport code and future reusable DSoftBus core work
- Open questions:
  - which pieces should later move into shared no_std-capable crates under `TASK-0022`
  - whether any remote-gateway record handling should become a reusable protocol module later
- Stabilized contracts:
  - existing QEMU and 2-VM proof behavior stayed unchanged while the daemon structure was modularized
