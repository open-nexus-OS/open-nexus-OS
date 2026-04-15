---
title: TASK-0022 DSoftBus core refactor: no_std-compatible core + transport abstraction (unblocks OS backends)
status: In Progress
owner: @runtime
created: 2025-12-22
depends-on:
  - TASK-0015
  - TASK-0021
follow-up-tasks:
  - TASK-0023
  - TASK-0044
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - ADR: docs/adr/0005-dsoftbus-architecture.md
  - RFC (seed contract): docs/rfcs/RFC-0036-dsoftbus-core-no-std-transport-abstraction-v1.md
  - Production gate track: tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md
  - RFC (modular daemon boundary): docs/rfcs/RFC-0027-dsoftbusd-modular-daemon-structure-v1.md
  - RFC (host QUIC scaffold baseline): docs/rfcs/RFC-0035-dsoftbus-quic-v1-host-first-os-scaffold.md
  - DSoftBus overview: docs/distributed/dsoftbus-lite.md
  - Depends-on (modularization base): tasks/TASK-0015-dsoftbusd-refactor-v1-modular-os-daemon-structure.md
  - Related baseline: tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md
  - Related baseline: tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md
  - Related baseline (Done): tasks/TASK-0021-dsoftbus-quic-v1-host-first-os-scaffold.md
  - Unblocks: tasks/TASK-0023-dsoftbus-quic-v2-os-enabled-gated.md
  - Testing methodology: docs/testing/index.md
  - Testing contract: scripts/qemu-test.sh
  - Testing contract (2-VM): tools/os2vm.sh
---

## Short description

- **Scope**: Split DSoftBus into a `no_std + alloc` core and transport/back-end adapters.
- **Deliver**: Deterministic core state-machine tests, explicit transport traits, and an OS-compilable core boundary.
- **Out of scope**: Implementing QUIC or new OS networking features in this task.

## Production Closure Phases (RFC-0034 alignment)

This task follows the shared production gate profile (`Core + Performance`) from `RFC-0034`.
No phase may be marked green without the linked proof evidence.

- **Phase A (Contract lock)**: lock no_std core boundaries, trait contracts, and fail-closed error model.
- **Phase B (Host proof)**: requirement-named host tests (including reject paths) are green.
- **Phase C (OS-gated proof)**: OS build integration and marker ladder are green where applicable.
- **Phase D (Performance gate)**: deterministic budget checks for core path overhead and backpressure are met.
- **Phase E (Closure & handoff)**: docs/testing + board/order + RFC state are synchronized with proof evidence, and for distributed claims the `tools/os2vm.sh` release artifacts are reviewed (`summary.{json,txt}` + `release-evidence.json`).

Canonical gate commands:

- Host baseline freeze (must stay green while refactoring): `cd /home/jenning/open-nexus-OS && just test-dsoftbus-quic`
- Host: task-owned requirement suites for core state machine and transport trait behavior.
- OS: `cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`
- Build hygiene: `cd /home/jenning/open-nexus-OS && just dep-gate && just diag-os`
- Dependency hygiene: `cd /home/jenning/open-nexus-OS && just deny-check`
- Regression: `cd /home/jenning/open-nexus-OS && just test-e2e && just test-os-dhcp`
- Release evidence review (if distributed behavior is asserted): `artifacts/os2vm/runs/<runId>/summary.{json,txt}` and `artifacts/os2vm/runs/<runId>/release-evidence.json`

## Program alignment (TRACK production gates)

- This task executes in the `DSoftBus & Distributed` `production-floor` gate family from `tasks/TRACK-PRODUCTION-GATES-KERNEL-SERVICES.md`.
- Language policy for this slice: use `production-class` quality wording for DSoftBus closure claims; avoid broad `production-ready` overclaims for the distributed stack.
- `TASK-0021` closure is frozen baseline: host QUIC proof, strict fail-closed semantics, and deterministic OS fallback markers must not regress during this refactor.
- Primary closure intent here is architectural: extract reusable `no_std` core seams that unblock `TASK-0023` without regressing existing host/OS behavior contracts.

