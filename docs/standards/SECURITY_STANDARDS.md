# Security Standards

This document defines the security standards, guidelines, and best practices for the Open Nexus OS project.

**Status**: Active (2026-01-07)  
**Audience**: All developers, agents, reviewers

---

## Core Security Philosophy

Open Nexus OS follows a **security-by-design** approach inspired by seL4 and Fuchsia:

1. **Kernel minimal**: Policy, crypto, and complex logic stay in userspace
2. **Capability-based access**: No ambient authority; all access requires explicit capabilities
3. **Deny-by-default**: Operations without explicit policy allow are rejected
4. **Single authority**: Security decisions go through `policyd` (no bypass paths)
5. **Channel-bound identity**: Trust kernel-provided `sender_service_id`, never payload strings
6. **Audit everything**: All security-relevant decisions produce audit records

---

## Security Invariants (MUST Hold)

These invariants MUST be maintained across all code. Violations are **security bugs**.

### 1. Secrets Management

```
✅ DO:
- Store secrets only in keystored
- Use signing APIs (return signatures, not keys)
- Label test keys: // SECURITY: bring-up test keys, NOT production custody
- Use deterministic keys only in bring-up/test builds

❌ DON'T:
- Log plaintext secrets, private keys, or credentials
- Include keys in error messages or UART output
- Use deterministic/hardcoded keys in production
- Store secrets in plaintext files outside /state/keystore/
```

### 2. Identity and Authentication

```
✅ DO:
- Use sender_service_id from kernel IPC (unforgeable)
- Verify identity binding (device_id ↔ noise_static_pub)
- Complete authentication before processing application data
- Reject sessions on identity mismatch

❌ DON'T:
- Trust identity strings from payload bytes
- Skip authentication for "trusted" networks or localhost
- Allow "warn and continue" on identity verification failure
- Log session keys or derived secrets
```

### 3. Input Validation

```
✅ DO:
- Bound all input sizes (max lengths enforced)
- Validate format before parsing
- Reject malformed input deterministically (no crash)
- Use explicit error handling (match, not unwrap)
- Add #[must_use] on security-critical error types

❌ DON'T:
- Use unwrap/expect on untrusted input
- Accept unbounded input sizes
- Crash on malformed input (DoS vector)
- Trust caller-provided sizes or lengths
- Silently ignore security errors (use #[must_use])
```

**Error handling discipline** (TASK-11B principles):

All error types representing **security decisions** MUST be marked `#[must_use]`:

```rust
/// Permission denial MUST be handled (security-critical)
#[must_use = "permission errors must be handled"]
pub enum AuthError {
    PermissionDenied,
    InvalidCredential,
    SessionExpired,
}

/// Input validation MUST be checked (prevents injection)
#[must_use = "validation errors must be checked"]
pub enum ValidateError {
    OversizedInput,
    Malformed,
    InvalidEncoding,
}
```

This ensures the compiler **prevents accidental silent failures** in security-critical paths.

### 4. Capability and Policy

```
✅ DO:
- Route all sensitive operations through policyd
- Use deny-by-default policies
- Audit all allow/deny decisions
- Scope capabilities to minimum required

❌ DON'T:
- Duplicate policy logic in multiple services
- Grant ambient capabilities
- Skip policy checks for "trusted" services
- Allow runtime policy modification
```

### 5. Memory and Mapping

```
✅ DO:
- Enforce W^X (never RWX mappings)
- Bound MMIO mappings to device windows
- Use capability-gated device access
- Validate all VMO sizes and offsets

❌ DON'T:
- Allow executable mappings of device memory
- Grant MMIO capabilities to arbitrary services
- Map outside designated physical windows
- Skip capability checks for device access
```

---

## Security-Relevant Tasks

Tasks are considered **security-relevant** if they touch:

| Category | Examples | Requires Security Section |
|----------|----------|---------------------------|
| **Crypto/Auth** | Noise XK, key derivation, signatures | ✅ Yes |
| **Network** | Discovery, sessions, remote proxy | ✅ Yes |
| **IPC/Caps** | Capability transfer, policy enforcement | ✅ Yes |
| **Kernel boundary** | Syscalls, MMIO, memory mapping | ✅ Yes |
| **Sensitive data** | Secrets, credentials, PII | ✅ Yes |
| **Updates/Packaging** | Code signing, verification | ✅ Yes |
| **Persistence** | Key storage, credential storage | ✅ Yes |
| **UI/UX** | Layout, colors, animations | ❌ No |
| **Docs only** | README updates, comments | ❌ No |

