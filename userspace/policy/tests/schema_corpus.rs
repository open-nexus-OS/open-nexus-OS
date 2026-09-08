// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RFC-0091 shared schema corpus — every `policies/tests/*.toml`
//! fixture is parsed through the ONE `schema.rs` grammar (the same file
//! policyd's build.rs includes) and must match its `# expect:` verdict.
//! Positive fixtures additionally pin the transcode/compile semantics.
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! TEST_COVERAGE: TASK-0028 P1 (`test_reject_regex_dos` at the parser)

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use nexus_policy::expose::{
    check_exposes_unique, compile_exposes, Expose, Proto, RawExpose, TlsSlot,
};
use nexus_policy::schema::{
    check_quotas_disjoint, compile, compile_for, compile_quota, Action, AddressClass, PortRange,
    Profile, Quota, RawAbiProfile, RawQuota, Rule, SchemaError,
};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    #[serde(default)]
    abi_profile: BTreeMap<String, RawAbiProfile>,
    #[serde(default)]
    quota: BTreeMap<String, RawQuota>,
    #[serde(default)]
    expose: BTreeMap<String, Vec<RawExpose>>,
}

fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../policies/tests")
}

fn expectation(data: &str) -> String {
    data.lines()
        .find_map(|l| l.strip_prefix("# expect:"))
        .map(|v| v.trim().to_string())
        .unwrap_or_else(|| panic!("fixture without `# expect:` line"))
}

fn variant_name(err: &SchemaError) -> &'static str {
    match err {
        SchemaError::MixedVersions => "MixedVersions",
        SchemaError::EmptyPrefix => "EmptyPrefix",
        SchemaError::PrefixNotAbsolute(_) => "PrefixNotAbsolute",
        SchemaError::PrefixNotCanonical(_) => "PrefixNotCanonical",
        SchemaError::PrefixTooLong { .. } => "PrefixTooLong",
        SchemaError::PrefixPatternSyntax { .. } => "PrefixPatternSyntax",
        SchemaError::UnknownAction(_) => "UnknownAction",
        SchemaError::UnknownAddress(_) => "UnknownAddress",
        SchemaError::BadPort(_) => "BadPort",
        SchemaError::NoPortRanges => "NoPortRanges",
        SchemaError::TooManyPortRanges { .. } => "TooManyPortRanges",
        SchemaError::BadCidr(_) => "BadCidr",
        SchemaError::TooManyRules { .. } => "TooManyRules",
        SchemaError::RuleLimitAboveProfile { .. } => "RuleLimitAboveProfile",
        SchemaError::QuotaNoPrefixes => "QuotaNoPrefixes",
        SchemaError::QuotaTooManyPrefixes { .. } => "QuotaTooManyPrefixes",
        SchemaError::QuotaLimits { .. } => "QuotaLimits",
        SchemaError::QuotaOverlap { .. } => "QuotaOverlap",
        SchemaError::TooManyQuotas { .. } => "TooManyQuotas",
        SchemaError::AnyBindNeedsGateway { .. } => "AnyBindNeedsGateway",
        SchemaError::ExposeUnknownProto(_) => "ExposeUnknownProto",
        SchemaError::ExposeUnknownTls(_) => "ExposeUnknownTls",
        SchemaError::ExposeTlsUnsupported(_) => "ExposeTlsUnsupported",
        SchemaError::ExposeBadPort { .. } => "ExposeBadPort",
        SchemaError::ExposeNoCidrs => "ExposeNoCidrs",
        SchemaError::ExposeTooManyCidrs { .. } => "ExposeTooManyCidrs",
        SchemaError::ExposeRate { .. } => "ExposeRate",
        SchemaError::ExposeTooMany { .. } => "ExposeTooMany",
        SchemaError::ExposeTooManyTotal { .. } => "ExposeTooManyTotal",
        SchemaError::ExposeDuplicate { .. } => "ExposeDuplicate",
    }
}

