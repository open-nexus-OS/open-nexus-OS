// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RFC-0091 `test_learn_roundtrip` — learn log → `learn-gen`
//! skeleton → shared schema parse → matcher evaluation: the observed
//! arguments are now allowed, nothing else is. The generator and schema
//! are the host `policy` crate's files included by path (policyd is
//! `no_std` on the OS and must not depend on that crate), the matcher is
//! the real `nexus-abi` one, the records come from policyd's own
//! `OP_ABI_EVAL` path in Learn mode.
//! (A crate-internal test module: policyd's OS-lite modules are compiled
//! for unit tests only, not for integration-test builds.)
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! TEST_COVERAGE: TASK-0028 P2 required host proof

use std::collections::BTreeMap;

use crate::{learn_gen, learn_record, schema};

use crate::abi_learn::{AbiMode, EvalHost, LearnState};
use crate::lite_protocol::handle_frame_with;
use nexus_abi::abi_filter::{
    AbiLimits, AbiProfile, AbiRule, AddrClass, PortRange, RuleAction, MAX_PROFILE_BYTES,
};
use nexus_abi::policyd::{
    decode_rsp_v2, encode_abi_eval_v2, ABI_CLASS_NET_BIND, ABI_CLASS_NET_CONNECT,
    ABI_CLASS_STATEFS_PUT, STATUS_ALLOW, STATUS_DENY,
};
use nexus_sel::Policy;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    abi_profile: BTreeMap<String, schema::RawAbiProfile>,
}

struct LearnHost {
    state: LearnState,
    lines: Vec<String>,
}

impl EvalHost for LearnHost {
    fn now_ns(&self) -> u64 {
        0
    }
    fn mode_of(&self, _subject: u64) -> AbiMode {
        AbiMode::Learn
    }
    fn learn_state(&self) -> &LearnState {
        &self.state
    }
    fn emit_learn(&mut self, record: &[u8]) -> bool {
        self.lines.push(String::from_utf8(record.to_vec()).unwrap());
        true
    }
    fn set_mode(&mut self, _subject: u64, _mode: AbiMode) -> bool {
        false
    }
}

#[allow(clippy::too_many_arguments)]
fn eval(
    host: &mut LearnHost,
    subject: u64,
    class: u8,
    addr_class: u8,
    port: u16,
    addr: [u8; 4],
    payload: u32,
    path: &[u8],
) -> u8 {
    let policy = Policy::new(&[]);
    let mut req = [0u8; 256];
    let n = encode_abi_eval_v2(
        1,
        subject,
        class,
        addr_class,
        port,
        u32::from_be_bytes(addr),
        payload,
        0,
        path,
        &mut req,
    )
    .unwrap();
    let out = handle_frame_with(&policy, &req[..n], subject, false, host);
    decode_rsp_v2(out.as_slice()).unwrap().2
}

/// The schema → matcher bridge (what a consumer of the compiled profile does).
fn build_profile(subject: u64, p: &schema::Profile) -> AbiProfile {
    let mut out = AbiProfile::empty(subject).with_epoch(p.epoch);
    if let Some(l) = p.limits {
        out = out.with_limits(AbiLimits { max_payload: l.max_payload, deadline_ms: l.deadline_ms });
    }
    let act = |a: schema::Action| match a {
        schema::Action::Allow => RuleAction::Allow,
        schema::Action::Deny => RuleAction::Deny,
    };
    let ports = |v: &Vec<schema::PortRange>| -> Vec<PortRange> {
        v.iter().map(|r| PortRange { min: r.min, max: r.max }).collect()
    };
    for r in &p.rules {
        let rule = match r {
            schema::Rule::Statefs { action, prefix, max_payload } => {
                AbiRule::statefs(act(*action), prefix.as_bytes())
                    .unwrap()
                    .with_max_payload(*max_payload)
            }
            schema::Rule::NetBind { action, address, ports: pr } => AbiRule::net_bind(
                act(*action),
                match address {
                    schema::AddressClass::Loopback => AddrClass::Loopback,
                    schema::AddressClass::Any => AddrClass::Any,
                },
                &ports(pr),
            )
            .unwrap(),
            schema::Rule::NetConnect { action, cidr, cidr_len, ports: pr } => {
                AbiRule::net_connect(act(*action), *cidr, *cidr_len, &ports(pr)).unwrap()
            }
        };
        out.push_rule(rule).unwrap();
    }
    out
}

