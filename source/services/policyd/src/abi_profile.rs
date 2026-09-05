// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RFC-0091 per-subject ABI profile serving. The build-time table
//! (`policy_table.rs`, compiled by `build.rs` from `policies/*.toml`
//! through the shared `schema.rs` grammar) is turned into a
//! `nexus_abi::abi_filter::AbiProfile` and encoded as wire v2 on every
//! `OP_ABI_PROFILE_GET`. A subject without an authored profile gets the
//! empty deny-everything profile (epoch 0). Both host (in-process tests)
//! and OS-lite builds use this module.
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: tests/lite_protocol_inprocess.rs (profile fetch),
//!   selftest `SELFTEST: abi filter *` markers
//! RFC: docs/rfcs/RFC-0091-policy-profile-v2-schema-wire-argument-matchers.md

use nexus_abi::abi_filter::{
    encode_profile_v2, AbiLimits, AbiProfile, AbiRule, AddrClass, PortRange, RuleAction,
};

mod policy_table {
    include!(concat!(env!("OUT_DIR"), "/policy_table.rs"));
}

/// One compiled rule of the build-time table.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
pub enum AbiRuleEntry {
    /// `statefs` rule: literal prefix + per-rule payload ceiling (0 = inherit).
    Statefs { action: u8, prefix: &'static str, max_payload: u32 },
    /// `net.bind` rule: address class (0 loopback, 1 any) + port ranges.
    NetBind { action: u8, address: u8, ports: &'static [(u16, u16)] },
    /// `net.connect` rule: IPv4 CIDR + port ranges.
    NetConnect { action: u8, cidr: [u8; 4], cidr_len: u8, ports: &'static [(u16, u16)] },
}

/// One subject's compiled profile.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
pub struct AbiProfileEntry {
    /// `service_id_from_name(subject)`.
    pub subject_id: u64,
    /// Authored epoch (0 for v1-shaped profiles).
    pub epoch: u32,
    /// `(deadline_ms, max_payload)` when a `limits` table was authored.
    pub limits: Option<(u32, u32)>,
    /// Rules in authored order (precedence is order-free).
    pub rules: &'static [AbiRuleEntry],
}

fn entry_for(subject_id: u64) -> Option<&'static AbiProfileEntry> {
    policy_table::ABI_PROFILE_ENTRIES.iter().find(|e| e.subject_id == subject_id)
}

/// The subject's current epoch (0 when no profile is authored).
pub fn subject_epoch(subject_id: u64) -> u32 {
    entry_for(subject_id).map_or(0, |e| e.epoch)
}

fn action(v: u8) -> RuleAction {
    if v == RuleAction::Allow as u8 {
        RuleAction::Allow
    } else {
        RuleAction::Deny
    }
}

fn ranges(ports: &[(u16, u16)]) -> ([PortRange; 4], usize) {
    let mut out = [PortRange::default(); 4];
    let n = ports.len().min(4);
    for (slot, &(min, max)) in out.iter_mut().zip(ports.iter().take(n)) {
        *slot = PortRange { min, max };
    }
    (out, n)
}

/// Builds the subject's `AbiProfile` from the table (deny-all when absent).
/// `None` only when the table itself is inconsistent — build.rs validates
/// every rule, so that is a build defect, not a runtime condition.
pub fn subject_profile(subject_id: u64) -> Option<AbiProfile> {
    let Some(entry) = entry_for(subject_id) else {
        return Some(AbiProfile::empty(subject_id));
    };
    let mut profile = AbiProfile::empty(subject_id).with_epoch(entry.epoch);
    if let Some((deadline_ms, max_payload)) = entry.limits {
        profile = profile.with_limits(AbiLimits { max_payload, deadline_ms });
    }
    for rule in entry.rules {
        let built = match *rule {
            AbiRuleEntry::Statefs { action: a, prefix, max_payload } => {
                AbiRule::statefs(action(a), prefix.as_bytes()).ok()?.with_max_payload(max_payload)
            }
            AbiRuleEntry::NetBind { action: a, address, ports } => {
                let (r, n) = ranges(ports);
                let addr = AddrClass::from_u8(address)?;
                AbiRule::net_bind(action(a), addr, &r[..n]).ok()?
            }
            AbiRuleEntry::NetConnect { action: a, cidr, cidr_len, ports } => {
                let (r, n) = ranges(ports);
                AbiRule::net_connect(action(a), cidr, cidr_len, &r[..n]).ok()?
            }
        };
        profile.push_rule(built).ok()?;
    }
    Some(profile)
}

/// Encodes the subject's profile as RFC-0091 wire v2 into `out`.
pub fn encode_subject_profile(subject_id: u64, out: &mut [u8]) -> Option<usize> {
    let profile = subject_profile(subject_id)?;
    encode_profile_v2(&profile, out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selftest_profile_is_v2_with_authored_semantics() {
        let selftest = nexus_abi::service_id_from_name(b"selftest-client");
        let p = subject_profile(selftest).unwrap();
        assert!(p.epoch() >= 1);
        assert!(p.limits().is_some());
        assert_eq!(p.check_statefs_put(b"/state/app/selftest/token", 4), RuleAction::Allow);
        assert_eq!(p.check_statefs_put(b"/state/app/selftest/secrets/key", 4), RuleAction::Deny);
        assert_eq!(p.check_statefs_put(b"/state/forbidden/token", 4), RuleAction::Deny);
        assert_eq!(p.check_net_bind(80, AddrClass::Loopback), RuleAction::Deny);
        assert_eq!(p.check_net_bind(1024, AddrClass::Loopback), RuleAction::Allow);
        assert_eq!(p.check_net_bind(1024, AddrClass::Any), RuleAction::Deny);
        assert_eq!(p.check_net_connect([10, 0, 2, 2], 443), RuleAction::Allow);
        assert_eq!(p.check_net_connect([10, 0, 3, 2], 443), RuleAction::Deny);
        assert_eq!(subject_epoch(selftest), p.epoch());
        // Wire v2, decodes back equal.
        let mut buf = [0u8; nexus_abi::abi_filter::MAX_PROFILE_BYTES];
        let n = encode_subject_profile(selftest, &mut buf).unwrap();
        assert_eq!(buf[2], nexus_abi::abi_filter::PROFILE_VERSION_V2);
        assert_eq!(nexus_abi::abi_filter::decode_profile(&buf[..n]).unwrap(), p);
    }

    #[test]
    fn unprofiled_subject_is_deny_all_epoch_zero() {
        let other = nexus_abi::service_id_from_name(b"demo.testsvc");
        let p = subject_profile(other).unwrap();
        assert_eq!((p.epoch(), p.rule_count(), p.limits()), (0, 0, None));
        assert_eq!(subject_epoch(other), 0);
        assert_eq!(p.check_statefs_put(b"/state/x", 1), RuleAction::Deny);
        assert_eq!(p.check_net_bind(1024, AddrClass::Loopback), RuleAction::Deny);
        assert_eq!(p.check_net_connect([10, 0, 2, 2], 443), RuleAction::Deny);
    }
}
