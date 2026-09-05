// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RFC-0091 §1 `[abi_profile.<subject>]` schema v2 — the ONE
//! grammar both parsers accept. The host `policy` crate uses it as a
//! module; policyd's `build.rs` includes this file by path so the OS
//! table is compiled from the identical rules (no second parser). It
//! depends on `serde` only. Raw TOML shapes deserialize here; `compile`
//! validates them into the pure, canonical `Profile` (no nexus-abi
//! dependency — the matcher types live in `source/libs/nexus-abi`).
//! v1 keys (`statefs_put_allow_prefix`, `net_bind_min_port`) transcode to
//! v2 rules with `epoch = 0`; mixing v1 keys with v2 sections is an error.
//! Prefixes are bounded literals — any pattern syntax is rejected
//! (`test_reject_regex_dos`).
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: tests/schema_corpus.rs over `policies/tests/*.toml`
//! RFC: docs/rfcs/RFC-0091-policy-profile-v2-schema-wire-argument-matchers.md

use std::fmt;

use serde::{Deserialize, Serialize};

/// Rules per subject (mirrors `nexus_abi::abi_filter::MAX_RULES`).
pub const MAX_RULES: usize = 24;
/// Longest statefs prefix (mirrors `MAX_PATH_PREFIX_BYTES`).
pub const MAX_PATH_PREFIX_BYTES: usize = 64;
/// Port ranges per rule (mirrors `MAX_PORT_RANGES_PER_RULE`).
pub const MAX_PORT_RANGES_PER_RULE: usize = 4;

/// Bytes that would read as pattern syntax; a prefix is a literal.
const PATTERN_BYTES: &[u8] = b"*?[]{}()|^$+\\";

/// `[abi_profile.<subject>]` as authored (v1 keys and v2 sections).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawAbiProfile {
    #[serde(default)]
    pub epoch: Option<u32>,
    #[serde(default)]
    pub limits: Option<RawLimits>,
    #[serde(default)]
    pub statefs: Vec<RawStatefsRule>,
    #[serde(default)]
    pub net: Option<RawNet>,
    /// v1 key (transcoded).
    #[serde(default)]
    pub statefs_put_allow_prefix: Option<String>,
    /// v1 key (transcoded).
    #[serde(default)]
    pub net_bind_min_port: Option<u16>,
}

/// `[abi_profile.<subject>.limits]`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawLimits {
    #[serde(default)]
    pub deadline_ms: Option<u32>,
    #[serde(default)]
    pub max_payload: Option<u32>,
}

/// `[[abi_profile.<subject>.statefs]]`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawStatefsRule {
    pub action: String,
    pub prefix: String,
    #[serde(default)]
    pub max_payload: Option<u32>,
}

/// `[abi_profile.<subject>.net]` container for `bind` / `connect` arrays.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawNet {
    #[serde(default)]
    pub bind: Vec<RawBindRule>,
    #[serde(default)]
    pub connect: Vec<RawConnectRule>,
}

/// `[[abi_profile.<subject>.net.bind]]`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawBindRule {
    pub action: String,
    pub ports: Vec<String>,
    #[serde(default)]
    pub address: Option<String>,
}

/// `[[abi_profile.<subject>.net.connect]]`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawConnectRule {
    pub action: String,
    pub cidr: String,
    pub ports: Vec<String>,
}

/// Rule verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Action {
    Deny,
    Allow,
}

/// `net.bind` address class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum AddressClass {
    Loopback,
    Any,
}

/// Inclusive port range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PortRange {
    pub min: u16,
    pub max: u16,
}

/// Subject-wide ceilings; `0` = unset.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Limits {
    pub deadline_ms: u32,
    pub max_payload: u32,
}

/// One compiled rule (authored order preserved; precedence is order-free).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum Rule {
    Statefs { action: Action, prefix: String, max_payload: u32 },
    NetBind { action: Action, address: AddressClass, ports: Vec<PortRange> },
    NetConnect { action: Action, cidr: [u8; 4], cidr_len: u8, ports: Vec<PortRange> },
}

/// A validated, canonical subject profile.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Profile {
    pub epoch: u32,
    pub limits: Option<Limits>,
    pub rules: Vec<Rule>,
}

/// Schema rejects (stable vocabulary; every variant is a parse error).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaError {
    MixedVersions,
    EmptyPrefix,
    PrefixNotAbsolute(String),
    PrefixNotCanonical(String),
    PrefixTooLong { len: usize, max: usize },
    PrefixPatternSyntax { prefix: String, byte: char },
    UnknownAction(String),
    UnknownAddress(String),
    BadPort(String),
    NoPortRanges,
    TooManyPortRanges { count: usize, max: usize },
    BadCidr(String),
    TooManyRules { count: usize, max: usize },
    RuleLimitAboveProfile { rule: u32, profile: u32 },
}

