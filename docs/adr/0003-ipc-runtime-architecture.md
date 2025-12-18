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

## Current state (2025-12-18)

This ADR remains directionally correct, but the implementation is currently in a transitional state:

- **Host backend**: implemented (in-process channels), used heavily by `tests/e2e*` and `tests/remote_e2e`.
- **OS-lite backend**: implemented (cooperative mailbox), used for OS bring-up; **not security relevant**.
- **OS “kernel IPC” backend**: wired to kernel IPC v1 syscalls for payload transport, including deadline semantics (RFC-0005).

Routing note (bootstrap):

- The OS backend resolves named targets via an init-lite routing responder. Each service receives
  per-process control endpoint capabilities in deterministic slots (slot 1 = control SEND, slot 2 = control RECV).
  The client sends a `ROUTE_GET` frame containing the target name and receives a `ROUTE_RSP` frame containing
  the capability slots to use for that target. This keeps service code free of hard-coded slot numbers.

To prevent drift between docs and code, the kernel IPC + capability model is now specified in:

- `docs/rfcs/RFC-0005-kernel-ipc-capability-model.md`

This ADR defines the runtime abstraction (`Client`/`Server`/`Wait`), while RFC-0005 defines the
kernel-enforced transport and capability semantics required to make the OS backend real.

## Implementation Plan
1. Implement core Client/Server traits
2. Create host backend using std::sync::mpsc
3. Create OS standard backend using kernel IPC syscalls (see RFC-0005)
4. Create OS-lite backend using mailbox transport
5. Add comprehensive test coverage

## References
- `userspace/nexus-ipc/src/lib.rs`
- `source/services/*/src/main.rs`
