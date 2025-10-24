# ADR-0002: Nexus Loader Architecture

Status: Accepted
Date: 2025-01-27
Owners: @runtime

## Context
The system needs a secure, enterprise-grade ELF64/RISC-V loader for user program execution. The loader must handle segment mapping, security validation, and provide both host and OS backends.

## Decision
Implement `userspace/nexus-loader` as the single source of truth for ELF loading with the following architecture:

- **Core API**: `parse_elf64_riscv()` and `load_with()` functions
- **Security**: No W+X segments, page-aligned segments, sorted non-overlapping addresses
- **Backends**: Host mapper for testing, OS mapper for production
- **Error Handling**: Comprehensive error types for all failure modes

## Rationale
- Centralizes ELF loading logic in one place
- Enforces security invariants at the library level
- Provides clear separation between parsing and mapping
- Enables testing without kernel dependencies

## Consequences
- All ELF loading must go through this library
- Kernel user_loader becomes a thin ABI bridge only
- Security constraints are enforced consistently
- Host and OS backends can evolve independently

## Invariants
- No W+X segments allowed (security requirement)
- All segments must be page-aligned (4KB boundary)
- Segments must be sorted by virtual address and non-overlapping
- File size cannot exceed memory size (prevents buffer overflows)

## Implementation Plan
1. Implement core ELF parsing with security validation
2. Create host mapper for testing
3. Create OS mapper for production use
4. Add comprehensive test coverage
5. Deprecate duplicate loader implementations

## References
- `userspace/nexus-loader/src/lib.rs`
- `source/kernel/neuron/src/user_loader.rs`
