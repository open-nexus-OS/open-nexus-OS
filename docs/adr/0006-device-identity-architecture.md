# ADR-0006: Device Identity Architecture

Status: Accepted
Date: 2025-01-27
Owners: @runtime

## Context
The system needs a robust device identity system for cryptographic signing, authentication, and secure communication.

## Decision
Implement `userspace/identity` as the device identity system with the following architecture:

- **Identity Generation**: Cryptographically secure key generation
- **Device IDs**: Stable identifiers derived from public key hashes
- **Signing**: Ed25519 digital signatures
- **Persistence**: JSON serialization for key storage
- **Verification**: Signature verification against public keys

## Rationale
- Provides secure device identification
- Uses industry-standard Ed25519 signatures
- Enables key persistence and recovery
- Supports both host and OS environments

## Consequences
- All device identities must use this system
- Device IDs are cryptographically derived
- Signing keys are protected in memory
- JSON persistence enables key recovery

## Invariants
- Device IDs are cryptographically derived from public keys (SHA-256 hash)
- Signing keys are generated using cryptographically secure random number generators
- All cryptographic operations use industry-standard algorithms (Ed25519)
- No unsafe code in cryptographic operations
- Signing operations are policy-gated (require `crypto.sign` capability, TASK-0008)
- Private keys never leave keystored (signatures are returned, not key material)

## Policy Integration (TASK-0008)

As of TASK-0008, signing operations are policy-gated:

- `keystored` enforces `crypto.sign` capability via `policyd`
- Policy check binds to `sender_service_id` (kernel-provided, unforgeable)
- Denials are audit-logged via `logd`
- See `docs/rfcs/RFC-0015-policy-authority-audit-baseline-v1.md`

## Implementation Plan
1. Implement Ed25519 key generation
2. Implement device ID derivation
3. Implement signing and verification
4. Implement JSON persistence
5. Add comprehensive test coverage

## References
- `userspace/identity/src/lib.rs`
- `userspace/dsoftbus/src/lib.rs`
