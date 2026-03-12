---
title: TASK-0015 DSoftBusd refactor v1: modular OS daemon structure without behavior change
status: In Progress
owner: @runtime
created: 2026-03-12
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - RFC: docs/rfcs/RFC-0027-dsoftbusd-modular-daemon-structure-v1.md
  - ADR: docs/adr/0005-dsoftbus-architecture.md
  - Docs: docs/distributed/dsoftbus-lite.md
  - Follow-on: tasks/TASK-0016-dsoftbus-remote-packagefs-ro.md
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
- This is now a maintenance problem, not just a style issue: the file is hard to review safely, hard to test in narrow slices, and hard to extend for upcoming DSoftBus work (`TASK-0016`, `TASK-0020`, `TASK-0021`, `TASK-0022`) without re-opening the whole daemon each time.
- We need a preparatory refactor that improves internal structure **without** changing the existing transport contracts, marker semantics, or proof behavior.

## Goal

- Refactor `dsoftbusd` into a small set of internal modules with explicit boundaries so the daemon stays behavior-compatible today, but becomes safe to extend in later DSoftBus tasks.

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
- Required coverage (if module seams are extracted cleanly enough):
  - nonce-correlation helper rejects mismatched replies
  - session FSM reconnect path advances epoch and drops old session handle

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

## Touched paths (allowlist)

Only these paths may be modified without opening a separate task/ADR:

- `source/services/dsoftbusd/**`
- `docs/distributed/dsoftbus-lite.md`
- `docs/testing/index.md`

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
- Single VM:
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
- Cross-VM:
  - `RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh`
- Tests:
  - any added `cargo test -p dsoftbusd -- --nocapture` summary

## RFC seeds (for later, when the step is complete)

- Decisions made:
  - final internal module boundaries for `dsoftbusd`
  - extraction seam between daemon-local transport code and future reusable DSoftBus core work
- Open questions:
  - which pieces should later move into shared no_std-capable crates under `TASK-0022`
  - whether any remote-gateway record handling should become a reusable protocol module later
- Stabilized contracts:
  - existing QEMU and 2-VM proof behavior stayed unchanged while the daemon structure was modularized