/// Parses one fixture the way both parsers do: TOML → raw → compile.
fn run(path: &Path) -> Result<BTreeMap<String, Profile>, String> {
    let data = fs::read_to_string(path).unwrap();
    let fixture: Fixture = toml::from_str(&data).map_err(|_| "parse".to_string())?;
    let mut out = BTreeMap::new();
    for (subject, raw) in fixture.abi_profile {
        let profile = compile_for(&subject, &raw).map_err(|e| variant_name(&e).to_string())?;
        out.insert(subject, profile);
    }
    let mut quotas: BTreeMap<String, Quota> = BTreeMap::new();
    for (subject, raw) in fixture.quota {
        let q = compile_quota(&raw).map_err(|e| variant_name(&e).to_string())?;
        quotas.insert(subject, q);
    }
    check_quotas_disjoint(quotas.iter().map(|(k, v)| (k.as_str(), v)))
        .map_err(|e| variant_name(&e).to_string())?;
    let mut exposes: BTreeMap<String, Vec<Expose>> = BTreeMap::new();
    for (subject, raw) in fixture.expose {
        let e = compile_exposes(&subject, &raw).map_err(|e| variant_name(&e).to_string())?;
        exposes.insert(subject, e);
    }
    check_exposes_unique(exposes.iter().map(|(k, v)| (k.as_str(), v.as_slice())))
        .map_err(|e| variant_name(&e).to_string())?;
    Ok(out)
}

/// Compiles only the `[[expose]]` part of a fixture (the runner above
/// folds every domain into one verdict).
fn run_expose(path: &Path) -> BTreeMap<String, Vec<Expose>> {
    let data = fs::read_to_string(path).unwrap();
    let fixture: Fixture = toml::from_str(&data).unwrap();
    fixture
        .expose
        .into_iter()
        .map(|(subject, raw)| {
            let e = compile_exposes(&subject, &raw).unwrap();
            (subject, e)
        })
        .collect()
}

#[test]
fn corpus_verdicts_match_expectations() {
    let mut files: Vec<PathBuf> = fs::read_dir(corpus_dir())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("toml"))
        .collect();
    files.sort();
    assert!(files.len() >= 15, "corpus too small: {}", files.len());
    let mut seen_ok = 0;
    let mut seen_reject = 0;
    for path in files {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let expect = expectation(&fs::read_to_string(&path).unwrap());
        let verdict = match run(&path) {
            Ok(_) => "ok".to_string(),
            Err(e) => e,
        };
        assert_eq!(verdict, expect, "fixture {name}");
        if name.starts_with("ok_") {
            assert_eq!(expect, "ok", "fixture {name} is named ok_ but expects a reject");
            seen_ok += 1;
        } else {
            assert!(name.starts_with("reject_"), "fixture {name} must be ok_* or reject_*");
            assert_ne!(expect, "ok", "fixture {name} is named reject_ but expects ok");
            seen_reject += 1;
        }
    }
    assert!(seen_ok >= 3 && seen_reject >= 12, "ok={seen_ok} reject={seen_reject}");
}

#[test]
fn test_reject_regex_dos() {
    // The parser is where pattern syntax dies: no matcher ever sees it.
    let verdict = run(&corpus_dir().join("reject_regex_prefix.toml")).unwrap_err();
    assert_eq!(verdict, "PrefixPatternSyntax");
    for bad in
        ["/state/a*", "/state/a?", "/state/[ab]/", "/state/(a)/", "/state/a|b/", "/state/a\\b"]
    {
        let raw = RawAbiProfile {
            epoch: Some(1),
            statefs: vec![nexus_policy::schema::RawStatefsRule {
                action: "allow".into(),
                prefix: bad.into(),
                max_payload: None,
            }],
            ..Default::default()
        };
        assert!(
            matches!(compile(&raw), Err(SchemaError::PrefixPatternSyntax { .. })),
            "{bad} must be rejected"
        );
    }
}

