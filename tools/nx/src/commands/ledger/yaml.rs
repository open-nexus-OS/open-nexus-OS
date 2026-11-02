// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: The canonical frontmatter subset of RFC-0115: parse and render. The renderer is the
//!          definition of the format; a file is canonical when parse → render reproduces it.
//! OWNERS: @tools-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: tools/nx/tests/ledger_cli.rs (round trips, rejects)
//! ADR: docs/rfcs/RFC-0115-planning-as-data-milestones-sprints-phases-one-ledger-cli.md

/// One value. `Map` and `Records` appear only at the top level; their entries hold
/// `Str`, `Num` or `List` (of `Str`/`Num`) only.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Value {
    Str(String),
    Num(String),
    List(Vec<Value>),
    Map(Vec<(String, Value)>),
    Records(Vec<Vec<(String, Value)>>),
}

pub(crate) type Entries = Vec<(String, Value)>;

impl Value {
    pub(crate) fn str(s: impl Into<String>) -> Self {
        Value::Str(s.into())
    }

    pub(crate) fn as_text(&self) -> Option<&str> {
        match self {
            Value::Str(s) | Value::Num(s) => Some(s),
            _ => None,
        }
    }

    pub(crate) fn items(&self) -> Vec<&str> {
        match self {
            Value::List(items) => items.iter().filter_map(Value::as_text).collect(),
            _ => Vec::new(),
        }
    }

    pub(crate) fn records(&self) -> &[Vec<(String, Value)>] {
        match self {
            Value::Records(r) => r,
            _ => &[],
        }
    }
}

pub(crate) fn get<'a>(entries: &'a [(String, Value)], key: &str) -> Option<&'a Value> {
    entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

pub(crate) fn get_text<'a>(entries: &'a [(String, Value)], key: &str) -> Option<&'a str> {
    get(entries, key).and_then(Value::as_text)
}

/// Set (or with `None` remove) a key; a new key goes where `order` puts it.
pub(crate) fn set(entries: &mut Entries, key: &str, value: Option<Value>, order: &[&str]) {
    let pos = entries.iter().position(|(k, _)| k == key);
    match (pos, value) {
        (Some(i), Some(v)) => entries[i].1 = v,
        (Some(i), None) => {
            entries.remove(i);
        }
        (None, Some(v)) => {
            let rank = |k: &str| order.iter().position(|o| *o == k).unwrap_or(usize::MAX);
            let at = entries.iter().position(|(k, _)| rank(k) > rank(key)).unwrap_or(entries.len());
            entries.insert(at, (key.to_string(), v));
        }
        (None, None) => {}
    }
}

/// A file split into its frontmatter text and its body (everything after the closing fence).
pub(crate) struct Split<'a> {
    pub(crate) front: &'a str,
    pub(crate) body: &'a str,
}

pub(crate) fn split(text: &str) -> Result<Split<'_>, String> {
    let rest = text.strip_prefix("---\n").ok_or("the file does not start with `---`")?;
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if line == "---\n" || line == "---" {
            return Ok(Split { front: &rest[..offset], body: &rest[offset + line.len()..] });
        }
        offset += line.len();
    }
    Err("the frontmatter has no closing `---`".into())
}

pub(crate) fn render_file(entries: &[(String, Value)], body: &str) -> String {
    format!("---\n{}---\n{}", render(entries), body)
}

pub(crate) fn render(entries: &[(String, Value)]) -> String {
    let mut out = String::new();
    for (key, value) in entries {
        match value {
            Value::Map(map) => {
                out.push_str(&format!("{key}:\n"));
                for (k, v) in map {
                    out.push_str(&format!("  {k}: {}\n", inline(v)));
                }
            }
            Value::Records(records) if records.is_empty() => out.push_str(&format!("{key}: []\n")),
            Value::Records(records) => {
                out.push_str(&format!("{key}:\n"));
                for record in records {
                    for (i, (k, v)) in record.iter().enumerate() {
                        let lead = if i == 0 { "  - " } else { "    " };
                        out.push_str(&format!("{lead}{k}: {}\n", inline(v)));
                    }
                }
            }
            other => out.push_str(&format!("{key}: {}\n", inline(other))),
        }
    }
    out
}

fn inline(value: &Value) -> String {
    match value {
        Value::Str(s) => scalar(s),
        Value::Num(n) => n.clone(),
        Value::List(items) => {
            let parts: Vec<String> = items.iter().map(inline).collect();
            format!("[{}]", parts.join(", "))
        }
        Value::Map(_) | Value::Records(_) => String::from("[]"),
    }
}

/// Plain when safe in every YAML reader, else a JSON string (a valid YAML double-quoted scalar).
pub(crate) fn scalar(s: &str) -> String {
    if is_plain_safe(s) && !looks_numeric(s) && !is_reserved(s) {
        s.to_string()
    } else {
        serde_json::to_string(s).unwrap_or_else(|_| String::from("\"\""))
    }
}

fn is_plain_safe(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphanumeric() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '/' | '+' | '-'))
}

