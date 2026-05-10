// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Deny-by-default secret scanner for evidence bundles
//! (P5-04). Runs before [`crate::Bundle::seal`] so that any bundle
//! with leaked key material refuses to seal — no signed bundle ever
//! commits a secret to the evidence stream.
//!
//! Scanned artifacts:
//!   - `uart.log` (raw bytes; line-by-line)
//!   - `trace.jsonl` (each entry's `marker` field)
//!   - `config.json` (selected text fields: `kernel_cmdline`,
//!     `qemu_args`, `host_info`, `env` values)
//!
//! Patterns (all reject; first hit wins for the diagnostic):
//!   1. PEM private-key headers: `BEGIN (RSA|EC|OPENSSH|PGP|DSA)
//!      PRIVATE KEY`.
//!   2. Bringup key path leak: literal `bringup-key/private` (covers
//!      `~/.config/nexus/bringup-key/private.ed25519` and any
//!      reasonable variation).
//!   3. KEY-style env-var assignment with a long secret-looking
//!      tail: `^.*PRIVATE_KEY.*=.*[A-Za-z0-9+/=]{40,}`.
//!   4. High-entropy base64-looking blob ≥ 64 chars on a single line.
//!      The heuristic is conservative: it requires the blob to span
//!      ≥64 contiguous base64-alphabet bytes and to NOT match the
//!      allowlist in [`scan.toml`].
//!
//! Allowlist: `source/libs/nexus-evidence/scan.toml` carries an
//! `[allowlist]` table of literal substrings that suppress pattern
//! 4 (the high-entropy heuristic) on a per-fragment basis. P5-04
//! ships an empty allowlist; entries land case-by-case as
//! false-positives surface against the real `uart.log`.
//!
//! OWNERS: @runtime
//! STATUS: Functional (P5-04 surface)
//! API_STABILITY: Unstable (Phase 5 evolves shape between cuts)
//! TEST_COVERAGE: see `tests/scan.rs` (5 tests)

use crate::{Bundle, EvidenceError};

/// Result of a single allowlist load. Empty by default; populated
/// from the `[allowlist] substrings = [...]` array in `scan.toml`.
#[derive(Debug, Clone, Default)]
pub struct ScanAllowlist {
    substrings: Vec<String>,
}

impl ScanAllowlist {
    /// Empty allowlist; all heuristics fire unmodified.
    pub fn empty() -> Self {
        ScanAllowlist::default()
    }

    /// Parse a `scan.toml` body. The expected schema is:
    ///
    /// ```toml
    /// [allowlist]
    /// substrings = ["known-benign-fragment", ...]
    /// ```
    ///
    /// Unknown keys are tolerated (the file is consumer-side
    /// configuration, not a manifest); structural failure returns
    /// [`EvidenceError::CanonicalizationFailed`].
    pub fn from_toml(src: &str) -> Result<Self, EvidenceError> {
        let mut subs: Vec<String> = Vec::new();
        let mut in_allowlist = false;
        for raw_line in src.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                in_allowlist = line == "[allowlist]";
                continue;
            }
            if !in_allowlist {
                continue;
            }
            if let Some(rest) = line.strip_prefix("substrings") {
                let rest = rest.trim_start_matches([' ', '=', '\t']);
                if !rest.starts_with('[') || !rest.ends_with(']') {
                    return Err(EvidenceError::CanonicalizationFailed {
                        detail: format!("scan_allowlist: bad substrings line `{}`", line),
                    });
                }
                let inner = &rest[1..rest.len() - 1];
                for fragment in inner.split(',') {
                    let f = fragment.trim();
                    if f.is_empty() {
                        continue;
                    }
                    let unq = f.trim_matches(|c| c == '"' || c == '\'');
                    if !unq.is_empty() {
                        subs.push(unq.to_string());
                    }
                }
            }
        }
        Ok(ScanAllowlist { substrings: subs })
    }

    fn is_allowed(&self, fragment: &str) -> bool {
        self.substrings.iter().any(|s| fragment.contains(s.as_str()))
    }
}

/// Reason a secret-scan rejection fired. Encoded into the
/// [`EvidenceError::SecretLeak`] payload so the operator's runbook
/// can branch on the cause.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeakKind {
    /// PEM private-key block ("BEGIN ... PRIVATE KEY").
    PemPrivateKey,
    /// Literal bringup-key/private path leak.
    BringupKeyPath,
    /// `*PRIVATE_KEY*=…` env-style assignment with a long tail.
    PrivateKeyEnvAssignment,
    /// High-entropy base64-looking blob (≥64 contiguous chars).
    HighEntropyBlob,
}

impl LeakKind {
    /// Stable short label used in error diagnostics + runbooks.
    pub fn as_str(self) -> &'static str {
        match self {
            LeakKind::PemPrivateKey => "pem_private_key",
            LeakKind::BringupKeyPath => "bringup_key_path",
            LeakKind::PrivateKeyEnvAssignment => "private_key_env_assignment",
            LeakKind::HighEntropyBlob => "high_entropy_blob",
        }
    }
}

