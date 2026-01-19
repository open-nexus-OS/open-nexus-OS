---
title: TASK-0157 DSoftBus v1a (host-first): localSim discovery + numeric pairing + reliable msg/byte streams + deterministic tests
status: Draft
owner: @runtime
created: 2025-12-26
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - NexusNet SDK track (cloud + DSoftBus): tasks/TRACK-NEXUSNET-SDK.md
  - DSoftBus overview: docs/distributed/dsoftbus-lite.md
  - DSoftBus no_std refactor: tasks/TASK-0022-dsoftbus-core-no_std-transport-refactor.md
  - Share v2 (intents) later: tasks/TASK-0126-share-v2a-intentsd-registry-dispatch-policy-host.md
  - Persistence substrate (/state): tasks/TASK-0009-persistence-v1-virtio-blk-statefs.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

Networking-backed DSoftBus work (`TASK-0003..0005`) is currently gated on userspace virtio-net + MMIO access (`TASK-0010`).
We still need a deterministic, offline slice that proves:

- discovery UX flows,
- pairing/auth flows,
- reliable stream semantics (message + byte),
- bounded backpressure and error semantics.

This task delivers **localSim** mode: no sockets, no network, one synthetic peer, deterministic behavior.
OS wiring, policy/perms, demo app, and QEMU selftests are in `TASK-0158`.

## Goal

Deliver a DSoftBus v1 local simulation backend with:

1. Discovery:
   - returns `self` + one seeded peer (`peer-sim-01`)
   - deterministic `lastSeenNs` via injected clock
2. Pairing/auth:
   - `pairOffer(peer)` returns a numeric code (6 digits) generated from a deterministic RNG seeded via config
   - `pairAccept(peer, code)` validates and marks as paired in memory
3. Streams:
   - `MsgStream`: bounded FIFO messages, ordered, reliable, backpressure (`WouldBlock/EAGAIN`) when full
   - `ByteStream`: bounded ring buffer with chunking, EOF (`finish`) and abort semantics
   - stable errors: `EPIPE` on closed, explicit abort reason on aborted
4. Registry model (host-only persistence in v1a):
   - define stable on-disk format for peers (JSON)
   - host tests prove round-trip read/write deterministically
   - OS persistence is gated on `/state` and handled in `TASK-0158`
5. Markers (rate-limited):
   - `dsoftbusd: ready`
   - `dsoftbus: discovered n=<n>`
   - `dsoftbus: pair offer peer=<id> code=<xxxxxx>`
   - `dsoftbus: paired peer=<id>`
   - `dsoftbus: msg open peer=<id> ch=<ch>`
   - `dsoftbus: byte open peer=<id> ch=<ch>`

## Non-Goals

- Kernel changes.
- Real network discovery/auth/streams (handled by networking tasks).
- Noise/TLS crypto handshake (localSim pairing is numeric-code gated only; secure channels are a follow-up).
- UI consent prompts (handled in `TASK-0158` via permsd/systemui).

Follow-up note (secure channels + file share):

- Noise-secured channels, encrypted framing, and a quota/resume file-share protocol are tracked as `TASK-0195` (host-first) and `TASK-0196` (devnet UDP discovery gating).

## Constraints / invariants (hard requirements)

- Determinism: seeded RNG + injected clock + stable ordering rules.
- Bounded memory: fixed queue sizes; bounded byte buffers; no unbounded allocations.
- No `unwrap/expect`; no blanket `allow(dead_code)`.
- No fake success: markers only after real state transitions (paired only after accept).

## Red flags / decision points (track explicitly)

- **YELLOW (API drift vs network DSoftBus)**:
  - localSim stream semantics must match the future network backend semantics (open/close/EPIPE/backpressure),
    so higher layers don’t fork.

- **RED (`/state` gating)**:
  - registry persistence in OS builds is gated on `TASK-0009`. Until then, OS must be explicit about non-persistence.

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - Command(s):
    - `cargo test -p dsoftbus_v1_host -- --nocapture` (or equivalent crate name)
  - Required tests:
    - discovery returns deterministic peers
    - pairing code deterministic under seed; accept persists to registry format
    - MsgStream ordering + bounded backpressure
    - ByteStream chunking + EOF + abort semantics
    - registry round-trip survives “restart” (load after save)

## Touched paths (allowlist)

- `source/services/dsoftbusd/` (daemon wiring; localSim engine host-first)
- `userspace/dsoftbus/` (core traits/state machine; align with `TASK-0022`)
- `tests/dsoftbus_v1_host/` (new)
- `docs/dsoftbus/overview.md` (added in `TASK-0158`)

## Plan (small PRs)

1. Define the localSim protocol/state machine + stable errors + deterministic helpers (clock/RNG)
2. Implement MsgStream + ByteStream with bounds + tests
3. Implement registry format + round-trip tests

## Acceptance criteria (behavioral)

- Host tests deterministically prove discovery/pairing/streams/backpressure and registry round-trip.
