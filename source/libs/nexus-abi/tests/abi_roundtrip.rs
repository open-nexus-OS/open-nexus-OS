//! CONTEXT: Test for MsgHeader round-trip LE encoding/decoding
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 1 unit test
//!
//! TEST_SCOPE:
//!   - Message header serialization
//!   - Little-endian encoding/decoding
//!   - Round-trip consistency
//!
//! TEST_SCENARIOS:
//!   - test_header_matches(): Test header round-trip serialization
//!
//! DEPENDENCIES:
//!   - nexus_abi::MsgHeader: Message header type
//!
//! ADR: docs/adr/0016-kernel-libs-architecture.md
use nexus_abi::MsgHeader;

#[test]
fn header_matches() {
    let header = MsgHeader::new(1, 2, 3, 4, 5);
    assert_eq!(MsgHeader::from_le_bytes(header.to_le_bytes()), header);
}
