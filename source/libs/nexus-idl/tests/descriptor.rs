//! CONTEXT: Tests for nexus_interface! macro descriptor generation
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