/// Run all scanners against the textual artifacts of `bundle`.
///
/// Returns `Ok(())` only if every scan passed. The first hit
/// triggers an [`EvidenceError::SecretLeak`] carrying the artifact
/// name, the offending line number (1-indexed), and the matched
/// [`LeakKind`]. The function does NOT redact or mutate the bundle —
/// surfacing the leak is the caller's responsibility.
pub fn scan_for_secrets(bundle: &Bundle) -> Result<(), EvidenceError> {
    scan_for_secrets_with(bundle, &ScanAllowlist::empty())
}

/// Same as [`scan_for_secrets`] but with a caller-supplied
/// allowlist (e.g. parsed from `scan.toml`). Used by tests and by
/// callers that want to suppress known-benign patterns surgically.
pub fn scan_for_secrets_with(
    bundle: &Bundle,
    allowlist: &ScanAllowlist,
) -> Result<(), EvidenceError> {
    // `uart.log` (line-by-line; raw bytes lossily rendered to UTF-8
    // for scanning — non-UTF-8 sequences become `?` and don't match
    // any pattern, which is the safe default).
    let uart_text = String::from_utf8_lossy(&bundle.uart.bytes);
    for (lineno, line) in uart_text.lines().enumerate() {
        scan_text("uart.log", lineno + 1, line, allowlist)?;
    }

    for (idx, entry) in bundle.trace.entries.iter().enumerate() {
        scan_text("trace.jsonl", idx + 1, &entry.marker, allowlist)?;
    }

    let cfg = &bundle.config;
    scan_text("config.json", 1, &cfg.kernel_cmdline, allowlist)?;
    for (i, arg) in cfg.qemu_args.iter().enumerate() {
        scan_text("config.json", 100 + i, arg, allowlist)?;
    }
    scan_text("config.json", 200, &cfg.host_info, allowlist)?;
    for (lineno, (k, v)) in cfg.env.iter().enumerate() {
        let joined = format!("{}={}", k, v);
        scan_text("config.json", 300 + lineno, &joined, allowlist)?;
    }
    Ok(())
}

fn scan_text(
    artifact: &'static str,
    line: usize,
    text: &str,
    allowlist: &ScanAllowlist,
) -> Result<(), EvidenceError> {
    if let Some(kind) = match_pem_private_key(text) {
        return Err(leak(artifact, line, kind));
    }
    if let Some(kind) = match_bringup_key_path(text) {
        return Err(leak(artifact, line, kind));
    }
    if let Some(kind) = match_private_key_env(text) {
        return Err(leak(artifact, line, kind));
    }
    if let Some(kind) = match_high_entropy_blob(text, allowlist) {
        return Err(leak(artifact, line, kind));
    }
    Ok(())
}

fn match_pem_private_key(text: &str) -> Option<LeakKind> {
    if !text.contains("PRIVATE KEY") {
        return None;
    }
    if !text.contains("BEGIN ") {
        return None;
    }
    for kw in &["RSA", "EC", "OPENSSH", "PGP", "DSA"] {
        let needle = format!("BEGIN {} PRIVATE KEY", kw);
        if text.contains(&needle) {
            return Some(LeakKind::PemPrivateKey);
        }
    }
    if text.contains("BEGIN PRIVATE KEY") {
        return Some(LeakKind::PemPrivateKey);
    }
    None
}

fn match_bringup_key_path(text: &str) -> Option<LeakKind> {
    if text.contains("bringup-key/private") {
        Some(LeakKind::BringupKeyPath)
    } else {
        None
    }
}

fn match_private_key_env(text: &str) -> Option<LeakKind> {
    let upper = text.to_ascii_uppercase();
    let key_idx = upper.find("PRIVATE_KEY")?;
    let after_key = &text[key_idx + "PRIVATE_KEY".len()..];
    let eq_idx = after_key.find('=')?;
    let after_eq = &after_key[eq_idx + 1..];
    let mut run = 0usize;
    for ch in after_eq.chars() {
        if is_b64_char(ch) {
            run += 1;
            if run >= 40 {
                return Some(LeakKind::PrivateKeyEnvAssignment);
            }
        } else {
            run = 0;
        }
    }
    None
}

fn match_high_entropy_blob(text: &str, allowlist: &ScanAllowlist) -> Option<LeakKind> {
    let bytes = text.as_bytes();
    let mut start = 0usize;
    let mut run = 0usize;
    for (i, &b) in bytes.iter().enumerate() {
        if is_b64_byte(b) {
            if run == 0 {
                start = i;
            }
            run += 1;
        } else {
            if run >= 64 {
                let fragment = &text[start..i];
                if !allowlist.is_allowed(fragment) {
                    return Some(LeakKind::HighEntropyBlob);
                }
            }
            run = 0;
        }
    }
    if run >= 64 {
        let fragment = &text[start..];
        if !allowlist.is_allowed(fragment) {
            return Some(LeakKind::HighEntropyBlob);
        }
    }
    None
}

fn is_b64_char(c: char) -> bool {
    matches!(c, 'A'..='Z' | 'a'..='z' | '0'..='9' | '+' | '/' | '=')
}

fn is_b64_byte(b: u8) -> bool {
    matches!(b, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'+' | b'/' | b'=')
}

fn leak(artifact: &'static str, line: usize, kind: LeakKind) -> EvidenceError {
    EvidenceError::SecretLeak { artifact, line, pattern: kind.as_str() }
}