#[test]
fn v1_legacy_transcodes_to_v2_rules() {
    let profiles = run(&corpus_dir().join("ok_v1_legacy.toml")).unwrap();
    let p = &profiles["demo.legacy"];
    assert_eq!(p.epoch, 0);
    assert_eq!(p.limits, None);
    assert_eq!(
        p.rules,
        vec![
            Rule::Statefs {
                action: Action::Allow,
                prefix: "/state/app/legacy/".into(),
                max_payload: 0
            },
            Rule::NetBind {
                action: Action::Allow,
                address: AddressClass::Loopback,
                ports: vec![PortRange { min: 1024, max: 65535 }],
            },
        ]
    );
}

#[test]
fn v2_full_compiles_canonically() {
    let profiles = run(&corpus_dir().join("ok_v2_full.toml")).unwrap();
    let p = &profiles["demo.full"];
    assert_eq!(p.epoch, 7);
    assert_eq!(p.limits.map(|l| (l.deadline_ms, l.max_payload)), Some((2000, 4096)));
    assert_eq!(p.rules.len(), 6);
    assert!(matches!(&p.rules[0], Rule::Statefs { max_payload: 2048, .. }));
    assert!(matches!(
        &p.rules[3],
        Rule::NetBind { action: Action::Deny, address: AddressClass::Any, .. }
    ));
    assert!(matches!(&p.rules[5], Rule::NetConnect { cidr: [0, 0, 0, 0], cidr_len: 0, .. }));
    // The root loader compiles the shipped base.toml through the same path.
    let tree = nexus_policy::PolicyTree::load_root(&corpus_dir().join("..")).unwrap();
    let live = tree.policy().abi_profile("selftest-client").expect("selftest profile");
    assert!(live.epoch >= 1);
    assert!(live.rules.iter().any(|r| matches!(r, Rule::NetConnect { .. })));
}

#[test]
fn expose_declarations_compile_canonically() {
    let all = run_expose(&corpus_dir().join("ok_expose.toml"));
    let web = &all["demo.web"];
    assert_eq!(web.len(), 2);
    assert_eq!((web[0].port, web[0].proto, web[0].backend), (8080, Proto::Tcp, 18080));
    assert_eq!(web[0].cidr_allow.len(), 2);
    assert_eq!((web[0].cidr_allow[0].addr, web[0].cidr_allow[0].len), ([10, 0, 2, 0], 24));
    assert_eq!((web[0].rate_per_s, web[0].burst, web[0].tls), (100, 20, TlsSlot::None));
    assert_eq!((web[1].port, web[1].proto), (5353, Proto::Udp));
    // The same port on a different proto is a distinct exposure.
    let api = &all["demo.api"];
    assert_eq!((api[0].port, api[0].proto), (8080, Proto::Udp));
    // The shipped policy root compiles the domain through the same loader
    // (no exposure is declared until ingressd runs — TASK-0052 P3).
    let tree = nexus_policy::PolicyTree::load_root(&corpus_dir().join("..")).unwrap();
    assert_eq!(
        tree.policy().expose_count(),
        tree.policy().exposes().map(|(_, e)| e.len()).sum::<usize>()
    );
}

#[test]
fn quota_declarations_compile_and_reject() {
    let ok = RawQuota {
        prefixes: vec!["/state/app/demo/".into(), "/state/demo/".into()],
        soft_bytes: 100,
        hard_bytes: 200,
    };
    let q = compile_quota(&ok).unwrap();
    assert_eq!((q.soft_bytes, q.hard_bytes, q.prefixes.len()), (100, 200, 2));
    // The shipped base.toml declares the selftest quota through the root loader.
    let tree = nexus_policy::PolicyTree::load_root(&corpus_dir().join("..")).unwrap();
    let live = tree.policy().quota("selftest-client").expect("selftest quota");
    assert!(live.hard_bytes >= live.soft_bytes && live.soft_bytes > 0);
    assert!(live.prefixes.iter().all(|p| p.starts_with("/state/") && p.ends_with('/')));
}