For security-relevant tasks, the task template includes:

1. **Security considerations** (optional, N/A if not relevant)
   - Threat model
   - Security invariants
   - DON'T DO list
   - Attack surface impact
   - Mitigations

2. **Security proof** (for security-relevant tasks only)
   - Audit tests (negative cases / attack simulation)
   - Hardening markers (QEMU)
   - Fuzz coverage (optional)

---

## Security Testing Requirements

### Negative Case Tests

Security-relevant code MUST include tests that verify rejection of invalid inputs:

```rust
// Example: test_reject_* pattern
#[test]
fn test_reject_identity_mismatch() {
    // Provide wrong key, verify session is rejected
    let result = handshake_with_wrong_key();
    assert!(result.is_err());
    assert!(matches!(result, Err(AuthError::IdentityMismatch)));
}

#[test]
fn test_reject_oversized_input() {
    // Provide input exceeding max size
    let oversized = vec![0u8; MAX_INPUT_SIZE + 1];
    let result = parse_packet(&oversized);
    assert!(result.is_err());
}
```

### Hardening Markers (QEMU)

Security behavior MUST be verifiable via QEMU markers:

```
# Successful security checks
dsoftbusd: auth ok
dsoftbusd: identity bound peer=<id>

# Security rejections (prove enforcement works)
dsoftbusd: identity mismatch peer=<id>
dsoftbusd: announce ignored (malformed)
policyd: deny (subject=<svc> action=<op>)
keystored: sign denied (subject=<svc>)
```

### Fuzz Testing (Recommended)

For parsing and protocol code, fuzz testing is recommended:

```bash
# Discovery packet fuzzing
cargo +nightly fuzz run fuzz_discovery_packet

# Noise handshake fuzzing
cargo +nightly fuzz run fuzz_noise_handshake

# Policy rule parsing
cargo +nightly fuzz run fuzz_policy_parser
```

---

## Code Review Security Checklist

Reviewers MUST verify for security-relevant PRs:

### Identity and Auth
- [ ] No payload-provided identity trusted
- [ ] `sender_service_id` used for all policy decisions
- [ ] Identity binding verified before `auth ok`
- [ ] No "warn and continue" on auth failure

### Secrets
- [ ] No secrets in logs, markers, or error messages
- [ ] Test keys labeled `// SECURITY: bring-up test keys`
- [ ] No deterministic keys in production paths

### Input Handling
- [ ] All inputs bounded (max sizes enforced)
- [ ] No `unwrap`/`expect` on untrusted data
- [ ] Malformed input handled gracefully (no crash)

### Policy
- [ ] Sensitive ops go through `policyd`
- [ ] Deny-by-default behavior verified
- [ ] Audit records produced for decisions

### Memory
- [ ] No executable device mappings
- [ ] MMIO bounded to device window
- [ ] Capability checks present

---

## Bring-up vs Production Security

Some security shortcuts are acceptable during bring-up, but MUST be:

1. **Explicitly labeled** in code:
   ```rust
   // SECURITY: bring-up test keys, NOT production custody
   let key = derive_test_secret(port);
   ```

2. **Feature-gated** for removal in production:
   ```rust
   #[cfg(feature = "bring-up")]
   fn derive_test_secret(port: u16) -> [u8; 32] { ... }
   ```

3. **Documented in the task** with migration path:
   ```markdown
   ### Security considerations
   - Test keys used for bring-up (not production)
   - Migration: TASK-0008 integrates with keystored
   ```

---

## Security Contact

For security-sensitive issues:

- **Internal**: Use `RED (blocking)` in task/RFC red flags
- **Review**: Tag security-relevant PRs with `security-review-required`
- **Escalation**: Contact @runtime for critical security decisions

---

## Related Documents

- `BUILD_STANDARDS.md` — Build hygiene and `no_std` dependency rules
- `DOCUMENTATION_STANDARDS.md` — Documentation requirements
- `docs/rfcs/RFC-0005-kernel-ipc-capability-model.md` — Capability model
- `docs/rfcs/RFC-0008-dsoftbus-noise-xk-v1.md` — Noise XK identity binding
- `tasks/TASK-TEMPLATE.md` — Task template with security sections