impl fmt::Display for SchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MixedVersions => write!(f, "profile mixes v1 keys with v2 sections"),
            Self::EmptyPrefix => write!(f, "statefs prefix is empty"),
            Self::PrefixNotAbsolute(p) => write!(f, "statefs prefix is not absolute: {p:?}"),
            Self::PrefixNotCanonical(p) => write!(f, "statefs prefix is not canonical: {p:?}"),
            Self::PrefixTooLong { len, max } => {
                write!(f, "statefs prefix too long: len={len} max={max}")
            }
            Self::PrefixPatternSyntax { prefix, byte } => {
                write!(f, "statefs prefix {prefix:?} contains pattern syntax {byte:?}")
            }
            Self::UnknownAction(a) => write!(f, "unknown action {a:?} (allow|deny)"),
            Self::UnknownAddress(a) => write!(f, "unknown address {a:?} (loopback|any)"),
            Self::BadPort(p) => write!(f, "bad port spec {p:?} (\"n\" or \"a-b\", 1..=65535)"),
            Self::NoPortRanges => write!(f, "rule has no port ranges"),
            Self::TooManyPortRanges { count, max } => {
                write!(f, "too many port ranges: {count} > {max}")
            }
            Self::BadCidr(c) => write!(f, "bad IPv4 CIDR {c:?}"),
            Self::TooManyRules { count, max } => write!(f, "too many rules: {count} > {max}"),
            Self::RuleLimitAboveProfile { rule, profile } => {
                write!(f, "rule max_payload {rule} exceeds limits.max_payload {profile}")
            }
        }
    }
}

impl std::error::Error for SchemaError {}

fn parse_action(s: &str) -> Result<Action, SchemaError> {
    match s.trim() {
        "allow" => Ok(Action::Allow),
        "deny" => Ok(Action::Deny),
        other => Err(SchemaError::UnknownAction(other.to_string())),
    }
}

fn parse_address(s: Option<&str>) -> Result<AddressClass, SchemaError> {
    match s.map(str::trim) {
        None | Some("loopback") => Ok(AddressClass::Loopback),
        Some("any") => Ok(AddressClass::Any),
        Some(other) => Err(SchemaError::UnknownAddress(other.to_string())),
    }
}

fn parse_port(s: &str, spec: &str) -> Result<u16, SchemaError> {
    let v: u16 = s.parse().map_err(|_| SchemaError::BadPort(spec.to_string()))?;
    if v == 0 {
        return Err(SchemaError::BadPort(spec.to_string()));
    }
    Ok(v)
}

/// `"n"` or `"a-b"` (inclusive, ascending, 1..=65535).
pub fn parse_port_range(spec: &str) -> Result<PortRange, SchemaError> {
    let s = spec.trim();
    let (min, max) = match s.split_once('-') {
        Some((a, b)) => (parse_port(a.trim(), spec)?, parse_port(b.trim(), spec)?),
        None => {
            let p = parse_port(s, spec)?;
            (p, p)
        }
    };
    if min > max {
        return Err(SchemaError::BadPort(spec.to_string()));
    }
    Ok(PortRange { min, max })
}

fn parse_ports(list: &[String]) -> Result<Vec<PortRange>, SchemaError> {
    if list.is_empty() {
        return Err(SchemaError::NoPortRanges);
    }
    if list.len() > MAX_PORT_RANGES_PER_RULE {
        return Err(SchemaError::TooManyPortRanges {
            count: list.len(),
            max: MAX_PORT_RANGES_PER_RULE,
        });
    }
    list.iter().map(|s| parse_port_range(s)).collect()
}

/// `"a.b.c.d/n"` with canonical host bits (`10.0.2.1/24` is rejected).
pub fn parse_cidr(spec: &str) -> Result<([u8; 4], u8), SchemaError> {
    let bad = || SchemaError::BadCidr(spec.to_string());
    let (addr, len) = spec.trim().split_once('/').ok_or_else(bad)?;
    let len: u8 = len.parse().map_err(|_| bad())?;
    if len > 32 {
        return Err(bad());
    }
    let mut octets = [0u8; 4];
    let mut count = 0;
    for part in addr.split('.') {
        if count == 4 || part.is_empty() || part.len() > 3 {
            return Err(bad());
        }
        octets[count] = part.parse().map_err(|_| bad())?;
        count += 1;
    }
    if count != 4 {
        return Err(bad());
    }
    let mask = if len == 0 { 0 } else { u32::MAX << (32 - u32::from(len)) };
    if u32::from_be_bytes(octets) & !mask != 0 {
        return Err(bad());
    }
    Ok((octets, len))
}

