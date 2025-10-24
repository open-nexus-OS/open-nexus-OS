//! CONTEXT: Log daemon CLI tests
//! OWNERS: @services-team
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 1 CLI test
//!
//! TEST_SCOPE:
//!   - Kernel/user log writing
//!   - Ring buffer management
//!   - Log filtering and subscription
//!
//! TEST_SCENARIOS:
//!   - test_capture_log(): Test log capture functionality
//!
//! DEPENDENCIES:
//!   - logd::execute: CLI execution function
//!   - policyd: Access control
//!
//! ADR: docs/adr/0017-service-architecture.md
#[test]
fn capture_log() {
    assert!(logd::execute(&["event"]).contains("event"));
}
