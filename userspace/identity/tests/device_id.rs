// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Unit tests for DeviceId parsing helpers
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 1 integration test
//!
//! TEST_SCOPE:
//!   - DeviceId::from_hex_sha256 validation
//!
//! TEST_SCENARIOS:
//!   - device_id_rejects_non_hex_or_wrong_len(): invalid ids are rejected deterministically
//!
//! DEPENDENCIES:
//!   - identity::DeviceId
//!
//! ADR: docs/adr/0006-device-identity-architecture.md

use identity::DeviceId;

#[cfg(nexus_env = "host")]
mod host {
    use identity::DeviceId;

    #[test]
    fn device_id_rejects_non_hex_or_wrong_len() {
        assert!(DeviceId::from_hex_sha256("abc").is_err());
        assert!(DeviceId::from_hex_sha256(&"g".repeat(64)).is_err());
        assert!(DeviceId::from_hex_sha256(&"a".repeat(63)).is_err());
        assert!(DeviceId::from_hex_sha256(&"a".repeat(65)).is_err());
        assert!(DeviceId::from_hex_sha256(&"a".repeat(64)).is_ok());
    }
}

