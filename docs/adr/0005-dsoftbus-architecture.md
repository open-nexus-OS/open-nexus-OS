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
- **Backends**: Host (TCP), OS (kernel transport)

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
1. Implement discovery interface and backends
2. Implement Noise protocol handshake
3. Implement authenticated sessions
4. Implement reliable streams with multiplexing
5. Add comprehensive test coverage

## References
- `userspace/dsoftbus/src/lib.rs`
- `userspace/identity/src/lib.rs`

