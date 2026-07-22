// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! i18n runtime (TASK-0077): locale catalogs + fallback chain.
//!
//! A [`Catalog`] resolves the program's `i18nKeys` table (key index →
//! template). Templates use `{0}`, `{1}` … placeholders filled from the
//! `@t(key, args…)` argument values. Catalogs chain: lookup falls through
//! missing keys to the next catalog, ending at the **pseudo-locale**
//! (the key text itself) so a missing translation is visible on screen and
//! deterministic in goldens — never a panic, never an empty string.
//!
//! Authoring is JSON per locale; the compiled binary catalog format rides
//! with the CLI `i18n compile` verb (TASK-0078). Host tests and the
//! in-compositor mount build catalogs programmatically from entries.

use crate::store::Value;
use crate::LocaleSource;
use alloc::{
    string::{String, ToString},
    vec::Vec,
};

/// One locale's translations, indexed by the program's key table.
pub struct Catalog {
    /// `templates[key_index]` — `None` = untranslated (falls through).
    templates: Vec<Option<String>>,
}

impl Catalog {
    /// Builds a catalog against the program's key table.
    ///
    /// `program_keys` = the dotted key names in **key-index order** (derive
    /// them from the program: `i18nKeys[i].key` → symbol text).
    /// `entries` = (key name, template) translation pairs; unknown keys are
    /// ignored (a lint reports them at extract time, not at mount).
    #[must_use]
    pub fn from_entries(program_keys: &[&str], entries: &[(&str, &str)]) -> Self {
        let templates = program_keys
            .iter()
            .map(|key| {
                entries.iter().find(|(k, _)| k == key).map(|(_, template)| String::from(*template))
            })
            .collect();
        Self { templates }
    }

    fn lookup(&self, key: u32) -> Option<&str> {
        self.templates.get(key as usize).and_then(|t| t.as_deref())
    }

    /// Loads a compiled binary catalog (`nx-dsl i18n compile` output).
    ///
    /// Format: `NXC1` magic, u32-LE entry count, then per entry
    /// `u32-LE key-len, key bytes, u32-LE value-len, value bytes`, entries
    /// sorted by key (deterministic bytes). `None` = malformed — a broken
    /// catalog fails at load, never silently at lookup.
    #[must_use]
    pub fn from_binary(program_keys: &[&str], bytes: &[u8]) -> Option<Self> {
        let mut cursor = 4usize;
        if bytes.get(..4)? != b"NXC1" {
            return None;
        }
        let take = |cursor: &mut usize, n: usize| -> Option<&[u8]> {
            let slice = bytes.get(*cursor..*cursor + n)?;
            *cursor += n;
            Some(slice)
        };
        let count = u32::from_le_bytes(take(&mut cursor, 4)?.try_into().ok()?) as usize;
        if count > 65536 {
            return None; // bounded
        }
        let mut entries: Vec<(String, String)> = Vec::with_capacity(count);
        for _ in 0..count {
            let key_len = u32::from_le_bytes(take(&mut cursor, 4)?.try_into().ok()?) as usize;
            let key = core::str::from_utf8(take(&mut cursor, key_len)?).ok()?;
            let val_len = u32::from_le_bytes(take(&mut cursor, 4)?.try_into().ok()?) as usize;
            let value = core::str::from_utf8(take(&mut cursor, val_len)?).ok()?;
            entries.push((String::from(key), String::from(value)));
        }
        if cursor != bytes.len() {
            return None;
        }
        let borrowed: Vec<(&str, &str)> =
            entries.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        Some(Self::from_entries(program_keys, &borrowed))
    }

