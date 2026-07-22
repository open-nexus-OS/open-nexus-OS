// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RFC-0077 locale packaging — the `NXL1` index-aligned pack
//! encoder and the `NXLC` bundle-payload container built on top of
//! `project::compile_project_build`. Deterministic bytes (goldens in
//! `tests/dsl_goldens`); all bounds are BUILD errors (fail-closed).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: tests/dsl_goldens/tests/i18n_packs.rs
//! RFC: docs/rfcs/RFC-0077-i18n-v2-locale-packs-runtime-switch.md

use alloc::string::String;

#[cfg(feature = "std")]
use crate::project::compile_project_build;

/// A compiled project plus the metadata bundle packaging needs (RFC-0077).
#[cfg(feature = "std")]
pub struct ProjectBuild {
    /// Canonical NXIR bytes (the program).
    pub nxir: alloc::vec::Vec<u8>,
    /// The program's i18n keys in KEY-INDEX ORDER — locale packs are
    /// index-aligned to this table (key names are baked away in NXIR).
    pub i18n_keys: alloc::vec::Vec<String>,
}

/// RFC-0077 bounds: locale packs are UI-string tables, not documents.
pub const LOCALE_PACK_MAX_KEYS: usize = 4096;
/// Maximum bytes per translated template.
pub const LOCALE_PACK_MAX_TEMPLATE: usize = 4096;

/// Encodes an index-aligned `NXL1` locale pack (RFC-0077): one entry per
/// program i18n key, absent entries fall back to the baked default at
/// runtime. Deterministic bytes; bounds are BUILD errors (fail-closed).
pub fn encode_locale_pack(
    keys: &[String],
    entries: &alloc::collections::BTreeMap<String, String>,
) -> Result<alloc::vec::Vec<u8>, String> {
    if keys.len() > LOCALE_PACK_MAX_KEYS {
        return Err(alloc::format!("locale pack: {} keys > {LOCALE_PACK_MAX_KEYS}", keys.len()));
    }
    let mut out = alloc::vec::Vec::new();
    out.extend_from_slice(b"NXL1");
    out.extend_from_slice(&(keys.len() as u32).to_le_bytes());
    for key in keys {
        match entries.get(key) {
            Some(template) => {
                let bytes = template.as_bytes();
                if bytes.len() > LOCALE_PACK_MAX_TEMPLATE {
                    return Err(alloc::format!(
                        "locale pack: template for `{key}` exceeds {LOCALE_PACK_MAX_TEMPLATE} bytes"
                    ));
                }
                out.push(1);
                out.extend_from_slice(&(bytes.len() as u16).to_le_bytes());
                out.extend_from_slice(bytes);
            }
            None => out.push(0),
        }
    }
    Ok(out)
}

/// Compiles a project into its BUNDLE PAYLOAD (RFC-0077): the raw NXIR when
/// the app ships no locale catalogs, else an `NXLC` container carrying the
/// NXIR plus one index-aligned pack per `i18n/<tag>.json` (sorted by tag —
/// deterministic bytes). A malformed catalog fails the BUILD.
#[cfg(feature = "std")]
pub fn compile_project_bundle(root: &std::path::Path) -> Result<alloc::vec::Vec<u8>, String> {
    let build = compile_project_build(root)?;
    let i18n_dir = root.join("i18n");
    let mut packs: alloc::vec::Vec<(String, alloc::vec::Vec<u8>)> = alloc::vec::Vec::new();
    if i18n_dir.is_dir() {
        let entries = std::fs::read_dir(&i18n_dir)
            .map_err(|e| alloc::format!("read {}: {e}", i18n_dir.display()))?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let Some(tag) = path.file_stem().and_then(|n| n.to_str()).map(String::from) else {
                continue;
            };
            if tag.is_empty() || tag.len() > 16 || !tag.bytes().all(|b| b.is_ascii_alphanumeric()) {
                return Err(alloc::format!("locale tag `{tag}`: 1-16 ASCII alphanumerics"));
            }
            let text = std::fs::read_to_string(&path)
                .map_err(|e| alloc::format!("read {}: {e}", path.display()))?;
            let map = parse_flat_json_map(&text)
                .map_err(|e| alloc::format!("{}: {e}", path.display()))?;
            let pack = encode_locale_pack(&build.i18n_keys, &map)?;
            packs.push((tag, pack));
        }
    }
    if packs.is_empty() {
        return Ok(build.nxir); // legacy raw payload — pack-less apps unchanged
    }
    packs.sort_by(|a, b| a.0.cmp(&b.0));
    let mut out = alloc::vec::Vec::new();
    out.extend_from_slice(b"NXLC");
    out.push(1); // version
    out.extend_from_slice(&[0, 0, 0]); // reserved
    out.extend_from_slice(&(build.nxir.len() as u32).to_le_bytes());
    out.extend_from_slice(&[0, 0, 0, 0]); // pad: NXIR starts 8-ALIGNED (16)
    out.extend_from_slice(&build.nxir);
    out.push(packs.len() as u8);
    for (tag, pack) in &packs {
        out.push(tag.len() as u8);
        out.extend_from_slice(tag.as_bytes());
        out.extend_from_slice(&(pack.len() as u32).to_le_bytes());
        out.extend_from_slice(pack);
    }
    // Zero-pad to a multiple of 8: the bundle payload path requires
    // 8-byte-multiple lengths (same invariant as raw NXIR).
    while out.len() % 8 != 0 {
        out.push(0);
    }
    Ok(out)
}

