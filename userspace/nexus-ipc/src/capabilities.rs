// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Capability namespace — the single source of truth for capability names
//! (RFC-0066). Every capability the system gates on is a **typed** [`Capability`]
//! here, not a magic string scattered across services.
//!
//! Why: capability strings were ad-hoc (`"rng.entropy"`, `"nexus.permission.WINDOW"`
//! — inconsistent naming, easy to typo, impossible to audit). Declaring them once
//! means a typo is a compile error, the set is host-tested, and a new capability
//! cannot be forgotten (you add an enum variant, which every `match` must handle).
//!
//! Naming convention (enforced by a host test): `<domain>.<verb>`, lowercase.

/// A typed system capability. The wire/policy string is [`Capability::as_str`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Capability {
    /// Read entropy from the RNG service (rngd).
    RngEntropy,
    /// Query the bundle registry — enumerate/resolve installed apps (bundlemgrd).
    BundleQuery,
    /// Launch an app / drive its lifecycle (abilitymgr) — only the broker holds it.
    AppLaunch,
    /// Spawn an OS process (execd) — only the lifecycle broker should hold it.
    ProcessSpawn,
    /// Own a window surface composited by windowd.
    WindowSurface,
}

impl Capability {
    /// The canonical capability name used on the policyd wire + in manifests.
    pub const fn as_str(self) -> &'static str {
        match self {
            Capability::RngEntropy => "rng.entropy",
            Capability::BundleQuery => "bundle.query",
            Capability::AppLaunch => "app.launch",
            Capability::ProcessSpawn => "process.spawn",
            Capability::WindowSurface => "window.surface",
        }
    }

    /// The capability name as bytes (for the policyd wire).
    pub const fn as_bytes(self) -> &'static [u8] {
        self.as_str().as_bytes()
    }

    /// Parses a capability name, or `None` if unknown.
    pub fn from_str(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|c| c.as_str() == name)
    }

    /// Every declared capability — the complete namespace.
    pub const ALL: &'static [Capability] = &[
        Capability::RngEntropy,
        Capability::BundleQuery,
        Capability::AppLaunch,
        Capability::ProcessSpawn,
        Capability::WindowSurface,
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for cap in Capability::ALL {
            assert_eq!(Capability::from_str(cap.as_str()), Some(*cap));
        }
        assert_eq!(Capability::from_str("does.not.exist"), None);
    }

    #[test]
    fn names_are_unique() {
        for (i, a) in Capability::ALL.iter().enumerate() {
            for b in &Capability::ALL[i + 1..] {
                assert_ne!(a.as_str(), b.as_str(), "duplicate capability name {}", a.as_str());
            }
        }
    }

    #[test]
    fn names_follow_domain_dot_verb_convention() {
        for cap in Capability::ALL {
            let s = cap.as_str();
            assert!(!s.is_empty() && s.len() <= 48, "{s} bad length");
            assert!(
                s.chars().all(|c| c.is_ascii_lowercase() || c == '.'),
                "{s} must be lowercase + dots"
            );
            assert_eq!(s.split('.').count(), 2, "{s} must be `<domain>.<verb>`");
            assert!(s.split('.').all(|p| !p.is_empty()), "{s} has an empty segment");
        }
    }
}
