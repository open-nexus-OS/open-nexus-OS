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
    pub service_id: u64,
    pub capabilities: &'static [&'static str],
}

pub struct Policy<'a> {
    entries: &'a [PolicyEntry],
}

impl<'a> Policy<'a> {
    pub const fn new(entries: &'a [PolicyEntry]) -> Self {
        Self { entries }
    }

    pub fn allows(&self, service_id: u64, capability: &str) -> bool {
        self.entries
            .iter()
            .find(|entry| entry.service_id == service_id)
            .map(|entry| entry.capabilities.iter().any(|cap| cap.eq_ignore_ascii_case(capability)))
            .unwrap_or(false)
    }

    pub fn has_any_capability(&self, service_id: u64) -> bool {
        self.entries
            .iter()
            .find(|entry| entry.service_id == service_id)
            .map(|entry| !entry.capabilities.is_empty())
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::{Policy, PolicyEntry};

    #[test]
    fn allow_lookup() {
        let svc_a = 0x10u64;
        let svc_b = 0x20u64;
        let entries = [
            PolicyEntry { service_id: svc_a, capabilities: &["window.manage", "ipc.core"] },
            PolicyEntry { service_id: svc_b, capabilities: &[] },
        ];
        let policy = Policy::new(&entries);
        assert!(policy.allows(svc_a, "window.manage"));
        assert!(policy.allows(svc_a, "IPC.CORE"));
        assert!(!policy.allows(svc_a, "window.debug"));
        assert!(!policy.allows(svc_b, "window.manage"));
        assert!(!policy.allows(0x99, "missing"));
        assert!(policy.has_any_capability(svc_a));
        assert!(!policy.has_any_capability(svc_b));
    }
}
