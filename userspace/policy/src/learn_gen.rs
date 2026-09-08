// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RFC-0091 §5 generator behind `nx policy learn-gen`: learn
//! records (one subject) → a review-first `[abi_profile.<subject>]`
//! SKELETON in schema v2. Dedup, deterministic order, capped at
//! `MAX_RULES`; every observed `would=deny|limit` argument becomes an
//! `allow` rule. `address = "any"` binds are emitted commented-out unless
//! `allow_any` is set — a generated profile never widens exposure by
//! default (and a connect rule is always the observed /32 host, never
//! `0.0.0.0/0`). The output must compile through `schema::compile`
//! (`learn_roundtrip` tests prove it).
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below; tools/nx/tests/policy_cli.rs;
//!   policyd `abi_learn_roundtrip_tests.rs` (`test_learn_roundtrip`)
//! RFC: docs/rfcs/RFC-0091-policy-profile-v2-schema-wire-argument-matchers.md

use std::collections::BTreeSet;
use std::fmt::Write as _;

use crate::learn_record::{parse_learn_record, LearnAddr, LearnArg, LearnRecord};
use crate::schema::MAX_RULES;

/// Generator options.
#[derive(Debug, Clone)]
pub struct GenOptions {
    /// Subject name (the TOML key); its FNV id selects the records.
    pub subject_name: String,
    /// Subject id the records must carry (`service_id_from_name(subject_name)`).
    pub subject_id: u64,
    /// Emit `address = "any"` bind rules live instead of commented-out.
    pub allow_any: bool,
}

/// What the generator produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Generated {
    /// The skeleton TOML.
    pub toml: String,
    /// Lines that parsed as learn records (any subject).
    pub records_parsed: usize,
    /// Lines that did not parse (ignored, counted).
    pub lines_ignored: usize,
    /// Records for the requested subject.
    pub records_for_subject: usize,
    /// Distinct (class, argument) keys for the subject.
    pub unique_args: usize,
    /// Rules emitted live.
    pub rules_emitted: usize,
    /// Rules kept commented-out (`any` binds without `allow_any`).
    pub rules_held: usize,
    /// Unique arguments dropped by the `MAX_RULES` cap.
    pub rules_capped: usize,
    /// Highest epoch observed for the subject.
    pub epoch: u32,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Key {
    Statefs(String),
    NetBind(u16, LearnAddr),
    NetConnect([u8; 4], u16),
}

fn key_of(arg: LearnArg<'_>) -> Key {
    match arg {
        LearnArg::Statefs(p) => Key::Statefs(p.to_string()),
        LearnArg::NetBind(port, addr) => Key::NetBind(port, addr),
        LearnArg::NetConnect(a, port) => Key::NetConnect(a, port),
    }
}

