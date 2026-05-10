// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Marker-ladder extraction from raw UART output (P5-02).
//! Reads `uart.log` line-by-line, parses the optional `[ts=…ms]`
//! prefix, and substring-matches the rest against every manifest
//! marker literal — the same approach the P4-09 `verify-uart`
//! analyzer uses, so the trace extractor sees the same set of
//! markers the analyzer guards. Real UART has log-level prefixes
//! (`[INFO selftest] KSELFTEST: …`) and ~25 distinct service prefixes
//! (`samgrd:`, `policyd:`, `vfsd:`, …); substring matching covers
//! them all uniformly.
//!
//! Hard rules:
//!   - `[ts=…ms]` prefix must parse as `[ts=<u64>ms] ` (literal `ms]`
//!     suffix and a single trailing space). Anything else is a
//!     malformed-trace error — never a silent `None`.
//!   - Lines that DON'T match any manifest literal are silently
//!     skipped (boot banners, harness logs, init progress, …).
//!     EXCEPT: a line whose body starts with `SELFTEST:` or
//!     `dsoftbusd:` MUST resolve to at least one manifest literal —
//!     orphan SELFTEST/dsoftbusd output is a hard error
//!     (`unknown_marker`). This mirrors the deny-by-default posture
//!     of `verify-uart`: every assertion-class marker must be
//!     declared.
//!   - `dbg:` lines are by spec excluded from the manifest universe
//!     (rule 12-debug-discipline) and therefore never appear in the
//!     trace.
//!
//! OWNERS: @runtime
//! STATUS: Functional (P5-02 surface)
//! API_STABILITY: Unstable (Phase 5 evolves shape between cuts)
//! TEST_COVERAGE: see `tests/assemble.rs` (5 tests)

use std::collections::BTreeMap;

use nexus_proof_manifest::Manifest;

use crate::{EvidenceError, TraceEntry};

/// Extract the marker ladder from raw UART text.
///
/// `uart` is the verbatim UART transcript (line endings are normalised
/// internally — both `\n` and `\r\n` are accepted as line separators).
/// `manifest` is the proof-manifest the run sealed against (used to
/// validate every recognised marker literal and to attach the declared
/// `phase` to each [`TraceEntry`]).
/// `profile` is the profile name the run executed under; it is copied
/// verbatim to every emitted entry.
///
/// Returns the trace entries in **UART appearance order**. Order does
/// NOT affect the canonical hash (the hash sorts internally), but
/// preserving it makes the resulting `trace.jsonl` easy to diff
/// against the raw UART transcript.
///
/// # Errors
///
/// - [`EvidenceError::MalformedTrace`] if a `[ts=…ms]` prefix is
///   present but unparseable, or if a recognised marker literal is
///   not declared in the manifest.
pub fn extract_trace(
    uart: &str,
    manifest: &Manifest,
    profile: &str,
) -> Result<Vec<TraceEntry>, EvidenceError> {
    // Build a literal -> phase index for O(1) lookup. Done once per
    // call; the manifest is small (<500 markers as of P5-00).
    let index: BTreeMap<&str, &str> =
        manifest.markers.iter().map(|m| (m.literal.as_str(), m.phase.as_str())).collect();

    // Iterate marker literals in length-descending order so that a
    // line containing both a long literal (`samgrd: ready`) and a
    // shorter one (`samgrd:`) emits the more specific match first.
    // Matches are deduplicated per (line, literal) pair so prefix
    // overlap doesn't cause double-counting when both literals are
    // declared.
    let mut literals: Vec<(&str, &str)> = index.iter().map(|(k, v)| (*k, *v)).collect();
    literals.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then(a.0.cmp(b.0)));

    let mut out = Vec::new();
    for raw_line in uart.split('\n') {
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        if line.is_empty() {
            continue;
        }

        let (ts_ms, body) = match parse_ts_prefix(line)? {
            Some((ts, rest)) => (Some(ts), rest),
            None => (None, line),
        };

        let mut matched_any = false;
        let mut seen: Vec<&str> = Vec::new();
        for (lit, phase) in &literals {
            if !body.contains(lit) {
                continue;
            }
            if seen.contains(lit) {
                continue;
            }
            seen.push(lit);
            matched_any = true;
            out.push(TraceEntry {
                marker: (*lit).to_string(),
                phase: (*phase).to_string(),
                ts_ms_from_boot: ts_ms,
                profile: profile.to_string(),
            });
        }

        // Deny-by-default: orphan SELFTEST/dsoftbusd lines are bugs.
        // Other prefixes (kernel banners, init: progress, etc.) are
        // benign noise — silently skipped.
        if !matched_any && (body.starts_with("SELFTEST:") || body.starts_with("dsoftbusd:")) {
            return Err(EvidenceError::MalformedTrace {
                detail: format!("unknown_marker `{}`", truncate_for_diag(body, 120)),
            });
        }
    }
    Ok(out)
}

/// If `line` starts with `[ts=<u64>ms] `, return `(ts, rest)`; else
/// return `None`. Returns an error for malformed prefixes (e.g.
/// `[ts=foo]`, `[ts=42ms]` without trailing space, `[ts=42`).
fn parse_ts_prefix(line: &str) -> Result<Option<(u64, &str)>, EvidenceError> {
    if !line.starts_with("[ts=") {
        return Ok(None);
    }
    // Find the `ms] ` terminator.
    let after_open = &line[4..];
    let close_idx = match after_open.find("ms] ") {
        Some(i) => i,
        None => {
            return Err(EvidenceError::MalformedTrace {
                detail: format!("malformed_ts_prefix `{}`", truncate_for_diag(line, 80)),
            });
        }
    };
    let digits = &after_open[..close_idx];
    let ts: u64 = digits.parse().map_err(|_| EvidenceError::MalformedTrace {
        detail: format!("malformed_ts_value `{}`", truncate_for_diag(digits, 32)),
    })?;
    let rest = &after_open[close_idx + 4..];
    Ok(Some((ts, rest)))
}

fn truncate_for_diag(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max).collect();
        t.push_str("...");
        t
    }
}
