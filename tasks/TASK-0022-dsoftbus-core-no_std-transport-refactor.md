---
title: TASK-0022 DSoftBus core refactor: no_std-compatible core + transport abstraction (unblocks OS backends)
status: Draft
owner: @runtime
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - ADR: docs/adr/0005-dsoftbus-architecture.md
  - DSoftBus overview: docs/distributed/dsoftbus-lite.md
  - Unblocks: tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md
  - Unblocks: tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md
  - Unblocks: tasks/TASK-0021-dsoftbus-quic-v1-host-first-os-scaffold.md
---

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

## Non-Goals

- Implement OS networking (nexus-net) in this task.
- Implement QUIC in this task.

## Constraints / invariants (hard requirements)

- Kernel untouched.
- `dsoftbus-core` must be **`#![no_std]` + `extern crate alloc`** (no `std`).
- No `unwrap`/`expect`; no blanket `allow(dead_code)`.
- Deterministic tests on host for the core state machine.

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