/// Generates the skeleton from record lines (non-records are ignored and
/// counted, so a raw UART or logd dump works as input).
pub fn generate<'a, I>(lines: I, opts: &GenOptions) -> Generated
where
    I: IntoIterator<Item = &'a str>,
{
    let mut parsed = 0usize;
    let mut ignored = 0usize;
    let mut for_subject = 0usize;
    let mut epoch = 0u32;
    let mut keys: BTreeSet<Key> = BTreeSet::new();
    for line in lines {
        let Some(rec) = parse_learn_record(line) else {
            if !line.trim().is_empty() {
                ignored += 1;
            }
            continue;
        };
        parsed += 1;
        if rec.subject != opts.subject_id {
            continue;
        }
        for_subject += 1;
        epoch = epoch.max(rec.epoch);
        keys.insert(key_of(rec.arg));
    }
    let unique = keys.len();
    let mut toml = String::new();
    let _ = writeln!(toml, "# generated — review before enabling");
    let _ = writeln!(
        toml,
        "# nx policy learn-gen: subject {:?} (id {:016x}), observed under epoch {epoch};",
        opts.subject_name, opts.subject_id
    );
    let _ = writeln!(
        toml,
        "# {for_subject} records → {unique} unique arguments. Every rule below is an ALLOW\n\
         # for something the profile refused; bump `epoch` when you adopt it."
    );
    let _ = writeln!(toml, "[abi_profile.{:?}]", opts.subject_name);
    let _ = writeln!(toml, "epoch = {}", epoch.saturating_add(1));
    let mut emitted = 0usize;
    let mut held = 0usize;
    let mut capped = 0usize;
    for key in keys {
        if emitted >= MAX_RULES {
            capped += 1;
            continue;
        }
        toml.push('\n');
        match key {
            Key::Statefs(prefix) => {
                let _ = writeln!(toml, "[[abi_profile.{:?}.statefs]]", opts.subject_name);
                let _ = writeln!(toml, "action = \"allow\"");
                let _ = writeln!(toml, "prefix = {prefix:?}");
                emitted += 1;
            }
            Key::NetBind(port, addr) => {
                let live = addr == LearnAddr::Loopback || opts.allow_any;
                let c = if live { "" } else { "# " };
                if !live {
                    let _ = writeln!(
                        toml,
                        "# held: any-interface bind needs an exposure intent (re-run with --allow-any)"
                    );
                }
                let _ = writeln!(toml, "{c}[[abi_profile.{:?}.net.bind]]", opts.subject_name);
                let _ = writeln!(toml, "{c}action = \"allow\"");
                let _ = writeln!(toml, "{c}ports = [\"{port}\"]");
                let _ = writeln!(
                    toml,
                    "{c}address = \"{}\"",
                    match addr {
                        LearnAddr::Loopback => "loopback",
                        LearnAddr::Any => "any",
                    }
                );
                if live {
                    emitted += 1;
                } else {
                    held += 1;
                }
            }
            Key::NetConnect(a, port) => {
                let _ = writeln!(toml, "[[abi_profile.{:?}.net.connect]]", opts.subject_name);
                let _ = writeln!(toml, "action = \"allow\"");
                let _ = writeln!(toml, "cidr = \"{}.{}.{}.{}/32\"", a[0], a[1], a[2], a[3]);
                let _ = writeln!(toml, "ports = [\"{port}\"]");
                emitted += 1;
            }
        }
    }
    Generated {
        toml,
        records_parsed: parsed,
        lines_ignored: ignored,
        records_for_subject: for_subject,
        unique_args: unique,
        rules_emitted: emitted,
        rules_held: held,
        rules_capped: capped,
        epoch,
    }
}