    /// Parses an INDEX-ALIGNED `NXL1` locale pack (RFC-0077: compiled at
    /// bundle build against the program's key order — no name matching at
    /// runtime). Fail-closed: any truncation/oversize/UTF-8 error ⇒ `None`
    /// (callers fall back to the baked default, never render raw bytes).
    #[must_use]
    pub fn from_indexed_pack(bytes: &[u8]) -> Option<Self> {
        const MAX_KEYS: usize = 4096;
        const MAX_TEMPLATE: usize = 4096;
        if bytes.get(..4)? != b"NXL1" {
            return None;
        }
        let count = u32::from_le_bytes(bytes.get(4..8)?.try_into().ok()?) as usize;
        if count > MAX_KEYS {
            return None;
        }
        let mut cursor = 8usize;
        let mut templates: Vec<Option<String>> = Vec::with_capacity(count);
        for _ in 0..count {
            match bytes.get(cursor)? {
                0 => {
                    cursor += 1;
                    templates.push(None);
                }
                1 => {
                    let len =
                        u16::from_le_bytes(bytes.get(cursor + 1..cursor + 3)?.try_into().ok()?)
                            as usize;
                    if len > MAX_TEMPLATE {
                        return None;
                    }
                    let text =
                        core::str::from_utf8(bytes.get(cursor + 3..cursor + 3 + len)?).ok()?;
                    templates.push(Some(String::from(text)));
                    cursor += 3 + len;
                }
                _ => return None,
            }
        }
        if cursor != bytes.len() {
            return None;
        }
        Some(Self { templates })
    }
}

/// The RFC-0077 runtime chain: the ACTIVE catalog over the program's BAKED
/// default text (the terminal — shipped apps never render raw key names).
pub struct CatalogOverBaked<'a> {
    /// The active locale's catalog (`None` = baked default only).
    pub catalog: Option<&'a Catalog>,
    /// Program symbol table (baked display texts live here).
    pub symbols: &'a [String],
    /// i18n key table: key index → symbol id of the baked display text.
    pub keys: &'a [u32],
}

impl LocaleSource for CatalogOverBaked<'_> {
    fn format(&self, key: u32, args: &[Value]) -> String {
        if let Some(template) = self.catalog.and_then(|c| c.lookup(key)) {
            return format_template(template, args);
        }
        self.keys
            .get(key as usize)
            .and_then(|&sym| self.symbols.get(sym as usize))
            .cloned()
            .unwrap_or_default()
    }
}

/// One locale pack inside an `NXLC` payload container (borrowed slices).
pub struct ContainerPack<'a> {
    /// Locale tag (the `i18n/<tag>.json` stem, e.g. `de`).
    pub tag: &'a str,
    /// The raw `NXL1` pack bytes (parse via [`Catalog::from_indexed_pack`]).
    pub pack: &'a [u8],
}

/// Splits an `NXLC` payload container (RFC-0077) into the NXIR slice and its
/// locale packs. `None` = not a container (callers treat the payload as raw
/// NXIR) or malformed (fail-closed — no partial packs).
#[must_use]
pub fn parse_payload_container(bytes: &[u8]) -> Option<(&[u8], Vec<ContainerPack<'_>>)> {
    if bytes.get(..4)? != b"NXLC" || *bytes.get(4)? != 1 {
        return None;
    }
    let nxir_len = u32::from_le_bytes(bytes.get(8..12)?.try_into().ok()?) as usize;
    // 16-byte header keeps the NXIR at an 8-ALIGNED offset (capnp canonical
    // bytes require it); bytes 12..16 are reserved padding (must be zero).
    if bytes.get(12..16)? != [0u8; 4] {
        return None;
    }
    let nxir = bytes.get(16..16 + nxir_len)?;
    let mut cursor = 16 + nxir_len;
    let count = usize::from(*bytes.get(cursor)?);
    cursor += 1;
    let mut packs = Vec::with_capacity(count);
    for _ in 0..count {
        let tag_len = usize::from(*bytes.get(cursor)?);
        if tag_len == 0 || tag_len > 16 {
            return None;
        }
        let tag = core::str::from_utf8(bytes.get(cursor + 1..cursor + 1 + tag_len)?).ok()?;
        cursor += 1 + tag_len;
        let pack_len = u32::from_le_bytes(bytes.get(cursor..cursor + 4)?.try_into().ok()?) as usize;
        let pack = bytes.get(cursor + 4..cursor + 4 + pack_len)?;
        cursor += 4 + pack_len;
        packs.push(ContainerPack { tag, pack });
    }
    // The container tail is zero-padded to a multiple of 8 (payload-length
    // invariant of the bundle path); anything else is a malformed container.
    let pad = bytes.get(cursor..)?;
    if pad.len() >= 8 || pad.iter().any(|&b| b != 0) || bytes.len() % 8 != 0 {
        return None;
    }
    Some((nxir, packs))
}

