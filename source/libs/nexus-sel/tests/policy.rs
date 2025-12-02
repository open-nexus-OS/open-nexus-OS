//! CONTEXT: Tests for entitlement policy allow/deny lookup
use nexus_sel::{Policy, PolicyEntry};

#[test]
fn unknown_capability_denied() {
    let entries = [PolicyEntry {
        capability: "ability.start",
        allow: true,
    }];
    let policy = Policy::new(&entries);
    assert!(!policy.allows("window.debug"));
}
