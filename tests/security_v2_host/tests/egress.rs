// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: TASK-0043 P3 host proofs — the shipped `policies/base.toml`
//! `net.connect` profile of `selftest-client` compiled through the shared
//! grammar and evaluated by the real `nexus-abi` matcher, composed with the
//! seam decision (`seam_admits`): CIDR and port refusals, the allowed tuple,
//! and the unattributed-sender refusal. The QEMU proof (`SELFTEST: egress
//! deny/allow ok`) exercises exactly these tuples through netstackd.
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! TEST_COVERAGE: TASK-0043 P3 required `test_reject_*` host proofs

use nexus_abi::abi_filter::{AbiLimits, AbiProfile, AbiRule, AddrClass, PortRange, RuleAction};
use nexus_abi::policyd::{STATUS_ALLOW, STATUS_DENY, STATUS_UNSUPPORTED};
use nexus_ipc::policyd::seam_admits;
use nexus_policy::schema::{Action, AddressClass, Profile, Rule};
use nexus_policy::PolicyTree;

fn shipped_profile(subject: &str) -> AbiProfile {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../policies");
    let tree = PolicyTree::load_root(&root).unwrap();
    let p: &Profile = tree.policy().abi_profile(subject).expect("profile");
    let sid = nexus_abi::service_id_from_name(subject.as_bytes());
    let mut out = AbiProfile::empty(sid).with_epoch(p.epoch);
    if let Some(l) = p.limits {
        out = out.with_limits(AbiLimits { max_payload: l.max_payload, deadline_ms: l.deadline_ms });
    }
    let act = |a: Action| match a {
        Action::Allow => RuleAction::Allow,
        Action::Deny => RuleAction::Deny,
    };
    let ports = |v: &Vec<nexus_policy::schema::PortRange>| -> Vec<PortRange> {
        v.iter().map(|r| PortRange { min: r.min, max: r.max }).collect()
    };
    for r in &p.rules {
        let rule = match r {
            Rule::Statefs { action, prefix, max_payload } => {
                AbiRule::statefs(act(*action), prefix.as_bytes())
                    .unwrap()
                    .with_max_payload(*max_payload)
            }
            Rule::NetBind { action, address, ports: pr } => AbiRule::net_bind(
                act(*action),
                match address {
                    AddressClass::Loopback => AddrClass::Loopback,
                    AddressClass::Any => AddrClass::Any,
                },
                &ports(pr),
            )
            .unwrap(),
            Rule::NetConnect { action, cidr, cidr_len, ports: pr } => {
                AbiRule::net_connect(act(*action), *cidr, *cidr_len, &ports(pr)).unwrap()
            }
        };
        out.push_rule(rule).unwrap();
    }
    out
}

/// What policyd answers the seam for this tuple (ALLOW/DENY).
fn eval_status(profile: &AbiProfile, addr: [u8; 4], port: u16) -> u8 {
    match profile.check_net_connect(addr, port) {
        RuleAction::Allow => STATUS_ALLOW,
        RuleAction::Deny => STATUS_DENY,
    }
}

#[test]
fn test_reject_egress_cidr() {
    let p = shipped_profile("selftest-client");
    let sid = p.subject_service_id();
    // Outside 10.0.2.0/24 — every port, including an otherwise allowed one.
    for (addr, port) in [([192, 168, 1, 1], 80), ([10, 0, 3, 2], 443), ([1, 1, 1, 1], 53)] {
        assert_eq!(eval_status(&p, addr, port), STATUS_DENY, "{addr:?}:{port}");
        assert!(!seam_admits(sid, Some(eval_status(&p, addr, port))));
    }
}

#[test]
fn test_reject_egress_port() {
    let p = shipped_profile("selftest-client");
    let sid = p.subject_service_id();
    for port in [8080u16, 22, 8443, 1024] {
        assert_eq!(eval_status(&p, [10, 0, 2, 2], port), STATUS_DENY, "port {port}");
        assert!(!seam_admits(sid, Some(STATUS_DENY)));
    }
}

#[test]
fn egress_allowed_target_passes() {
    let p = shipped_profile("selftest-client");
    let sid = p.subject_service_id();
    for port in [53u16, 80, 443] {
        assert_eq!(eval_status(&p, [10, 0, 2, 2], port), STATUS_ALLOW, "port {port}");
        assert!(seam_admits(sid, Some(STATUS_ALLOW)));
    }
}

#[test]
fn test_reject_unattributed_connect() {
    let p = shipped_profile("selftest-client");
    // Even an allowed tuple is refused without a kernel-attributed sender.
    assert!(!seam_admits(0, Some(eval_status(&p, [10, 0, 2, 2], 53))));
    // A subject without a profile is not governed (capability-only) — but still attributed.
    assert!(seam_admits(7, Some(STATUS_UNSUPPORTED)));
    assert!(!seam_admits(0, Some(STATUS_UNSUPPORTED)));
}

#[test]
fn default_deny_without_connect_rules() {
    // A governed subject with no net.connect rule may connect nowhere.
    let sid = nexus_abi::service_id_from_name(b"demo.nonet");
    let p = AbiProfile::empty(sid).with_epoch(1);
    assert_eq!(eval_status(&p, [10, 0, 2, 2], 53), STATUS_DENY);
    assert_eq!(eval_status(&p, [127, 0, 0, 1], 80), STATUS_DENY);
}