/// A bounded literal, absolute, canonical prefix; pattern bytes rejected.
pub fn validate_prefix(prefix: &str) -> Result<String, SchemaError> {
    let p = prefix.trim();
    if p.is_empty() {
        return Err(SchemaError::EmptyPrefix);
    }
    if !p.starts_with('/') {
        return Err(SchemaError::PrefixNotAbsolute(p.to_string()));
    }
    if p.len() > MAX_PATH_PREFIX_BYTES {
        return Err(SchemaError::PrefixTooLong { len: p.len(), max: MAX_PATH_PREFIX_BYTES });
    }
    if let Some(b) =
        p.bytes().find(|b| PATTERN_BYTES.contains(b) || b.is_ascii_control() || *b == b' ')
    {
        return Err(SchemaError::PrefixPatternSyntax { prefix: p.to_string(), byte: b as char });
    }
    // Segment-wise canonical: no empty, `.` or `..` segments (a trailing
    // `/` is the "directory prefix" form and is allowed).
    let body = p.trim_end_matches('/');
    if body.len() + 1 < p.len() {
        return Err(SchemaError::PrefixNotCanonical(p.to_string()));
    }
    if body.len() > 1 && body[1..].split('/').any(|seg| seg.is_empty() || seg == "." || seg == "..")
    {
        return Err(SchemaError::PrefixNotCanonical(p.to_string()));
    }
    Ok(p.to_string())
}

/// Validates and canonicalizes one authored profile.
pub fn compile(raw: &RawAbiProfile) -> Result<Profile, SchemaError> {
    let has_v1 = raw.statefs_put_allow_prefix.is_some() || raw.net_bind_min_port.is_some();
    let has_v2 =
        raw.epoch.is_some() || raw.limits.is_some() || !raw.statefs.is_empty() || raw.net.is_some();
    if has_v1 && has_v2 {
        return Err(SchemaError::MixedVersions);
    }
    let mut profile = Profile::default();
    if has_v1 {
        if let Some(prefix) = raw.statefs_put_allow_prefix.as_deref() {
            let prefix = prefix.trim();
            if !prefix.is_empty() {
                profile.rules.push(Rule::Statefs {
                    action: Action::Allow,
                    prefix: validate_prefix(prefix)?,
                    max_payload: 0,
                });
            }
        }
        if let Some(min) = raw.net_bind_min_port {
            profile.rules.push(Rule::NetBind {
                action: Action::Allow,
                address: AddressClass::Loopback,
                ports: vec![PortRange { min: min.max(1), max: u16::MAX }],
            });
        }
        return Ok(profile);
    }
    profile.epoch = raw.epoch.unwrap_or(0);
    let mut profile_max_payload = 0;
    if let Some(l) = &raw.limits {
        let limits = Limits {
            deadline_ms: l.deadline_ms.unwrap_or(0),
            max_payload: l.max_payload.unwrap_or(0),
        };
        profile_max_payload = limits.max_payload;
        profile.limits = Some(limits);
    }
    for r in &raw.statefs {
        let max_payload = r.max_payload.unwrap_or(0);
        if profile_max_payload != 0 && max_payload > profile_max_payload {
            return Err(SchemaError::RuleLimitAboveProfile {
                rule: max_payload,
                profile: profile_max_payload,
            });
        }
        profile.rules.push(Rule::Statefs {
            action: parse_action(&r.action)?,
            prefix: validate_prefix(&r.prefix)?,
            max_payload,
        });
    }
    if let Some(net) = &raw.net {
        for r in &net.bind {
            profile.rules.push(Rule::NetBind {
                action: parse_action(&r.action)?,
                address: parse_address(r.address.as_deref())?,
                ports: parse_ports(&r.ports)?,
            });
        }
        for r in &net.connect {
            let (cidr, cidr_len) = parse_cidr(&r.cidr)?;
            profile.rules.push(Rule::NetConnect {
                action: parse_action(&r.action)?,
                cidr,
                cidr_len,
                ports: parse_ports(&r.ports)?,
            });
        }
    }
    if profile.rules.len() > MAX_RULES {
        return Err(SchemaError::TooManyRules { count: profile.rules.len(), max: MAX_RULES });
    }
    Ok(profile)
}