## Context

- OS userland bundles are `#![no_std]` (example: `userspace/apps/demo-exit0/src/lib.rs`).
- `userspace/dsoftbus` is currently **std-based** and its OS backend is a **placeholder** (`userspace/dsoftbus/src/os.rs`).
  Note: OS bring-up streams exist via os-lite services (`netstackd` + `dsoftbusd`) as of TASK-0005, but
  they are not yet factored into a reusable no_std-capable core/backend split.
- `TASK-0021` is now `Done` with a real host QUIC transport path (`auto|tcp|quic`) and deterministic fallback proofs; this task must preserve those externally visible contracts while refactoring internals.

This blocks any “OS transport ON” work (including QUIC over UDP): we need a DSoftBus core that can run in OS.

Scope note:

- A deterministic, offline “localSim” DSoftBus slice (discovery + pairing + msg/byte streams) is tracked as
  `TASK-0157` (host-first) and `TASK-0158` (OS wiring + demos). That work should align with this refactor:
  the localSim backend is a good first no_std-capable backend to remove `todo!()` placeholders without requiring networking/MMIO.

## Goal

Make the DSoftBus “core protocol + state machine” usable in OS builds by:

- separating core logic from host networking,
- introducing a minimal transport trait that can be implemented by host TCP and OS nexus-net UDP/TCP,
- removing `todo!()` placeholders in OS backend by replacing them with a real adapter boundary (even if the first OS impl stays ENOTSUP).

## Target-state alignment (post TASK-0015 / RFC-0027)

- Extraction boundaries should mirror stabilized daemon seams:
  - transport adapter surface,
  - discovery/session state machine ownership,
  - gateway/protocol payload handling.
- Avoid pulling policy/gateway decisions into transport-core; keep security boundaries explicit and layered.
- `dsoftbusd` integration after this split should reduce daemon-local duplication, not re-monolithize logic.

## Non-Goals

- Implement OS networking (nexus-net) in this task.
- Implement QUIC in this task.

## Constraints / invariants (hard requirements)

- Kernel untouched.
- `dsoftbus-core` must be **`#![no_std]` + `extern crate alloc`** (no `std`).
- No `unwrap`/`expect`; no blanket `allow(dead_code)`.
- Deterministic tests on host for the core state machine.
- `TASK-0021` host/selection/fallback proof contracts remain green throughout refactor (`just test-dsoftbus-quic` must stay green).
- Zero-copy-first discipline: control-path framing may copy bounded metadata, but bulk payload path must prefer borrowed buffers / VMO-backed/filebuffer-style transfer over eager reallocation/copy chains.
- Rust API discipline for new core boundaries: use `newtype`s for domain IDs/handles, `#[must_use]` on decision-bearing return values, explicit ownership transfer semantics, and reviewed `Send`/`Sync` behavior (no unsafe blanket trait shortcuts).
- OS proof runs must stay aligned with modern virtio-mmio defaults (no legacy-mode dependency for success claims).

## Security considerations

### Threat model

- Contract drift during split weakens identity/session validation paths.
- Nonce/reply-correlation behavior regresses when moving logic into shared core.
- `std`-only convenience APIs sneak into OS-facing paths and bypass deterministic bounds/error handling.

### Security invariants (MUST hold)

- Identity/auth decisions remain fail-closed and transport-agnostic.
- Correlation/state-machine transitions remain deterministic and bounded (no unbounded wait loops).
- Core parsing/frame handling enforces explicit size bounds.
- No secret/session material is emitted in logs/errors during core/backend boundaries.
- Service identity binding remains channel-authoritative (`sender_service_id` from IPC metadata), never payload-derived.
- Ownership and concurrency boundaries remain explicit: cross-thread/session state sharing must preserve safety invariants without hidden aliasing assumptions.

### DON'T DO

