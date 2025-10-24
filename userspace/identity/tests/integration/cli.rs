// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Integration tests for identity CLI functionality
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 1 integration test
//!
//! TEST_SCOPE:
//!   - Identity generation and serialization
//!   - Cryptographic signing and verification
//!   - JSON roundtrip operations
//!   - Key management
//!
//! TEST_SCENARIOS:
//!   - test_sign_and_verify_via_json_roundtrip(): Test identity generation, serialization, and signing
//!
//! DEPENDENCIES:
//!   - identity::Identity: Identity management functionality
//!   - Ed25519 cryptographic operations
//!   - JSON serialization/deserialization
//!
//! ADR: docs/adr/0006-device-identity-architecture.md

use identity::Identity;

#[test]
fn sign_and_verify_via_json_roundtrip() {
    let identity = Identity::generate().expect("identity generation");
    let exported = identity.to_json().expect("serialize");
    let restored = Identity::from_json(&exported).expect("deserialize");

    let payload = b"integration";
    let signature = restored.sign(payload);
    assert!(Identity::verify_with_key(&restored.verifying_key(), payload, &signature));
}
