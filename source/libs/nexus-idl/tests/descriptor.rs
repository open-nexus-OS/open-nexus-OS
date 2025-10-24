//! CONTEXT: Tests for nexus_interface! macro descriptor generation
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 1 unit test
//!
//! TEST_SCOPE:
//!   - Macro expansion validation
//!   - Descriptor generation
//!   - Interface trait creation
//!
//! TEST_SCENARIOS:
//!   - test_descriptor_generation(): Test macro descriptor generation
//!
//! DEPENDENCIES:
//!   - nexus_idl::nexus_interface: Interface definition macro
//!
//! ADR: docs/adr/0016-kernel-libs-architecture.md
use nexus_idl::nexus_interface;

nexus_interface!(interface sample {
    fn hello(&self) -> ();
});

struct Impl;

impl sample::Service for Impl {
    fn hello(&self) -> () {
        ()
    }
}

#[test]
fn descriptor_contains_hello() {
    assert_eq!(sample::descriptor(), ["hello"]);
}
