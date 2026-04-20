// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Canonicalization helpers for the evidence-bundle hash.
//! Each function is a pure transformation from a typed artifact to the
//! exact byte sequence that will be SHA-256'd. The canonical form is
//! frozen at P5-01 — see `docs/testing/evidence-bundle.md` §3.2 for
//! the normative spec.
//!
//! Hard invariants (locked by `tests/canonical_hash.rs`):
//!   - Trace serialization is order-invariant in input (sorted internally).
//!   - Config serialization is env-key-order-invariant (`BTreeMap`-backed).
//!   - UART normalization collapses `\r\n` → `\n`, nothing else.
//!   - `wall_clock_utc` is excluded from `serialize_config_canonical`.
//!
//! OWNERS: @runtime
//! STATUS: Functional (P5-01 surface)
//! API_STABILITY: Unstable (Phase 5 evolves shape between cuts)
//! TEST_COVERAGE: see `tests/canonical_hash.rs` (6 tests)

use crate::{BundleMeta, ConfigArtifact, TraceEntry};

/// Encode `meta` into the exact UTF-8 byte sequence the canonical hash
/// consumes (`schema_version=<u8>\nprofile=<str>\n`). No quoting, no
/// surrounding whitespace.
pub(crate) fn serialize_meta_canonical(meta: &BundleMeta) -> Vec<u8> {
    let mut out = String::with_capacity(48 + meta.profile.len());
    out.push_str("schema_version=");
    out.push_str(&meta.schema_version.to_string());
    out.push('\n');
    out.push_str("profile=");
    out.push_str(&meta.profile);
    out.push('\n');
    out.into_bytes()
}

/// Normalize UART bytes: collapse every `\r\n` to `\n`. Bare `\r` is
/// preserved as-is (Phase-5 UART output is ASCII-line-oriented; bare
/// `\r` is a deliberate carriage return that downstream tools may want
/// to see). No other transformation.
pub(crate) fn normalize_line_endings(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\r' && i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
            out.push(b'\n');
            i += 2;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

/// Serialize trace entries into the canonical newline-separated JSON
/// form. Sort key is `(marker, phase)`. Output has no trailing newline
/// after the last entry; an empty input produces an empty `Vec`.
pub(crate) fn serialize_trace_canonical(entries: &[TraceEntry]) -> Vec<u8> {
    let mut sorted: Vec<&TraceEntry> = entries.iter().collect();
    // Sort by (marker, phase) byte-lex; stable for equal keys.
    sorted.sort_by(|a, b| {
        a.marker
            .as_bytes()
            .cmp(b.marker.as_bytes())
            .then_with(|| a.phase.as_bytes().cmp(b.phase.as_bytes()))
    });

    let mut out = Vec::new();
    for (idx, entry) in sorted.iter().enumerate() {
        if idx > 0 {
            out.push(b'\n');
        }
        // Hand-rolled emitter: fixed key order, compact form, `null`
        // for `None` ts. Using serde_json::to_string here would be
        // easier, but we emit by hand to lock the exact layout the
        // spec promises. JSON-string escaping delegates to serde_json
        // (Value::String) so we stay correct under control characters.
        out.extend_from_slice(b"{\"marker\":");
        out.extend_from_slice(json_escape_string(&entry.marker).as_bytes());
        out.extend_from_slice(b",\"phase\":");
        out.extend_from_slice(json_escape_string(&entry.phase).as_bytes());
        out.extend_from_slice(b",\"ts_ms_from_boot\":");
        match entry.ts_ms_from_boot {
            Some(ts) => {
                let s = ts.to_string();
                out.extend_from_slice(s.as_bytes());
            }
            None => out.extend_from_slice(b"null"),
        }
        out.extend_from_slice(b",\"profile\":");
        out.extend_from_slice(json_escape_string(&entry.profile).as_bytes());
        out.push(b'}');
    }
    out
}

/// Serialize the config artifact for hashing. `wall_clock_utc` is
/// excluded; remaining fields are emitted in the spec's fixed order.
/// `env` is a `BTreeMap` so its iteration order (and serde_json's
/// emission order) is lexicographic on keys — guaranteeing
/// env-key-order-invariance of the hash.
pub(crate) fn serialize_config_canonical(config: &ConfigArtifact) -> Vec<u8> {
    // We emit by hand (rather than `#[derive(Serialize)]`) for two
    // reasons: (1) `wall_clock_utc` must be omitted, and (2) field
    // order must be locked to the spec — `serde_json` honors struct
    // field order, but a future field reorder in the source must NOT
    // silently change the hash. The hand-rolled emitter is the lock.
    let mut out = Vec::new();
    out.push(b'{');

    out.extend_from_slice(b"\"profile\":");
    out.extend_from_slice(json_escape_string(&config.profile).as_bytes());

    out.extend_from_slice(b",\"env\":");
    // Use serde_json on the BTreeMap directly — guaranteed sorted-key
    // emission, compact form (no whitespace). Serializing
    // `BTreeMap<String,String>` is infallible (no Serialize errors are
    // reachable for plain string→string maps); we emit `{}` if the
    // (impossible) error path ever fires.
    #[allow(clippy::unwrap_or_default)]
    let env_json = serde_json::to_string(&config.env).unwrap_or_else(|_| "{}".to_string());
    out.extend_from_slice(env_json.as_bytes());

    out.extend_from_slice(b",\"kernel_cmdline\":");
    out.extend_from_slice(json_escape_string(&config.kernel_cmdline).as_bytes());

    out.extend_from_slice(b",\"qemu_args\":");
    // Same infallibility argument as `env` above (Vec<String>).
    let qemu_args_json =
        serde_json::to_string(&config.qemu_args).unwrap_or_else(|_| "[]".to_string());
    out.extend_from_slice(qemu_args_json.as_bytes());

    out.extend_from_slice(b",\"host_info\":");
    out.extend_from_slice(json_escape_string(&config.host_info).as_bytes());

    out.extend_from_slice(b",\"build_sha\":");
    out.extend_from_slice(json_escape_string(&config.build_sha).as_bytes());

    out.extend_from_slice(b",\"rustc_version\":");
    out.extend_from_slice(json_escape_string(&config.rustc_version).as_bytes());

    out.extend_from_slice(b",\"qemu_version\":");
    out.extend_from_slice(json_escape_string(&config.qemu_version).as_bytes());

    out.push(b'}');
    out
}

/// Emit a JSON-escaped, double-quoted string for `s` using
/// `serde_json`'s default escaping rules. Returns the literal `"..."`.
///
/// Serializing a `&str` is infallible in `serde_json`; we fall back to
/// the empty-string literal in the unreachable error path so we keep
/// output deterministic (and clippy happy under `-D expect_used`).
fn json_escape_string(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string())
}
