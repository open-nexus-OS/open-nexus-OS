---
title: TASK-0022 DSoftBus core refactor: no_std-compatible core + transport abstraction (unblocks OS backends)
status: Draft
owner: @runtime
created: 2025-12-22
depends-on:
  - TASK-0015
follow-up-tasks: []
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - ADR: docs/adr/0005-dsoftbus-architecture.md
  - RFC (modular daemon boundary): docs/rfcs/RFC-0027-dsoftbusd-modular-daemon-structure-v1.md
  - DSoftBus overview: docs/distributed/dsoftbus-lite.md
  - Depends-on (modularization base): tasks/TASK-0015-dsoftbusd-refactor-v1-modular-os-daemon-structure.md
  - Unblocks: tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md
  - Related baseline: tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md
  - Unblocks: tasks/TASK-0021-dsoftbus-quic-v1-host-first-os-scaffold.md
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

- Host: task-owned requirement suites for core state machine and transport trait behavior.
- OS: `cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os`
- Build hygiene: `cd /home/jenning/open-nexus-OS && just dep-gate && just diag-os`
- Regression: `cd /home/jenning/open-nexus-OS && just test-e2e && just test-os-dhcp`
- Release evidence review (if distributed behavior is asserted): `artifacts/os2vm/runs/<runId>/summary.{json,txt}` and `artifacts/os2vm/runs/<runId>/release-evidence.json`

## Context

- OS userland bundles are `#![no_std]` (example: `userspace/apps/demo-exit0/src/lib.rs`).
- `userspace/dsoftbus` is currently **std-based** and its OS backend is a **placeholder** (`userspace/dsoftbus/src/os.rs`).
  Note: OS bring-up streams exist via os-lite services (`netstackd` + `dsoftbusd`) as of TASK-0005, but
  they are not yet factored into a reusable no_std-capable core/backend split.

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

### DON'T DO

- DON'T move policy authorization decisions into generic transport core.
- DON'T accept unauthenticated payload strings as identity inputs.
- DON'T introduce hidden `std` dependencies in no_std core paths.

### Required negative tests

- `test_reject_invalid_state_transition`
- `test_reject_nonce_mismatch_or_stale_reply`
- `test_reject_oversize_frame_or_record`
- `test_reject_unauthenticated_message_path`

## Security proof

### Audit tests (negative cases / attack simulation)

- Commands:
  - `cargo test -p dsoftbus -- reject --nocapture`
- Required tests:
  - `test_reject_invalid_state_transition`
  - `test_reject_nonce_mismatch_or_stale_reply`
  - `test_reject_oversize_frame_or_record`
  - `test_reject_unauthenticated_message_path`

### Hardening markers (QEMU, if applicable)

- `dsoftbusd: ready`
- `dsoftbusd: auth ok`

## Red flags / decision points

- **RED**: As long as DSoftBus depends on `std` types (`std::net::*`, `TcpStream`, `std::sync`), OS transports cannot be real.
- **YELLOW**: Crypto crates (Noise/TLS) may have `std` assumptions; pick `no_std`-capable dependencies or isolate them behind feature gates.

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