/// Minimal flat-JSON parser for locale catalogs: ONE object of string→string
/// pairs (the catalog contract). Escapes: `\"`, `\\`, `\n`, `\t`. Anything
/// else — nesting, arrays, numbers, exotic escapes — is a loud error: the
/// catalog format is deliberately this small (no serde in the compiler core).
#[cfg(feature = "std")]
pub(crate) fn parse_flat_json_map(
    text: &str,
) -> Result<alloc::collections::BTreeMap<String, String>, String> {
    fn parse_string(
        chars: &mut core::iter::Peekable<core::str::Chars<'_>>,
    ) -> Result<String, String> {
        let mut out = String::new();
        loop {
            match chars.next() {
                Some('"') => return Ok(out),
                Some('\\') => match chars.next() {
                    Some('"') => out.push('"'),
                    Some('\\') => out.push('\\'),
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    other => return Err(alloc::format!("unsupported escape {other:?}")),
                },
                Some(c) => out.push(c),
                None => return Err(String::from("unterminated string")),
            }
        }
    }
    let mut map = alloc::collections::BTreeMap::new();
    let mut chars = text.chars().peekable();
    let mut expect = "{";
    loop {
        // Skip whitespace between tokens.
        while matches!(chars.peek(), Some(c) if c.is_whitespace()) {
            chars.next();
        }
        match (expect, chars.next()) {
            ("{", Some('{')) => expect = "key-or-end",
            ("key-or-end", Some('}')) | ("key-or-comma-end", Some('}')) => break,
            ("key-or-end", Some('"')) | ("key", Some('"')) => {
                let key = parse_string(&mut chars)?;
                while matches!(chars.peek(), Some(c) if c.is_whitespace()) {
                    chars.next();
                }
                if chars.next() != Some(':') {
                    return Err(alloc::format!("expected ':' after key {key:?}"));
                }
                while matches!(chars.peek(), Some(c) if c.is_whitespace()) {
                    chars.next();
                }
                if chars.next() != Some('"') {
                    return Err(alloc::format!("expected string value for key {key:?}"));
                }
                let value = parse_string(&mut chars)?;
                map.insert(key, value);
                expect = "key-or-comma-end";
            }
            ("key-or-comma-end", Some(',')) => expect = "key",
            (state, token) => {
                return Err(alloc::format!("unexpected {token:?} (expected {state})"));
            }
        }
    }
    Ok(map)
}

/// Loads the app's DEFAULT-locale catalog (`i18n/en.json`, a flat JSON object
/// of `"key": "text"`). Absent file = empty catalog; malformed = build error.
#[cfg(feature = "std")]
pub(crate) fn load_default_locale_catalog(
    root: &std::path::Path,
) -> Result<alloc::collections::BTreeMap<String, String>, String> {
    let path = root.join("i18n/en.json");
    if !path.is_file() {
        return Ok(alloc::collections::BTreeMap::new());
    }
    let text = std::fs::read_to_string(&path)
        .map_err(|e| alloc::format!("read {}: {e}", path.display()))?;
    parse_flat_json_map(&text).map_err(|e| alloc::format!("{}: {e}", path.display()))
}
