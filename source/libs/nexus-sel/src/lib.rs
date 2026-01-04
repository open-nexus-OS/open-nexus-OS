#![cfg_attr(not(test), no_std)]
//! CONTEXT: Simple entitlement policy lookup for userland
//! OWNERS: @runtime
//! PUBLIC API: PolicyEntry, Policy
//! DEPENDS_ON: core
//! INVARIANTS: Read-only table; constant-time lookup per entry scan
//! ADR: docs/adr/0016-kernel-libs-architecture.md

/// Policy entry describing entitlement decisions.
#[derive(Clone, Copy, Debug)]
pub struct PolicyEntry {
    pub capability: &'static str,
    pub allow: bool,
}

pub struct Policy<'a> {
    entries: &'a [PolicyEntry],
}

impl<'a> Policy<'a> {
    pub const fn new(entries: &'a [PolicyEntry]) -> Self {
        Self { entries }
    }

    pub fn allows(&self, capability: &str) -> bool {
        self.entries
            .iter()
            .find(|entry| entry.capability == capability)
            .map(|entry| entry.allow)
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::{Policy, PolicyEntry};

    #[test]
    fn allow_lookup() {
        let entries = [
            PolicyEntry { capability: "window.manage", allow: true },
            PolicyEntry { capability: "window.debug", allow: false },
        ];
        let policy = Policy::new(&entries);
        assert!(policy.allows("window.manage"));
        assert!(!policy.allows("window.debug"));
        assert!(!policy.allows("missing"));
    }
}