#[test]
fn test_learn_roundtrip() {
    let subject = nexus_abi::service_id_from_name(b"selftest-client");
    let mut host = LearnHost { state: LearnState::new(), lines: Vec::new() };
    let z = [0u8; 4];
    // Learn mode: the shipped profile still refuses these…
    assert_eq!(
        eval(&mut host, subject, ABI_CLASS_STATEFS_PUT, 0, 0, z, 16, b"/state/app/other/token"),
        STATUS_DENY
    );
    assert_eq!(
        eval(&mut host, subject, ABI_CLASS_STATEFS_PUT, 0, 0, z, 16, b"/state/app/other/again"),
        STATUS_DENY
    );
    assert_eq!(eval(&mut host, subject, ABI_CLASS_NET_BIND, 0, 900, z, 0, b""), STATUS_DENY);
    assert_eq!(eval(&mut host, subject, ABI_CLASS_NET_BIND, 1, 8080, z, 0, b""), STATUS_DENY);
    assert_eq!(
        eval(&mut host, subject, ABI_CLASS_NET_CONNECT, 0, 8443, [192, 168, 1, 7], 0, b""),
        STATUS_DENY
    );
    // …and what it allows learns nothing.
    assert_eq!(
        eval(&mut host, subject, ABI_CLASS_STATEFS_PUT, 0, 0, z, 16, b"/state/app/selftest/token"),
        STATUS_ALLOW
    );
    assert_eq!(host.lines.len(), 4, "{:?}", host.lines);

    // learn-gen (default: any-interface binds are held).
    let opts = learn_gen::GenOptions {
        subject_name: "selftest-client".into(),
        subject_id: learn_record::service_id_from_name(b"selftest-client"),
        allow_any: false,
    };
    assert_eq!(opts.subject_id, subject, "one FNV for the table and the tools");
    let out = learn_gen::generate(host.lines.iter().map(String::as_str), &opts);
    assert_eq!(
        (out.records_for_subject, out.unique_args, out.rules_emitted, out.rules_held),
        (4, 4, 3, 1)
    );
    let authored = crate::abi_profile::subject_epoch(subject);
    assert_eq!(out.epoch, authored);

    // Shared grammar parses the skeleton; the matcher now allows exactly the observed arguments.
    let fixture: Fixture = toml::from_str(&out.toml).unwrap();
    let compiled = schema::compile(&fixture.abi_profile["selftest-client"]).unwrap();
    assert_eq!(compiled.epoch, authored + 1);
    let profile = build_profile(subject, &compiled);
    let mut wire = [0u8; MAX_PROFILE_BYTES];
    assert!(nexus_abi::abi_filter::encode_profile_v2(&profile, &mut wire).is_ok());
    assert_eq!(profile.check_statefs_put(b"/state/app/other/token", 16), RuleAction::Allow);
    assert_eq!(profile.check_statefs_put(b"/state/app/other/again", 16), RuleAction::Allow);
    assert_eq!(profile.check_statefs_put(b"/state/app/elsewhere/x", 16), RuleAction::Deny);
    assert_eq!(profile.check_net_bind(900, AddrClass::Loopback), RuleAction::Allow);
    assert_eq!(profile.check_net_bind(901, AddrClass::Loopback), RuleAction::Deny);
    assert_eq!(
        profile.check_net_bind(8080, AddrClass::Any),
        RuleAction::Deny,
        "held without --allow-any"
    );
    assert_eq!(profile.check_net_connect([192, 168, 1, 7], 8443), RuleAction::Allow);
    assert_eq!(profile.check_net_connect([192, 168, 1, 8], 8443), RuleAction::Deny);
    assert_eq!(profile.check_net_connect([192, 168, 1, 7], 8444), RuleAction::Deny);

    // --allow-any lifts exactly the held bind.
    let out = learn_gen::generate(
        host.lines.iter().map(String::as_str),
        &learn_gen::GenOptions { allow_any: true, ..opts },
    );
    let fixture: Fixture = toml::from_str(&out.toml).unwrap();
    // RFC-0092 Layer A: an any-address allow is refused for every subject but
    // the ingress gateway — the skeleton is a review artefact, not policy.
    assert!(matches!(
        schema::compile_for("selftest-client", &fixture.abi_profile["selftest-client"]),
        Err(schema::SchemaError::AnyBindNeedsGateway { .. })
    ));
    let profile = build_profile(
        subject,
        &schema::compile_for(schema::GATEWAY_SUBJECT, &fixture.abi_profile["selftest-client"])
            .unwrap(),
    );
    assert_eq!(profile.check_net_bind(8080, AddrClass::Any), RuleAction::Allow);
    assert_eq!(profile.check_net_bind(8081, AddrClass::Any), RuleAction::Deny);
}