- DON'T move policy authorization decisions into generic transport core.
- DON'T accept unauthenticated payload strings as identity inputs.
- DON'T introduce hidden `std` dependencies in no_std core paths.
- DON'T relax strict downgrade/auth reject behavior that was proven in `TASK-0021`.
- DON'T reintroduce copy-heavy hot paths where zero-copy-capable buffers are available.
- DON'T use unsafe `Send`/`Sync` impl shortcuts to force concurrency compatibility.

### Required negative tests

- `test_reject_invalid_state_transition`
- `test_reject_nonce_mismatch_or_stale_reply`
- `test_reject_oversize_frame_or_record`
- `test_reject_unauthenticated_message_path`
- `test_reject_payload_identity_spoof_vs_sender_service_id`

## Security proof

### Audit tests (negative cases / attack simulation)

- Commands:
  - `cargo test -p dsoftbus -- reject --nocapture`
- Required tests:
  - `test_reject_invalid_state_transition`
  - `test_reject_nonce_mismatch_or_stale_reply`
  - `test_reject_oversize_frame_or_record`
  - `test_reject_unauthenticated_message_path`
  - `test_reject_payload_identity_spoof_vs_sender_service_id`

### Hardening markers (QEMU, if applicable)

- `dsoftbusd: ready`
- `dsoftbusd: auth ok`

## Red flags / decision points

- **RED**:
  - none at task-entry. The prior `std` coupling is now this task's primary execution scope, not an external blocker.
- **YELLOW**:
  - Crypto crates (Noise/TLS) may have `std` assumptions; choose `no_std`-capable dependencies or isolate via feature-gated adapters.
  - Refactor churn can silently regress `TASK-0021` externally visible behavior; keep baseline host QUIC suites and fallback marker gates in the mandatory regression floor.

## Touched paths (allowlist)

- `userspace/dsoftbus/` (split into `dsoftbus-core` + host backend crate)
- `docs/distributed/` (document new crate boundaries)
- `tests/` (core host tests)

## Stop conditions (Definition of Done)

- Host: `cargo test` for the new core crate passes deterministically.
- Build: OS target can compile `dsoftbus-core` (no `std`).
- Documentation clearly explains:
  - what is “core” vs “backend”
  - what is required for an OS backend (UDP, timers, entropy/identity).
- Security gate:
  - `test_reject_*` coverage for parser/state/correlation invariants exists and is green.
- Baseline contract preservation:
  - `just test-dsoftbus-quic` remains green (no regression of `TASK-0021` host QUIC/selection/fallback proofs).
  - `just deny-check` remains green under strict policy (`multiple-versions = "deny"` with only narrow compatibility skips).
- Rust/zero-copy discipline:
  - New core boundary types apply `newtype`/ownership/`#[must_use]` where decision-safety requires it.
  - `Send`/`Sync` expectations for core/session types are documented and verified without unsafe blanket trait impls.
  - Data-path changes document why copies are unavoidable where zero-copy is not possible.
- OS proof discipline:
  - if OS integration hooks are touched, run single-VM + 2-VM proofs sequentially (`scripts/qemu-test.sh`, `tools/os2vm.sh`).

## Plan (small PRs)

1. Extract no_std core state-machine modules and lock transport-neutral contracts.
2. Add requirement-named host tests for state, correlation, bounds, and reject paths.
3. Integrate backend adapters and run build/proof gates (`dep-gate`, `diag-os`, OS/2-VM when touched).

## Alignment note (2026-02, low-drift)

- OS-lite runtime now uses an explicit session FSM + `EpochId` ownership boundary in `dsoftbusd` for
  reconnect-safe lifecycle control.
- Transport-facing behavior remains bounded and deterministic (`WouldBlock` + capped retries); no kernel API
  expansion was introduced.
- A narrow transport adapter boundary (`connect/accept/read/write/close/readiness`) now exists in the daemon and
  should be the extraction seam for this task's no_std core/backend split.
