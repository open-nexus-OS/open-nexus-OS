// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RFC-0092 §1 `[[expose."<subject>"]]` grammar — the ONE exposure
//! model policyd (authority), ingressd (data plane) and `nx` (validation)
//! compile. Sibling of `schema.rs`: the host `policy` crate uses it as a
//! module; policyd's and ingressd's `build.rs` include it by path so no
//! build-time table can drift from the host grammar. Every bound is a
//! parse error, never a truncation; `tls != "none"` is refused until the
//! network track delivers termination (`ExposeTlsUnsupported`) — never a
//! silent downgrade. Depends on `serde` and `schema::parse_cidr` only.
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: tests/schema_corpus.rs (`ok_expose`, `reject_expose_*`),
//!   tests/ingress_host/
//! RFC: docs/rfcs/RFC-0092-service-exposure-contract-ingress-policy-ingressd.md

use serde::{Deserialize, Serialize};

use super::schema::{parse_cidr, SchemaError};

/// Exposures one subject may declare (RFC-0092 invariants).
pub const MAX_EXPOSURES_PER_SUBJECT: usize = 8;
/// CIDRs per exposure.
pub const MAX_CIDRS_PER_EXPOSE: usize = 8;
/// Exposures a build carries in total (= open exposures per boot).
pub const MAX_EXPOSURES_TOTAL: usize = 64;
/// `rate_per_s` ceiling.
pub const MAX_RATE_PER_S: u32 = 10_000;
/// `burst` ceiling.
pub const MAX_BURST: u32 = 1_000;

/// `[[expose."<subject>"]]` as authored.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawExpose {
    pub port: u16,
    pub proto: String,
    pub cidr_allow: Vec<String>,
    pub rate_per_s: u32,
    pub burst: u32,
    /// Contract slot; `None` = `"none"`.
    #[serde(default)]
    pub tls: Option<String>,
    pub backend: u16,
}

/// Transport of an exposure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum Proto {
    Tcp,
    Udp,
}

impl Proto {
    /// Stable label (`tcp` | `udp`) for markers and tooling.
    pub fn label(self) -> &'static str {
        match self {
            Self::Tcp => "tcp",
            Self::Udp => "udp",
        }
    }
}

/// Termination slot (RFC-0092 §4). Only `None` compiles today.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum TlsSlot {
    None,
    Tls,
    Mtls,
}

/// One IPv4 allow-list entry with canonical host bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Cidr {
    pub addr: [u8; 4],
    pub len: u8,
}

/// A validated exposure declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Expose {
    pub port: u16,
    pub proto: Proto,
    pub cidr_allow: Vec<Cidr>,
    pub rate_per_s: u32,
    pub burst: u32,
    pub tls: TlsSlot,
    pub backend: u16,
}

fn parse_proto(s: &str) -> Result<Proto, SchemaError> {
    match s.trim() {
        "tcp" => Ok(Proto::Tcp),
        "udp" => Ok(Proto::Udp),
        other => Err(SchemaError::ExposeUnknownProto(other.to_string())),
    }
}

fn parse_tls(s: Option<&str>) -> Result<TlsSlot, SchemaError> {
    match s.map(str::trim) {
        None | Some("none") => Ok(TlsSlot::None),
        Some(slot @ ("tls" | "mtls")) => Err(SchemaError::ExposeTlsUnsupported(slot.to_string())),
        Some(other) => Err(SchemaError::ExposeUnknownTls(other.to_string())),
    }
}

/// Validates one authored exposure.
pub fn compile_expose(raw: &RawExpose) -> Result<Expose, SchemaError> {
    if raw.port == 0 {
        return Err(SchemaError::ExposeBadPort { field: "port" });
    }
    if raw.backend == 0 {
        return Err(SchemaError::ExposeBadPort { field: "backend" });
    }
    let proto = parse_proto(&raw.proto)?;
    if raw.cidr_allow.is_empty() {
        return Err(SchemaError::ExposeNoCidrs);
    }
    if raw.cidr_allow.len() > MAX_CIDRS_PER_EXPOSE {
        return Err(SchemaError::ExposeTooManyCidrs {
            count: raw.cidr_allow.len(),
            max: MAX_CIDRS_PER_EXPOSE,
        });
    }
    let mut cidr_allow = Vec::with_capacity(raw.cidr_allow.len());
    for spec in &raw.cidr_allow {
        let (addr, len) = parse_cidr(spec)?;
        cidr_allow.push(Cidr { addr, len });
    }
    if raw.rate_per_s == 0
        || raw.rate_per_s > MAX_RATE_PER_S
        || raw.burst == 0
        || raw.burst > MAX_BURST
    {
        return Err(SchemaError::ExposeRate { rate_per_s: raw.rate_per_s, burst: raw.burst });
    }
    let tls = parse_tls(raw.tls.as_deref())?;
    Ok(Expose {
        port: raw.port,
        proto,
        cidr_allow,
        rate_per_s: raw.rate_per_s,
        burst: raw.burst,
        tls,
        backend: raw.backend,
    })
}

/// Validates every exposure of `subject` (bounded per subject; duplicates
/// within the subject are rejected here, across subjects by
/// [`check_exposes_unique`]).
pub fn compile_exposes(subject: &str, raw: &[RawExpose]) -> Result<Vec<Expose>, SchemaError> {
    if raw.len() > MAX_EXPOSURES_PER_SUBJECT {
        return Err(SchemaError::ExposeTooMany {
            subject: subject.to_string(),
            count: raw.len(),
            max: MAX_EXPOSURES_PER_SUBJECT,
        });
    }
    let mut out: Vec<Expose> = Vec::with_capacity(raw.len());
    for r in raw {
        let e = compile_expose(r)?;
        if out.iter().any(|x| x.port == e.port && x.proto == e.proto) {
            return Err(SchemaError::ExposeDuplicate { port: e.port, proto: e.proto.label() });
        }
        out.push(e);
    }
    Ok(out)
}

/// `(port, proto)` is unique across ALL subjects and the total is bounded —
/// a NIC-facing port has exactly one owner.
pub fn check_exposes_unique<'a, I>(all: I) -> Result<(), SchemaError>
where
    I: IntoIterator<Item = (&'a str, &'a [Expose])>,
{
    let mut seen: Vec<(u16, Proto)> = Vec::new();
    for (_subject, exposes) in all {
        for e in exposes {
            if seen.contains(&(e.port, e.proto)) {
                return Err(SchemaError::ExposeDuplicate { port: e.port, proto: e.proto.label() });
            }
            seen.push((e.port, e.proto));
        }
    }
    if seen.len() > MAX_EXPOSURES_TOTAL {
        return Err(SchemaError::ExposeTooManyTotal {
            count: seen.len(),
            max: MAX_EXPOSURES_TOTAL,
        });
    }
    Ok(())
}
