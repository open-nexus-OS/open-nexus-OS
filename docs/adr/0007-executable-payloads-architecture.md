# ADR-0007: Executable Payloads Architecture

Status: Accepted
Date: 2025-01-27
Owners: @runtime

## Context
The system needs executable payloads for testing execd functionality and bootstrap processes.

## Decision
Implement `userspace/exec-payloads` as the executable payloads system with the following architecture:

- **Prebuilt ELF**: HELLO_ELF binary for testing
- **Bootstrap Messages**: Structured data passed from kernel to child
- **Entry Points**: Child process entry points with bootstrap handling
- **UART Markers**: Stable markers for test automation
- **Backends**: Host (pure Rust), OS (kernel syscalls)

## Rationale
- Provides test payloads for execd functionality
- Enables bootstrap process testing
- Maintains stable UART markers for automation
- Supports both host and OS environments

## Consequences
- All test payloads must use this system
- Bootstrap messages are structured and validated
- UART markers remain stable across changes
- Kernel integration is minimal and safe

## Invariants
- No unsafe code in host builds
- No direct MMIO access (uses kernel syscalls)
- Bootstrap message validation prevents null pointer dereference
- Stable UART markers for test automation

## Implementation Plan
1. Implement prebuilt ELF binary
2. Implement bootstrap message structure
3. Implement child entry points
4. Implement UART marker output
5. Add comprehensive test coverage

## References
- `userspace/exec-payloads/src/lib.rs`
- `source/services/execd/src/main.rs`









