---
title: TASK-0016B Netstackd refactor v1: modular OS daemon structure + loop/idiom hardening
status: In Review
owner: @runtime
created: 2026-03-24
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - RFC: docs/rfcs/RFC-0029-netstackd-modular-daemon-structure-v1.md
  - ADR: docs/adr/0005-dsoftbus-architecture.md
  - ADR: docs/adr/0026-network-address-profiles-and-validation.md
  - Architecture SSOT: docs/architecture/network-address-matrix.md
  - RFC (userspace networking contract): docs/rfcs/RFC-0006-userspace-networking-v1.md
  - Depends-on: tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md
  - Depends-on: tasks/TASK-0010-device-mmio-access-model.md
  - Related: tasks/TASK-0249-bringup-rv-virt-v1_2b-os-virtionetd-netstackd-fetchd-echod-selftests.md
  - Follow-on: tasks/TASK-0194-networking-v1b-os-devnet-gated-real-connect.md
  - Follow-on: tasks/TASK-0196-dsoftbus-v1_1b-devnet-udp-discovery-gated.md
  - Testing methodology: docs/testing/index.md
  - Testing contract: scripts/qemu-test.sh
  - Testing contract (2-VM): tools/os2vm.sh
---

## Context

- `source/services/netstackd/src/main.rs` currently concentrates almost the entire OS daemon in one file:
  - entry/wiring,
  - bootstrap + MMIO bring-up retry,
  - DHCP/fallback/static IPv4 policy,
  - gateway ping / DNS / TCP selftest bring-up,
  - IPC wire parsing and reply encoding,
  - handle tables for listeners/streams/UDP sockets,
  - loopback shims for local-only TCP/UDP bring-up,
  - the full facade service loop and RPC dispatch.
- This is now a maintenance and review problem, not only a style issue:
  - follow-on networking work would reopen a 2.4k-line file for every change,
  - the current structure hides ownership boundaries and retry budgets,
  - the file contains repeated byte-frame encoding and handle bookkeeping that is hard to test in narrow slices,
  - at least one daemon-path `expect` remains today, which conflicts with project daemon hygiene.
- We want the same preparatory cleanup that `TASK-0015` provided for `dsoftbusd`, but adapted to `netstackd` and extended with a second hardening phase for modern Rust idioms and explicit loop ownership.

## Goal

- Refactor `netstackd` into a small set of internal modules with explicit boundaries so the daemon remains behavior-compatible today, but becomes safe to extend for later networking tasks.
- After the structural split, harden the remaining large loops and typed boundaries using modern Rust patterns (`newtype`, `#[must_use]`, explicit ownership, bounded step helpers) without changing the existing public marker or IPC-wire behavior.

## Current state snapshot (2026-03-24)

- `netstackd` is the networking owner in the current OS bring-up contract (`TASK-0003`, `RFC-0006`).
- `main.rs` is reduced to thin entry/wiring (`ready` marker -> bootstrap -> facade runtime handoff).
- Internal module seams now exist under `source/services/netstackd/src/os/` for bootstrap, observability, IPC wire helpers, loopback helpers, and facade runtime/ops helpers.
- Dedicated host tests now exist under `source/services/netstackd/tests/`
  (`p0_unit.rs`, `handler_rejects.rs`, `runtime_steps.rs`, `ipc_parse_reply.rs`, `loopback_observability.rs`).
- `scripts/qemu-test.sh` already gates key `netstackd` markers (`netstackd: ready`, `SELFTEST: net iface ok`, `SELFTEST: net ping ok`, `SELFTEST: net udp dns ok`, `SELFTEST: net tcp listen ok`).
- Address profile governance is now explicit and centralized in:
  - `docs/architecture/network-address-matrix.md`
  - `docs/adr/0026-network-address-profiles-and-validation.md`

## Progress update (2026-03-24 session)

