// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the ONE deny taxonomy (TASK-0043 P4; RFC-0068 audit record,
//! RFC-0091 seams, RFC-0072 quotas). Every enforcer names its refusal with
//! a `DenyReason` from this vocabulary — policyd in its `audit v1 …
//! reason=<r>` record, statefsd in `statefs: quota deny … reason=<r>`,
//! netstackd's `!cap-deny` line — so `nx diagnose`/logd queries can match
//! one stable set of strings. `parse` is the reader's side and rejects
//! anything outside the vocabulary (`test_reject_audit_reason_unknown`).
//! Counters mirror the same names (`quota_denies_total`,
//! `egress_denies_total`, `ingress_denies_total`).
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below
//! RFC: docs/rfcs/RFC-0091-policy-profile-v2-schema-wire-argument-matchers.md

/// RFC-0091 argument-filter classes (mirror `nexus_abi::abi_filter::SyscallClass`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AbiClass {
    /// `statefs.put`.
    Statefs,
    /// `net.bind` / listen / udp-bind.
    NetBind,
    /// `net.connect`.
    NetConnect,
}

impl AbiClass {
    /// Wire class byte → class.
    pub const fn from_wire(class: u8) -> Option<Self> {
        match class {
            1 => Some(Self::Statefs),
            2 => Some(Self::NetBind),
            3 => Some(Self::NetConnect),
            _ => None,
        }
    }
}

/// Why a request was refused — the stable audit/marker vocabulary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DenyReason {
    /// Capability / route policy (policyd `check`, `route`, `exec`).
    Policy,
    /// RFC-0091 `statefs` argument filter refused (path / payload).
    AbiRuleStatefs,
    /// RFC-0091 `net.bind` refused — the user-facing ingress reason (TASK-0052).
    IngressDenied,
    /// RFC-0091 `net.connect` refused — the user-facing egress reason (TASK-0043).
    EgressDenied,
    /// RFC-0091 §6 mode transition (audited allow, not a refusal).
    AbiMode,
    /// RFC-0072 hard byte quota exceeded (statefsd).
    QuotaExceeded,
}

impl DenyReason {
    /// The reason an RFC-0091 evaluation refusal is reported as, by class.
    pub const fn for_abi_class(class: AbiClass) -> Self {
        match class {
            AbiClass::Statefs => Self::AbiRuleStatefs,
            AbiClass::NetBind => Self::IngressDenied,
            AbiClass::NetConnect => Self::EgressDenied,
        }
    }

    /// Stable audit/marker token.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Policy => "policy",
            Self::AbiRuleStatefs => "abi-rule:statefs",
            Self::IngressDenied => "ingress-denied",
            Self::EgressDenied => "egress-denied",
            Self::AbiMode => "abi-mode",
            Self::QuotaExceeded => "quota-exceeded",
        }
    }

    /// Counter name mirroring the reason (`None` = not counted).
    pub const fn counter_name(self) -> Option<&'static str> {
        match self {
            Self::QuotaExceeded => Some("quota_denies_total"),
            Self::EgressDenied => Some("egress_denies_total"),
            Self::IngressDenied => Some("ingress_denies_total"),
            Self::Policy | Self::AbiRuleStatefs | Self::AbiMode => None,
        }
    }

    /// Reader side: exactly the vocabulary, nothing else.
    pub fn parse(token: &str) -> Option<Self> {
        match token {
            "policy" => Some(Self::Policy),
            "abi-rule:statefs" => Some(Self::AbiRuleStatefs),
            "ingress-denied" => Some(Self::IngressDenied),
            "egress-denied" => Some(Self::EgressDenied),
            "abi-mode" => Some(Self::AbiMode),
            "quota-exceeded" => Some(Self::QuotaExceeded),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [DenyReason; 6] = [
        DenyReason::Policy,
        DenyReason::AbiRuleStatefs,
        DenyReason::IngressDenied,
        DenyReason::EgressDenied,
        DenyReason::AbiMode,
        DenyReason::QuotaExceeded,
    ];

    #[test]
    fn vocabulary_roundtrips() {
        for r in ALL {
            assert_eq!(DenyReason::parse(r.as_str()), Some(r));
        }
    }

    #[test]
    fn test_reject_audit_reason_unknown() {
        for bad in [
            "",
            "deny",
            "abi-rule",
            "abi-rule:net.connect",
            "quota",
            "Egress-Denied",
            "egress-denied ",
        ] {
            assert_eq!(DenyReason::parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn abi_classes_map_to_user_facing_reasons() {
        assert_eq!(DenyReason::for_abi_class(AbiClass::NetConnect), DenyReason::EgressDenied);
        assert_eq!(DenyReason::for_abi_class(AbiClass::NetBind), DenyReason::IngressDenied);
        assert_eq!(DenyReason::for_abi_class(AbiClass::Statefs), DenyReason::AbiRuleStatefs);
        assert_eq!(AbiClass::from_wire(9), None);
        assert_eq!(DenyReason::EgressDenied.counter_name(), Some("egress_denies_total"));
        assert_eq!(DenyReason::Policy.counter_name(), None);
    }
}
