//! CONTEXT: Tests for userland scheduler deadline helper
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 1 unit test
//!
//! TEST_SCOPE:
//!   - Deadline calculation
//!   - Time scaling
//!   - Expiration checking
//!
//! TEST_SCENARIOS:
//!   - test_deadline_from_ms_scales(): Test deadline time scaling
//!
//! DEPENDENCIES:
//!   - nexus_sched::Deadline: Deadline type
//!
//! ADR: docs/adr/0016-kernel-libs-architecture.md
use nexus_sched::Deadline;

#[test]
fn deadline_from_ms_scales() {
    let d = Deadline::from_ms(2);
    assert_eq!(d.ticks, 2000);
}
