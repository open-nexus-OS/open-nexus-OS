# ADR-0005: DSoftBus-lite Architecture

Status: Accepted
Date: 2025-01-27
Owners: @runtime

## Context

The system needs a distributed service fabric for service discovery, authenticated sessions, and reliable communication across devices.

## Decision

Implement `userspace/dsoftbus` as the distributed service fabric with the following architecture:

- **Discovery**: Service announcement and peer discovery
- **Authentication**: Noise protocol for secure handshakes
- **Sessions**: Authenticated connections with device identity
- **Streams**: Reliable framed communication with channel multiplexing
- **Backends**: Host (TCP), OS (userspace sockets facade via `netstackd`)

## Rationale

- Provides secure distributed communication
- Uses industry-standard Noise protocol
- Supports service discovery and session management
- Enables testing without kernel dependencies

## Consequences

- All distributed communication must use this fabric
- Device identities are cryptographically bound
- Handshake proofs prevent man-in-the-middle attacks
- Frame boundaries are preserved across network transport

## Invariants

- All communication uses Noise protocol for authenticated encryption
- Device identities are cryptographically bound to signing keys
- Handshake proofs prevent man-in-the-middle attacks
- Frame boundaries are preserved across network transport

## Implementation Plan

1. ✅ Implement discovery interface and backends (host-first)
2. ✅ Implement Noise protocol handshake (host + OS)
3. ✅ Implement authenticated sessions (loopback scope)
4. ✅ Implement discovery-driven connect + identity binding (TASK-0004)
5. ✅ Implement cross-VM discovery/session + minimal remote proxy (TASK-0005, opt-in)
6. ⬜ Implement reliable streams with multiplexing (TASK-0020+)
7. ✅ Add comprehensive test coverage (host tests green)

## Implementation Status (2026-01-13)

| Component | Host | OS | Task |
| --------- | ---- | -- | ---- |
| Discovery (announce/receive) | ✅ | ✅ (loopback) | TASK-0003C |
| Noise XK handshake | ✅ | ✅ | TASK-0003B |
| TCP sessions | ✅ | ✅ (loopback) | TASK-0003 |
| Discovery-driven connect | ✅ | ✅ | TASK-0004 |
| Identity binding enforcement | ✅ | ✅ | TASK-0004 |
| Dual-node proof | ✅ | ✅ | TASK-0004 |
| Cross-VM sessions (2× QEMU, opt-in) | ✅ | ✅ | TASK-0005 |
| Remote proxy (`samgrd`/`bundlemgrd`, deny-by-default) | ✅ | ✅ | TASK-0005 |

**Cross-VM harness contract (opt-in)**:

- Canonical runner: `tools/os2vm.sh`
- Contract seed: `docs/rfcs/RFC-0010-dsoftbus-cross-vm-harness-v1.md`
- Testing guide: `docs/testing/index.md`

**RFC Contracts**:

- RFC-0007: DSoftBus OS Transport v1
- RFC-0008: DSoftBus Noise XK v1
- RFC-0009: no_std Dependency Hygiene v1

## References

- `userspace/dsoftbus/src/lib.rs` (host-first library)
- `source/services/dsoftbusd/` (OS daemon)
- `source/libs/nexus-noise-xk/` (no_std Noise XK)
- `source/libs/nexus-discovery-packet/` (no_std discovery packet)
- `source/libs/nexus-peer-lru/` (no_std peer LRU)
- `tools/os2vm.sh` (2-VM harness)
