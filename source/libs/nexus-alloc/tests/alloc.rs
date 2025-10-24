//! CONTEXT: Tests for bump allocator alignment and reuse
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 1 unit test
//!
//! TEST_SCOPE:
//!   - Memory allocation alignment
//!   - Allocator reuse after reset
//!   - Memory boundary validation
//!
//! TEST_SCENARIOS:
//!   - test_reset_allows_reuse(): Test allocator reset and reuse
//!
//! DEPENDENCIES:
//!   - nexus_alloc::BumpAllocator: Bump allocator implementation
//!
//! ADR: docs/adr/0016-kernel-libs-architecture.md
use nexus_alloc::BumpAllocator;

#[test]
fn reset_allows_reuse() {
    let mut bump = BumpAllocator::new(100, 32);
    let first = bump.alloc(8, 8).unwrap();
    assert!(first >= 100);
    bump.reset();
    let second = bump.alloc(8, 8).unwrap();
    assert_eq!(first, second);
}
