//! CONTEXT: Tests for entitlement policy allow/deny lookup
use nexus_sel::{Policy, PolicyEntry};

#[test]
fn unknown_capability_denied() {
    let entries = [PolicyEntry { service_id: 0x10u64, capabilities: &["ability.start"] }];
    let policy = Policy::new(&entries);
    assert!(!policy.allows(0x10, "window.debug"));
}