pub(crate) fn looks_numeric(s: &str) -> bool {
    let t = s.strip_prefix(['-', '+']).unwrap_or(s);
    if t.starts_with("0x") || t.starts_with("0o") {
        return true;
    }
    let mut seen_digit = false;
    let mut seen_dot = false;
    let mut seen_exp = false;
    let bytes = t.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            b'0'..=b'9' | b'_' => seen_digit |= c != b'_',
            b'.' if !seen_dot && !seen_exp => seen_dot = true,
            b'e' | b'E' if seen_digit && !seen_exp => {
                seen_exp = true;
                if matches!(bytes.get(i + 1), Some(b'-' | b'+')) {
                    i += 1;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    seen_digit
}

fn is_reserved(s: &str) -> bool {
    matches!(
        s.to_ascii_lowercase().as_str(),
        "true" | "false" | "null" | "yes" | "no" | "on" | "off" | "y" | "n"
    )
}

pub(crate) fn parse(front: &str) -> Result<Entries, String> {
    let lines: Vec<&str> = front.lines().collect();
    let mut entries: Entries = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let n = i + 1;
        if line.starts_with(' ') || line.is_empty() {
            return Err(format!("line {n}: expected a top-level `key:`"));
        }
        let (key, rest) =
            split_key(line).ok_or_else(|| format!("line {n}: expected `key: value`"))?;
        if entries.iter().any(|(k, _)| k == key) {
            return Err(format!("line {n}: duplicate key `{key}`"));
        }
        i += 1;
        let value = if rest.is_empty() {
            let start = i;
            while i < lines.len() && lines[i].starts_with("  ") {
                i += 1;
            }
            block(&lines[start..i], start + 1).map_err(|e| format!("`{key}`: {e}"))?
        } else {
            inline_value(rest).map_err(|e| format!("line {n}: {e}"))?
        };
        entries.push((key.to_string(), value));
    }
    Ok(entries)
}

fn split_key(line: &str) -> Option<(&str, &str)> {
    let colon = line.find(':')?;
    let key = &line[..colon];
    if key.is_empty()
        || !key.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        return None;
    }
    let rest = &line[colon + 1..];
    if rest.is_empty() {
        return Some((key, ""));
    }
    rest.strip_prefix(' ').filter(|r| !r.is_empty() && !r.starts_with(' ')).map(|r| (key, r))
}

fn block(lines: &[&str], first: usize) -> Result<Value, String> {
    if lines.is_empty() {
        return Err("an empty block (write `[]` for an empty list)".into());
    }
    if lines[0].starts_with("  - ") {
        let mut records: Vec<Entries> = Vec::new();
        for (j, line) in lines.iter().enumerate() {
            let n = first + j;
            let (fresh, inner) = if let Some(r) = line.strip_prefix("  - ") {
                (true, r)
            } else if let Some(r) = line.strip_prefix("    ") {
                (false, r)
            } else {
                return Err(format!("line {n}: expected `  - key:` or `    key:`"));
            };
            let (k, v) =
                split_key(inner).ok_or_else(|| format!("line {n}: expected `key: value`"))?;
            if v.is_empty() {
                return Err(format!("line {n}: nested blocks are not part of the format"));
            }
            let value = inline_value(v).map_err(|e| format!("line {n}: {e}"))?;
            if fresh {
                records.push(Vec::new());
            }
            let record = records.last_mut().ok_or_else(|| format!("line {n}: no record"))?;
            if record.iter().any(|(rk, _)| rk == k) {
                return Err(format!("line {n}: duplicate key `{k}`"));
            }
            record.push((k.to_string(), value));
        }
        return Ok(Value::Records(records));
    }
    let mut map: Entries = Vec::new();
    for (j, line) in lines.iter().enumerate() {
        let n = first + j;
        let inner = line.strip_prefix("  ").filter(|r| !r.starts_with(' '));
        let inner = inner.ok_or_else(|| format!("line {n}: expected `  key: value`"))?;
        let (k, v) = split_key(inner).ok_or_else(|| format!("line {n}: expected `key: value`"))?;
        if v.is_empty() {
            return Err(format!("line {n}: nested blocks are not part of the format"));
        }
        if map.iter().any(|(mk, _)| mk == k) {
            return Err(format!("line {n}: duplicate key `{k}`"));
        }
        map.push((k.to_string(), inline_value(v).map_err(|e| format!("line {n}: {e}"))?));
    }
    Ok(Value::Map(map))
}

fn inline_value(text: &str) -> Result<Value, String> {
    if let Some(inner) = text.strip_prefix('[') {
        let inner = inner.strip_suffix(']').ok_or("a flow list must end with `]`")?;
        let mut items = Vec::new();
        let mut rest = inner;
        while !rest.is_empty() {
            let (item, after) = take_scalar(rest)?;
            items.push(item);
            rest = match after.strip_prefix(", ") {
                Some(r) if !r.is_empty() => r,
                Some(_) => return Err("a trailing `,` in a list".into()),
                None if after.is_empty() => after,
                None => return Err("list items are separated by `, `".into()),
            };
        }
        return Ok(Value::List(items));
    }
    let (value, after) = take_scalar(text)?;
    if !after.is_empty() {
        return Err(format!("unexpected text after the value: `{after}`"));
    }
    Ok(value)
}

/// One scalar from the front of `text`: a JSON-quoted string or a plain token.
fn take_scalar(text: &str) -> Result<(Value, &str), String> {
    if text.starts_with('"') {
        let bytes = text.as_bytes();
        let mut i = 1;
        while i < bytes.len() {
            match bytes[i] {
                b'\\' => i += 2,
                b'"' => {
                    let s: String = serde_json::from_str(&text[..=i])
                        .map_err(|e| format!("a bad quoted string: {e}"))?;
                    return Ok((Value::Str(s), &text[i + 1..]));
                }
                _ => i += 1,
            }
        }
        return Err("an unterminated quoted string".into());
    }
    let end = text.find(", ").unwrap_or(text.len());
    let token = &text[..end];
    if !is_plain_safe(token) {
        return Err(format!("`{token}` must be quoted"));
    }
    if is_reserved(token) {
        return Err(format!("`{token}` is a reserved word; quote it"));
    }
    let value = if looks_numeric(token) {
        Value::Num(token.to_string())
    } else {
        Value::Str(token.to_string())
    };
    Ok((value, &text[end..]))
}
