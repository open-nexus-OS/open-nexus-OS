# ADR-0004: IDL Runtime Architecture

Status: Accepted
Date: 2025-01-27
Owners: @runtime

## Context
The system needs a robust IDL runtime for control-plane messaging using Cap'n Proto with support for multiple service interfaces.

## Decision
Implement `userspace/nexus-idl-runtime` as the IDL runtime with the following architecture:

- **Generated Modules**: Auto-generated Cap'n Proto bindings for each service
- **Service Interfaces**: samgr, bundlemgr, vfs, packagefs, keystored, identity, dsoftbus, policyd, execd
- **Error Handling**: Common `IdlError` type for serialization failures
- **Feature Gates**: Conditional compilation based on `capnp` feature

## Rationale
- Centralizes IDL serialization logic
- Provides type-safe interfaces for all services
- Enables conditional compilation for different builds
- Maintains ABI stability through stable module names

## Consequences
- All IDL operations must use generated bindings
- Service interfaces are defined in .capnp schema files
- Serialization errors are handled consistently
- Build system controls which bindings are included

## Invariants
- No unsafe code in generated bindings
- All serialization is bounds-checked
- Message validation prevents malformed data processing
- Stable module names prevent ABI breakage

## Implementation Plan
1. Define .capnp schema files for each service
2. Generate Rust bindings using capnp
3. Implement common error handling
4. Add feature gates for conditional compilation
5. Add comprehensive test coverage

## References
- `userspace/nexus-idl-runtime/src/lib.rs`
- `tools/nexus-idl/schemas/*.capnp`