- Landed behavior-preserving modular split for `netstackd`:
  - facade runtime loop moved from `main.rs` to `source/services/netstackd/src/os/facade/runtime.rs`,
  - bootstrap/marker formatting and IPC helper seams moved to `src/os/**`,
  - typed handle wrappers introduced and integrated (`ListenerId`, `StreamId`, `UdpId`).
- Added required negative seam tests:
  - `test_reject_all_supported_ops_malformed_status_frame_shape`
  - `test_reject_handle_ops_not_found_status_frame_shape`
  - `test_reject_unknown_op_status_frame_shape`
  - `test_reject_invalid_wire_handles`
  - `test_reject_oversized_loopback_payload`
  - `test_pending_connect_unexpected_state_detection`
- Proof status in this session:
  - ✅ `cargo test -p netstackd --tests -- --nocapture`
  - ✅ `just dep-gate`
  - ✅ `just diag-os`
  - ✅ `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
  - ✅ `RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh` (`summary.txt`: `result: success`)

## Progress update (2026-03-24 optimization session)

- Applied post-completion structure and hardening slice on top of the modular split:
  - moved oversized UDP handler into `source/services/netstackd/src/os/facade/handlers/udp/{bind,send_to,recv_from}.rs`,
  - centralized additional reply payload framing in `source/services/netstackd/src/os/ipc/reply.rs`,
  - extracted repeated TCP would-block retry loop into `source/services/netstackd/src/os/facade/tcp.rs`,
  - tightened typed ownership boundaries (`StreamId` in loopback peer/pending state, typed `ReplyCapSlot` for reply cap routing),
  - added explicit ownership model notes in facade state/dispatch/runtime modules,
  - fixed malformed-op reply quirks in read/write parse paths (`OP_READ`, `OP_WRITE`).
- Determinism/observability hardening:
  - DNS success marker is now honest-green (`SELFTEST: net udp dns ok` only when DNS probe succeeds),
  - stable DNS failure marker added (`netstackd: net dns proof fail`),
  - MMIO/net init failure paths now emit additive deterministic fail-code labels (`netstackd: net fail-code 0x....`),
  - fatal floor paths emit one stable halt-reason marker before intentional park loops.
- Extended host test coverage:
  - new reply payload helper golden-frame tests in `tests/ipc_parse_reply.rs`,
  - validation outcome seam test in `tests/runtime_steps.rs`,
  - typed reply-cap slot roundtrip test in `tests/p0_unit.rs`.
- Re-proved after optimization:
  - ✅ `cargo test -p netstackd --tests -- --nocapture`
  - ✅ `just dep-gate`
  - ✅ `just diag-os`
  - ✅ `just test-os`
  - ✅ `RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh` (`summary.json`: `result=success`)

## Progress update (2026-03-24 address-governance sync session)

- Added normative networking profile governance and linked it to this completed task scope:
  - `docs/architecture/network-address-matrix.md` (SSOT for QEMU/os2vm address profiles)
  - `docs/adr/0026-network-address-profiles-and-validation.md` (policy/governance ADR)
- Synced related docs so contracts do not drift:
  - `docs/architecture/networking-authority.md`
  - `docs/distributed/dsoftbus-lite.md`
  - `docs/testing/network-distributed-debugging.md`
  - `docs/testing/e2e-coverage-matrix.md`
  - `docs/adr/0025-qemu-smoke-proof-gating.md`
- Aligned runtime code to named address-profile constants and semantic DNS-proof validation:
  - `source/services/netstackd/src/os/entry_pure.rs`
  - `source/services/netstackd/src/os/bootstrap.rs`
  - `source/services/netstackd/src/os/facade/**`
  - `source/services/dsoftbusd/src/os/{entry,entry_pure}.rs`
  - `source/services/dsoftbusd/src/os/session/{single_vm,cross_vm}.rs`
- Added/updated host tests for the new pure-seam address/profile helpers.
- Re-proved after address-governance sync:
  - ✅ `cargo test -p netstackd --tests -- --nocapture`
  - ✅ `cargo test -p dsoftbusd --tests -- --nocapture`
  - ✅ `just test-os-dhcp-strict`
  - ✅ `RUN_OS2VM=1 RUN_TIMEOUT=180s OS2VM_PROFILE=ci RUN_PHASE=end tools/os2vm.sh`

## Non-Goals

- No new network features, APIs, or transport behavior.
- No change to the userspace networking contract owned by `RFC-0006`.
- No new `netstackd` marker names unless a narrowly justified proof fix requires it.
- No kernel, MMIO, or capability-distribution redesign.
- No migration into new shared libraries under `source/libs/**` in this task.
- No speculative feature split by future domains such as devnet, fetchd, or QUIC-like transports.

## Constraints / invariants (hard requirements)

- **No fake success**: existing `netstackd:*` and `SELFTEST:*` markers must keep their current semantics.
- **Behavior-preserving Phase 0**:
  - same IPC wire format,
  - same marker names,
  - same bounded retry behavior,
  - same bootstrap/fallback intent.
- **No duplicate authority**:
  - `netstackd` remains the single owner of the networking stack in the `TASK-0003` / `RFC-0006` model,
  - this task must not create a second stack authority or undermine MMIO ownership rules from `TASK-0010`.
- **Determinism preserved**:
  - keep bounded loops and explicit budgets,
  - do not replace explicit retry ownership with hidden unbounded helpers,
  - keep UART markers stable and non-random.
- **Rust hygiene**:
  - remove daemon-path `unwrap/expect`,
  - no new `unsafe`,
  - no new dependencies unless clearly necessary,
  - keep OS build hygiene (`--no-default-features --features os-lite`) intact.
- **Testing discipline**:
  - new tests must prove desired behavior and fail-closed parsing/ownership boundaries,
  - host and OS proof commands must stay canonical and reproducible.

## Red flags / decision points (track explicitly)

- **RED (blocking / must decide now)**:
  - The task must stay internal-structure-first. If refactor pressure reveals a real public contract change, stop and create a separate RFC/task instead of silently expanding `16B`.
- **YELLOW (risky / likely drift / needs follow-up)**:
  - Over-splitting by hypothetical future features (`devnet`, `fetchd`, `virtionetd`) would create churn rather than stable seams.
  - Hiding end-state halt loops inside generic helpers would reduce debuggability and violate the current explicit failure policy.
- **GREEN (confirmed assumptions)**:
  - The current file already exposes natural seams: bootstrap, observability, loopback shim, IPC wire/reply handling, handle bookkeeping, and op-specific facade operations.

## Security considerations

### Threat model
- Refactor regressions accidentally weaken boundedness on request parsing, handle lookup, or retry ownership.
- Reply/wire refactors silently change behavior and make malformed frames fail open.
- Loop/ownership cleanup hides liveness failures behind generic helper behavior.

### Security invariants (MUST hold)
- Invalid or malformed IPC frames remain rejected deterministically.
- Handle lookup and socket state transitions remain bounded and fail-closed.
- `netstackd` remains the only service owning the underlying network stack in this path.
- Logs/markers must not leak secrets or introduce nondeterministic content.

### DON'T DO (explicit prohibitions)
- DON'T change the public wire format in this refactor task.
- DON'T add background threads, hidden retries, or unbounded drain loops.
- DON'T turn fatal bring-up policy into silent fallback success.
- DON'T add speculative abstractions that only exist for hypothetical future transports.

### Attack surface impact
- Intended impact: none.
- Regression risk: medium, because the touched code owns networking, IPC parsing, and bring-up/error policy.

### Mitigations
- Keep marker/wire proofs unchanged and rerun canonical QEMU gates.
- Add focused host tests for parse/reply helpers, handle IDs, loopback bounds, and bounded step behavior.
- Make remaining explicit halt/retry ownership visible in typed step helpers rather than broad generic utilities.

## Security proof

### Audit tests (negative cases / attack simulation)
- Command(s):
  - `cargo test -p netstackd --tests -- --nocapture`
- Required coverage:
  - `test_reject_all_supported_ops_malformed_status_frame_shape`
  - `test_reject_handle_ops_not_found_status_frame_shape`
  - `test_reject_unknown_op_status_frame_shape`
  - `test_reject_invalid_wire_handles`
  - `test_reject_oversized_loopback_payload`
  - `test_pending_connect_unexpected_state_detection`

### Hardening markers (QEMU, if applicable)
- Existing marker semantics remain unchanged:
  - `netstackd: ready`
  - `SELFTEST: net iface ok`
  - `SELFTEST: net ping ok`
  - `SELFTEST: net udp dns ok`
  - `SELFTEST: net tcp listen ok`

## Contract sources (single source of truth)

- **Userspace networking contract**: `docs/rfcs/RFC-0006-userspace-networking-v1.md`
- **Task execution truth for current networking owner path**: `tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md`
- **MMIO/capability boundary**: `tasks/TASK-0010-device-mmio-access-model.md`
- **Single-VM marker contract**: `scripts/qemu-test.sh`
- **Cross-VM regression contract**: `tools/os2vm.sh`
- **Current daemon implementation**: `source/services/netstackd/src/main.rs`

## Stop conditions (Definition of Done)

- **Structure**:
  - `netstackd` no longer concentrates its OS daemon logic in one monolithic `main.rs`.
  - `main.rs` becomes a thin entry/wiring file.
  - internal responsibilities are split into cohesive modules with boundaries roughly covering:
    - entry/runtime wiring,
    - bootstrap + fallback configuration,
    - observability/marker helpers,
    - IPC wire parsing + reply encoding,
    - handle IDs/tables,
    - loopback shim,
    - op-specific facade logic (listen/accept/connect/read/write/udp/ping/close).
- **Behavior**:
  - existing single-VM `netstackd` behavior remains intact,
  - marker names remain stable and semantics are honest-green (`ok` only after real success),
  - existing IPC wire compatibility remains unchanged.
- **Hardening**:
  - daemon-path `expect` / `unwrap` are removed,
  - handle IDs are strongly typed,
  - repeated response-frame construction is centralized,
  - remaining bounded retry loops are named and testable instead of duplicated inline.
- **Proof (build)**:
  - Command(s):
    - `just dep-gate`
    - `just diag-os`
- **Proof (host-first regression)**:
  - Command(s):
    - `cargo test -p netstackd --tests -- --nocapture`
- **Proof (single VM / canonical)**:
  - Command(s):
    - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
  - Required existing markers remain green, including:
    - `netstackd: ready`
    - `SELFTEST: net iface ok`
    - `SELFTEST: net ping ok`
    - `SELFTEST: net udp dns ok`
    - `SELFTEST: net tcp listen ok`
- **Proof (cross-VM / regression, if unchanged by profile)**:
  - Command(s):
    - `RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh`
  - Existing fallback/static-IP profile remains compatible if exercised by the current harness.

## Erfuellt-Bedingung (normative completion gate)

Per `docs/testing/index.md` (host-first, OS-last), this task is only considered fulfilled when all of the following are green and marker semantics remain unchanged:

1. Host seam/regression checks:
   - `cargo test -p netstackd --tests -- --nocapture`
2. Build hygiene:
   - `just dep-gate`
   - `just diag-os`
3. OS smoke / proof:
   - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
4. Optional harness regression when this path is still exercised:
   - `RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh`
5. Structure gate:
   - `source/services/netstackd/src/main.rs` is reduced to entry/wiring only.

## Touched paths (allowlist)

Only these paths may be modified without opening a separate task/ADR:

- `source/services/netstackd/**`
- `tasks/TASK-0016B-netstackd-refactor-v1-modular-os-daemon-structure.md`
- `docs/rfcs/RFC-0029-netstackd-modular-daemon-structure-v1.md`
- `docs/rfcs/README.md`
- `docs/testing/index.md` (only if proof/developer guidance would otherwise drift)
- `scripts/qemu-test.sh` (only if existing marker gate definitions need sync without semantic drift)
- `tools/os2vm.sh` (only if regression-harness sync is required without semantic drift)

## Plan (small PRs)

1. **Create the module skeleton**
   - Reduce `src/main.rs` to environment selection + entry wiring.
   - Introduce an `os/` module tree for internal daemon organization.
   - Suggested target shape:

   ```text
   source/services/netstackd/src/
     main.rs
     os/
       mod.rs
       entry.rs
       context.rs
       bootstrap.rs
       observability.rs
       config.rs
       loopback.rs
       ipc/
         mod.rs
         wire.rs
         parse.rs
         reply.rs
         handles.rs
       facade/
         mod.rs
         ops.rs
         tcp.rs
         udp.rs
         ping.rs
         validation.rs
   ```

2. **Extract the bootstrap boundary**
   - Move stack bring-up, DHCP/fallback/static-IP policy, and selftest warmup/probe steps behind typed helpers.
   - Preserve current bounded deadlines and explicit terminal failure policy.

3. **Extract the IPC/facade boundary**
   - Centralize wire constants, nonce parsing, reply builders, and typed handle IDs.
   - Move the large op dispatch into dedicated operation helpers without changing frame semantics.

4. **Phase 1 hardening**
   - Replace loose numeric IDs with `newtype` handles.
   - Remove daemon-path `expect`.
   - Introduce `#[must_use]` and typed step outcomes where this prevents silent error handling drift.
   - Keep explicit bounded retry ownership visible and testable.

5. **Add narrow tests**
   - Create host tests for pure/near-pure seams.
   - Prefer deterministic unit-style coverage over new integration scope.

6. **Proof + docs sync**
   - Re-run canonical build/host/QEMU proofs.
   - Update RFC/task/docs only where the internal daemon shape or proof guidance would otherwise drift.

## Task map alignment (program-level sequencing)

- **P0 foundations (already completed prerequisites)**:
  - `TASK-0003`
  - `TASK-0010`
- **P1 immediate structural outcome**:
  - `TASK-0016B` creates a stable internal seam for future `netstackd` work.
- **P2 likely downstream consumers**:
  - `TASK-0194` (OS devnet real connect)
  - `TASK-0196` (DSoftBus devnet UDP discovery gated)
  - `TASK-0249` (bring-up alternative/expansion)

## Acceptance criteria (behavioral)

- `main.rs` is a thin wrapper instead of the execution truth for the whole daemon.
- Bootstrap/fallback logic is no longer embedded directly in `main.rs`.
- IPC wire helper logic is centralized behind one internal adapter boundary.
- Loopback/handle bookkeeping is no longer interleaved across the entire RPC loop body.
- Existing `netstackd` marker names stay stable; success markers remain honest-green.
- The refactor prepares later networking tasks without pre-committing to speculative modules.

## Evidence (to paste into PR)

- Build:
  - `just dep-gate`
  - `just diag-os`
- Host-first regression:
  - `cargo test -p netstackd --tests -- --nocapture`
- Single VM:
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
- Cross-VM regression (if exercised):
  - `RUN_OS2VM=1 RUN_TIMEOUT=180s tools/os2vm.sh`
- Tests:
  - `source/services/netstackd/tests/p0_unit.rs`
  - `source/services/netstackd/tests/handler_rejects.rs`
  - `source/services/netstackd/tests/runtime_steps.rs`
  - `source/services/netstackd/tests/ipc_parse_reply.rs`
  - `source/services/netstackd/tests/loopback_observability.rs`

## Stabilized contracts expected from this task

- `netstackd` internal module boundaries become explicit and reviewable.
- Existing userspace networking owner semantics remain unchanged while the daemon structure is modularized.
- Phase-1 hardening remains internal and behavior-preserving, not a public wire or API change.