/// Fills `{n}` placeholders from the argument values.
fn format_template(template: &str, args: &[Value]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut chars = template.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '{' {
            let mut index = 0usize;
            let mut digits = 0usize;
            while let Some(d) = chars.peek().and_then(|c| c.to_digit(10)) {
                index = index * 10 + d as usize;
                digits += 1;
                chars.next();
            }
            if digits > 0 && chars.peek() == Some(&'}') {
                chars.next();
                match args.get(index) {
                    Some(Value::Str(s)) => out.push_str(s),
                    Some(Value::Int(i)) => out.push_str(&i.to_string()),
                    Some(Value::Bool(b)) => out.push_str(if *b { "true" } else { "false" }),
                    Some(Value::Fx(raw)) => out.push_str(&(raw >> 32).to_string()),
                    _ => out.push('?'),
                }
                continue;
            }
            out.push('{');
            // Re-emit any consumed digits verbatim (malformed placeholder).
            if digits > 0 {
                out.push_str(&index.to_string());
            }
        } else {
            out.push(ch);
        }
    }
    out
}

/// A fallback chain of catalogs ending in the pseudo-locale.
///
/// `LocaleChain::new(&[de, en], keys)` — try `de`, then `en`, then render the
/// key text itself (with args appended deterministically for visibility).
pub struct LocaleChain<'a> {
    catalogs: &'a [&'a Catalog],
    /// Key index → key text (the pseudo-locale terminal).
    key_names: &'a [String],
}

impl<'a> LocaleChain<'a> {
    #[must_use]
    pub fn new(catalogs: &'a [&'a Catalog], key_names: &'a [String]) -> Self {
        Self { catalogs, key_names }
    }
}

impl LocaleSource for LocaleChain<'_> {
    fn format(&self, key: u32, args: &[Value]) -> String {
        for catalog in self.catalogs {
            if let Some(template) = catalog.lookup(key) {
                return format_template(template, args);
            }
        }
        // Pseudo-locale: the key itself — visibly untranslated, deterministic.
        self.key_names.get(key as usize).cloned().unwrap_or_default()
    }
}

/// Extracts the program's key names in key-index order (helper for hosts).
///
/// # Panics
/// Never — unreadable entries resolve to empty names.
#[must_use]
pub fn key_names(
    root: nexus_dsl_ir::ui_ir_capnp::ui_program::Reader<'_>,
    symbols: &[String],
) -> Vec<String> {
    let Ok(keys) = root.get_i18n_keys() else { return Vec::new() };
    keys.iter().map(|k| symbols.get(k.get_key() as usize).cloned().unwrap_or_default()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_fill_placeholders_and_chain_falls_through() {
        let keys = ["todo.title", "todo.count"];
        let de = Catalog::from_entries(&keys, &[("todo.count", "{0} Einträge")]);
        let en =
            Catalog::from_entries(&keys, &[("todo.title", "Todos"), ("todo.count", "{0} items")]);
        let names: Vec<String> = keys.iter().map(|k| String::from(*k)).collect();
        let chain_catalogs = [&de, &en];
        let chain = LocaleChain::new(&chain_catalogs, &names);

        // de wins where translated; en fills the gap; args format.
        assert_eq!(chain.format(1, &[Value::Int(3)]), "3 Einträge");
        assert_eq!(chain.format(0, &[]), "Todos");

        // Pseudo-locale terminal: untranslated key stays visible.
        let empty = Catalog::from_entries(&keys, &[]);
        let only_empty = [&empty];
        let pseudo = LocaleChain::new(&only_empty, &names);
        assert_eq!(pseudo.format(0, &[]), "todo.title");
    }

    #[test]
    fn malformed_placeholders_render_verbatim_and_missing_args_are_visible() {
        let keys = ["k"];
        let cat = Catalog::from_entries(&keys, &[("k", "a {x} b {0 c {1}")]);
        let names = alloc::vec![String::from("k")];
        let cats = [&cat];
        let chain = LocaleChain::new(&cats, &names);
        assert_eq!(chain.format(0, &[]), "a {x} b {0 c ?");
    }
}
