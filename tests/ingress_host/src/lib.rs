// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: host proofs of the RFC-0092 ingress gateway (TASK-0052 P2):
//! the shared `[[expose]]` grammar feeds ingressd's registry, and the five
//! `test_reject_*` plus the allow path live in `tests/`. This lib carries
//! the fixture → table bridge (grammar output → `ExposeEntry`) and a scripted
//! policyd seam so every verdict is deterministic.
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! TEST_COVERAGE: tests/ingress.rs

#![forbid(unsafe_code)]

use std::collections::BTreeMap;

use ingressd::intent::{HostError, IntentHost};
use ingressd::table::{Cidr, ExposeEntry, Proto};
use nexus_policy::expose::{self, RawExpose};
use nexus_policy::learn_record::service_id_from_name;
use serde::Deserialize;

/// `[[expose."<subject>"]]` tables of a TOML fixture.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    #[serde(default)]
    expose: BTreeMap<String, Vec<RawExpose>>,
}

/// FNV-1a subject id (the one policy hash).
pub fn sid(name: &str) -> u64 {
    service_id_from_name(name.trim().to_ascii_lowercase().as_bytes())
}

/// Compiles a fixture through the shared grammar (including the cross-
/// subject uniqueness rule) into a gateway table, canonical order.
pub fn table_from_toml(text: &str) -> Result<&'static [ExposeEntry], String> {
    let fixture: Fixture = toml::from_str(text).map_err(|e| e.to_string())?;
    let mut compiled: BTreeMap<String, Vec<expose::Expose>> = BTreeMap::new();
    for (subject, raw) in fixture.expose {
        let list = expose::compile_exposes(&subject, &raw).map_err(|e| e.to_string())?;
        compiled.insert(subject.trim().to_ascii_lowercase(), list);
    }
    expose::check_exposes_unique(compiled.iter().map(|(k, v)| (k.as_str(), v.as_slice())))
        .map_err(|e| e.to_string())?;
    let mut entries = Vec::new();
    for (subject, list) in &compiled {
        for e in list {
            let cidrs: Vec<Cidr> =
                e.cidr_allow.iter().map(|c| Cidr { addr: c.addr, len: c.len }).collect();
            entries.push(ExposeEntry {
                subject_id: sid(subject),
                port: e.port,
                proto: match e.proto {
                    expose::Proto::Tcp => Proto::Tcp,
                    expose::Proto::Udp => Proto::Udp,
                },
                cidr_allow: Box::leak(cidrs.into_boxed_slice()),
                rate_per_s: e.rate_per_s,
                burst: e.burst,
                backend: e.backend,
            });
        }
    }
    Ok(Box::leak(entries.into_boxed_slice()))
}

/// A scripted policyd: which subjects hold `net.expose`, and whether the
/// authority answers at all. Every query is recorded.
#[derive(Default)]
pub struct ScriptedPolicy {
    pub grants: Vec<u64>,
    pub reachable: bool,
    pub queries: Vec<u64>,
}

impl ScriptedPolicy {
    /// Reachable authority granting `net.expose` to `subjects`.
    pub fn granting(subjects: &[&str]) -> Self {
        Self { grants: subjects.iter().map(|s| sid(s)).collect(), reachable: true, queries: vec![] }
    }

    /// An authority that never answers (routing/IPC failure).
    pub fn unreachable() -> Self {
        Self { grants: vec![], reachable: false, queries: vec![] }
    }
}

impl IntentHost for ScriptedPolicy {
    fn expose_capability(&mut self, subject: u64) -> Result<bool, HostError> {
        self.queries.push(subject);
        if !self.reachable {
            return Err(HostError);
        }
        Ok(self.grants.contains(&subject))
    }
}
