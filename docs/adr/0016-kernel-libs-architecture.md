# ADR-0016: Kernel Libraries Architecture

## Status
Accepted

## Context
The kernel libraries (`nexus-abi`, `nexus-alloc`, `nexus-hal`, `nexus-idl`, `nexus-sched`, `nexus-sel`, `nexus-sync`) provide foundational functionality for the NEURON kernel. These libraries need to be carefully designed to maintain kernel invariants while providing stable interfaces for kernel components.

## Decision
Create a unified architecture for kernel libraries with the following principles:

### Core Libraries
- **nexus-abi**: Shared ABI definitions between kernel and userspace
- **nexus-alloc**: Memory allocation primitives for kernel use
- **nexus-hal**: Hardware abstraction layer for kernel components
- **nexus-idl**: Interface definition language for kernel IPC
- **nexus-sched**: Scheduling primitives and algorithms
- **nexus-sel**: Security enforcement layer for capability-based access
- **nexus-sync**: Synchronization primitives for kernel concurrency

### Design Principles
1. **No_std Compatibility**: All libraries must work in no_std environment
2. **Unsafe Code Control**: Controlled use of unsafe code with clear invariants
3. **ABI Stability**: Stable interfaces between kernel and userspace
4. **Performance Critical**: Optimized for kernel performance requirements
5. **Memory Safety**: Strong memory safety guarantees where possible

### Invariants
- ABI headers maintain 16-byte alignment and little-endian format
- Memory allocators provide deterministic behavior in kernel context
- HAL interfaces abstract architecture-specific details
- Scheduling primitives maintain real-time guarantees
- Security layer enforces capability-based access control
- Synchronization primitives prevent deadlocks and ensure progress

## Consequences
- **Positive**: Clear separation of concerns, stable kernel interfaces, performance optimization
- **Negative**: Increased complexity in kernel development, stricter adherence to invariants
- **Risks**: Breaking ABI compatibility, performance regressions, memory safety violations