/// Convenience: records of `record` shape for tests and tooling.
pub fn format_record(record: &LearnRecord<'_>) -> Option<String> {
    let mut buf = [0u8; crate::learn_record::MAX_LEARN_RECORD_BYTES];
    let n = record.write(&mut buf)?;
    core::str::from_utf8(&buf[..n]).ok().map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::learn_record::{service_id_from_name, Would};
    use crate::schema::{compile, Action, AddressClass, RawAbiProfile, Rule};
    use std::collections::BTreeMap;

    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Fixture {
        abi_profile: BTreeMap<String, RawAbiProfile>,
    }

    fn opts(allow_any: bool) -> GenOptions {
        GenOptions {
            subject_name: "demo.learn".into(),
            subject_id: service_id_from_name(b"demo.learn"),
            allow_any,
        }
    }

    fn rec(arg: LearnArg<'_>, epoch: u32) -> String {
        format_record(&LearnRecord {
            epoch,
            subject: service_id_from_name(b"demo.learn"),
            arg,
            would: Would::Deny,
        })
        .unwrap()
    }

    #[test]
    fn skeleton_compiles_dedups_sorts_and_holds_any() {
        let other = format_record(&LearnRecord {
            epoch: 9,
            subject: service_id_from_name(b"someone.else"),
            arg: LearnArg::Statefs("/state/other/"),
            would: Would::Deny,
        })
        .unwrap();
        let lines = [
            rec(LearnArg::NetConnect([10, 0, 2, 2], 443), 2),
            rec(LearnArg::Statefs("/state/app/demo/"), 3),
            rec(LearnArg::Statefs("/state/app/demo/"), 3),
            rec(LearnArg::NetBind(8080, LearnAddr::Any), 3),
            rec(LearnArg::NetBind(9000, LearnAddr::Loopback), 1),
            other,
            "noise: not a record".to_string(),
        ];
        let out = generate(lines.iter().map(String::as_str), &opts(false));
        assert_eq!(
            (out.records_parsed, out.lines_ignored, out.records_for_subject, out.unique_args),
            (6, 1, 5, 4)
        );
        assert_eq!((out.rules_emitted, out.rules_held, out.rules_capped, out.epoch), (3, 1, 0, 3));
        assert!(out.toml.starts_with("# generated — review before enabling\n"));
        assert!(out.toml.contains("epoch = 4\n"));
        assert!(!out.toml.contains("\naddress = \"any\""));
        assert!(out.toml.contains("# address = \"any\""));
        let fixture: Fixture = toml::from_str(&out.toml).unwrap();
        let profile = compile(&fixture.abi_profile["demo.learn"]).unwrap();
        assert_eq!(profile.epoch, 4);
        assert_eq!(profile.rules.len(), 3);
        assert!(
            matches!(&profile.rules[0], Rule::Statefs { action: Action::Allow, prefix, .. } if prefix == "/state/app/demo/")
        );
        assert!(matches!(&profile.rules[1], Rule::NetBind { address: AddressClass::Loopback, .. }));
        assert!(matches!(
            &profile.rules[2],
            Rule::NetConnect { cidr: [10, 0, 2, 2], cidr_len: 32, .. }
        ));
        // Deterministic: same input ⇒ same bytes.
        assert_eq!(generate(lines.iter().map(String::as_str), &opts(false)).toml, out.toml);
        // --allow-any lifts the hold — but RFC-0092 reserves an any-address
        // allow for the ingress gateway: the skeleton compiles for `ingressd`
        // and is refused for anyone else (they declare an exposure instead).
        let out = generate(lines.iter().map(String::as_str), &opts(true));
        assert_eq!((out.rules_emitted, out.rules_held), (4, 0));
        let fixture: Fixture = toml::from_str(&out.toml).unwrap();
        let raw = &fixture.abi_profile["demo.learn"];
        assert!(matches!(
            crate::schema::compile_for("demo.learn", raw),
            Err(crate::schema::SchemaError::AnyBindNeedsGateway { .. })
        ));
        let profile = crate::schema::compile_for(crate::schema::GATEWAY_SUBJECT, raw).unwrap();
        assert!(profile
            .rules
            .iter()
            .any(|r| matches!(r, Rule::NetBind { address: AddressClass::Any, .. })));
    }

    #[test]
    fn cap_at_max_rules_is_reported_not_silent() {
        let lines: Vec<String> = (0..(MAX_RULES + 5))
            .map(|i| rec(LearnArg::NetBind(1000 + i as u16, LearnAddr::Loopback), 1))
            .collect();
        let out = generate(lines.iter().map(String::as_str), &opts(false));
        assert_eq!((out.rules_emitted, out.rules_capped), (MAX_RULES, 5));
        let fixture: Fixture = toml::from_str(&out.toml).unwrap();
        assert!(compile(&fixture.abi_profile["demo.learn"]).is_ok());
    }

    #[test]
    fn empty_input_is_an_explicit_deny_all_skeleton() {
        let out = generate(std::iter::empty(), &opts(false));
        assert_eq!(out.rules_emitted, 0);
        let fixture: Fixture = toml::from_str(&out.toml).unwrap();
        let profile = compile(&fixture.abi_profile["demo.learn"]).unwrap();
        assert!(profile.rules.is_empty());
        assert_eq!(profile.epoch, 1);
    }
}
