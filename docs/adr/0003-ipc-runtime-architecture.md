# ADR-0003: IPC Runtime Architecture

Status: Accepted
Date: 2025-01-27
Owners: @runtime

## Context
The system needs a robust IPC runtime for cross-process communication with support for different backends (host testing, OS production, embedded).

## Decision
Implement `userspace/nexus-ipc` as the IPC runtime with the following architecture:

- **Traits**: `Client` and `Server` interfaces for bidirectional communication
- **Wait Behavior**: `Blocking`, `NonBlocking`, and `Timeout` modes
- **Backends**: Host (in-process), OS standard (kernel IPC), OS-lite (mailbox)
- **Error Handling**: Comprehensive error types for all failure modes

## Rationale
- Provides consistent IPC interface across all backends
- Enables testing without kernel dependencies
- Supports different deployment scenarios
- Maintains frame boundaries and message integrity

## Consequences
- All IPC operations must use this runtime
- Backend selection is controlled by build configuration
- Timeout handling prevents indefinite blocking
- Peer disconnection is detected and reported

## Invariants
- All IPC operations are memory-safe (no unsafe code)
- Frame boundaries are preserved (no message corruption)
- Timeout handling prevents indefinite blocking
- Peer disconnection is detected and reported

## Implementation Plan
1. Implement core Client/Server traits
2. Create host backend using std::sync::mpsc
3. Create OS standard backend using nexus-abi
4. Create OS-lite backend using mailbox transport
5. Add comprehensive test coverage

## References
- `userspace/nexus-ipc/src/lib.rs`
- `source/services/*/src/main.rs`











