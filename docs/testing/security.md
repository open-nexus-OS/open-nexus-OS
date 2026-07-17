<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Security testing and fuzzing

Security-specific testing requirements: `test_reject_*` negative cases, hardening markers, fuzz targets, and the review checklist. Split out of the former `docs/testing/index.md`; see [README.md](README.md) for the entry point.

## Security testing

Security-relevant code requires additional testing beyond functional tests. See `docs/standards/SECURITY_STANDARDS.md` for full guidelines.

### Negative case tests (required for security code)

Security code MUST include tests that verify rejection of invalid/malicious inputs:

```rust
// Pattern: test_reject_* functions
#[test]
fn test_reject_identity_mismatch() {
    // Attempt auth with wrong key → verify rejection
    let result = handshake_with_wrong_key();
    assert!(result.is_err());
}

#[test]
fn test_reject_oversized_input() {
    let oversized = vec![0u8; MAX_SIZE + 1];
    let result = parse(&oversized);
    assert!(result.is_err());
}
```

Run security-specific tests:

```bash
# All reject tests across workspace
cargo test -- reject --nocapture

# Specific crate security tests
cargo test -p dsoftbusd -- reject
cargo test -p keystored -- reject
cargo test -p nexus-sel -- reject
```

### Hardening markers (QEMU)

Security behavior must be verifiable via QEMU markers that prove enforcement:

| Marker | Meaning |
| --- | --- |
| `dsoftbusd: auth ok` | Handshake + identity binding succeeded |
| `dsoftbusd: identity mismatch peer=<id>` | Identity binding enforcement works |
| `dsoftbusd: announce ignored (malformed)` | Input validation works |
| `policyd: deny (subject=<svc> action=<op>)` | Policy deny-by-default works |
| `policyd: allow (subject=<svc> action=<op>)` | Explicit allow logged |
| `policyd: audit emit ok` | Audit record successfully emitted to logd |
| `keystored: sign denied (subject=<svc>)` | Policy-gated signing works |
| `SELFTEST: policy deny audit ok` | Deny decision + audit record proven |
| `SELFTEST: policy allow audit ok` | Allow decision + audit record proven |
| `SELFTEST: keystored sign denied ok` | Policy-gated signing denied without required capability |

### Fuzz testing (recommended for parsers)

For parsing and protocol code:

```bash
# Install cargo-fuzz if needed
cargo install cargo-fuzz

# Run fuzz targets (if available)
cargo +nightly fuzz run fuzz_discovery_packet
cargo +nightly fuzz run fuzz_noise_handshake
cargo +nightly fuzz run fuzz_policy_parser
```

### Security review checklist

Before merging security-relevant PRs, verify:

- [ ] No secrets in logs, markers, or error messages
- [ ] Test keys labeled `// SECURITY: bring-up test keys`
- [ ] `sender_service_id` used (not payload strings) for identity
- [ ] Inputs bounded (max sizes enforced)
- [ ] No `unwrap`/`expect` on untrusted data
- [ ] Audit records produced for security decisions
- [ ] Negative case tests (`test_reject_*`) included
