// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Generated blocks in docs (TASK-0349): a table a doc would otherwise copy by hand is
//!          rendered from its source between `<!-- gen:NAME -->` and `<!-- /gen:NAME -->`. The gate
//!          compares, `nx ledger lint --fix` renders. Sources: the kernel's syscall numbers, the
//!          policy's capability grants, the proof manifest's phases and markers, and the docs'
//!          own frontmatter (the index).
//! OWNERS: @tools-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: tools/nx/tests/ledger_docs.rs
//! ADR: docs/rfcs/RFC-0115-planning-as-data-milestones-sprints-phases-one-ledger-cli.md

use super::docs::relative;
use super::model::Kind;
use super::repo::{Doc, Repo};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

pub(crate) const SYSCALLS: &str = "source/kernel/neuron/src/syscall/mod.rs";
pub(crate) const POLICY: &str = "policies/base.toml";
pub(crate) const MANIFEST: &str = "source/apps/selftest-client/proof-manifest";

const NAMES: &[&str] = &["syscalls", "capabilities", "markers", "docs-index"];

/// One block: its name and the byte range of its contents in the body.
struct Block<'a> {
    name: &'a str,
    inner: std::ops::Range<usize>,
}

fn open_name(line: &str) -> Option<&str> {
    line.trim().strip_prefix("<!-- gen:")?.strip_suffix(" -->")
}

fn blocks(body: &str) -> Result<Vec<Block<'_>>, String> {
    let mut out = Vec::new();
    let mut open: Option<(&str, usize)> = None;
    let mut offset = 0;
    for line in body.split_inclusive('\n') {
        let start = offset;
        offset += line.len();
        if let Some(name) = open_name(line) {
            if open.is_some() {
                return Err(format!("generated block `{name}` opens inside another block"));
            }
            if !NAMES.contains(&name) {
                return Err(format!(
                    "unknown generated block `{name}` (known: {})",
                    NAMES.join(", ")
                ));
            }
            open = Some((name, offset));
        } else if let Some(name) =
            line.trim().strip_prefix("<!-- /gen:").and_then(|l| l.strip_suffix(" -->"))
        {
            match open.take() {
                Some((n, from)) if n == name => out.push(Block { name, inner: from..start }),
                _ => return Err(format!("`<!-- /gen:{name} -->` closes no open block")),
            }
        }
    }
    match open {
        Some((name, _)) => Err(format!("generated block `{name}` is never closed")),
        None => Ok(out),
    }
}

/// The body without the contents of its generated blocks (what the word budget counts).
pub(crate) fn strip(body: &str) -> Result<String, String> {
    let mut out = String::new();
    let mut at = 0;
    for b in blocks(body)? {
        out.push_str(&body[at..b.inner.start]);
        at = b.inner.end;
    }
    out.push_str(&body[at..]);
    Ok(out)
}

/// The body with every generated block rendered from its source.
pub(crate) fn render(repo: &Repo, doc: &Doc) -> Result<String, String> {
    let mut out = String::new();
    let mut at = 0;
    for b in blocks(&doc.body)? {
        out.push_str(&doc.body[at..b.inner.start]);
        out.push_str(&generate(repo, doc, b.name)?);
        at = b.inner.end;
    }
    out.push_str(&doc.body[at..]);
    Ok(out)
}

fn generate(repo: &Repo, doc: &Doc, name: &str) -> Result<String, String> {
    match name {
        "syscalls" => syscalls(&repo.root),
        "capabilities" => capabilities(&repo.root),
        "markers" => markers(&repo.root),
        _ => Ok(index(repo, doc)),
    }
}

fn cell(s: &str) -> String {
    s.replace('|', "\\|")
}

/// The first sentence of a doc comment, at most ~110 characters.
fn first_sentence(text: &str) -> String {
    let end = text.find(". ").map_or(text.len(), |i| i + 1);
    let s = text[..end].trim();
    if s.chars().count() <= 110 {
        return s.to_string();
    }
    let cut: String = s.chars().take(108).collect();
    format!("{}…", cut.trim_end())
}

/// `| # | syscall | what |` from the kernel's `pub const SYSCALL_*: usize = N;` and the
/// `///` comment above each.
pub(crate) fn syscalls(root: &Path) -> Result<String, String> {
    let text = fs::read_to_string(root.join(SYSCALLS)).map_err(|e| format!("{SYSCALLS}: {e}"))?;
    let mut doc = String::new();
    let mut rows: BTreeMap<u32, (String, String)> = BTreeMap::new();
    for line in text.lines() {
        let t = line.trim();
        if let Some(c) = t.strip_prefix("///") {
            doc.push(' ');
            doc.push_str(c.trim());
            continue;
        }
        if let Some(rest) = t.strip_prefix("pub const SYSCALL_") {
            if let Some((name, value)) = rest.split_once(": usize =") {
                let n: u32 = value
                    .trim()
                    .trim_end_matches(';')
                    .trim()
                    .parse()
                    .map_err(|_| format!("{SYSCALLS}: SYSCALL_{name} has no plain number"))?;
                if rows.insert(n, (name.to_lowercase(), first_sentence(doc.trim()))).is_some() {
                    return Err(format!("{SYSCALLS}: number {n} is defined twice"));
                }
            }
        }
        doc.clear();
    }
    let mut out = String::from("| # | syscall | what |\n|---|---|---|\n");
    for (n, (name, what)) in rows {
        out.push_str(&format!("| {n} | `{name}` | {} |\n", cell(&what)));
    }
    Ok(out)
}

/// `| capability | held by |` from the `[allow]` table of the base policy.
pub(crate) fn capabilities(root: &Path) -> Result<String, String> {
    let text = fs::read_to_string(root.join(POLICY)).map_err(|e| format!("{POLICY}: {e}"))?;
    let value: toml::Value = text.parse().map_err(|e| format!("{POLICY}: {e}"))?;
    let allow = value
        .get("allow")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| format!("{POLICY}: no [allow] table"))?;
    let mut caps: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (service, list) in allow {
        for cap in list.as_array().into_iter().flatten().filter_map(toml::Value::as_str) {
            caps.entry(cap).or_default().push(service);
        }
    }
    let mut out = String::from("| capability | held by |\n|---|---|\n");
    for (cap, mut holders) in caps {
        holders.sort_unstable();
        out.push_str(&format!("| `{cap}` | {} |\n", holders.join(", ")));
    }
    Ok(out)
}

/// `| # | phase | markers | declared in |` from the proof manifest.
pub(crate) fn markers(root: &Path) -> Result<String, String> {
    let dir = root.join(MANIFEST);
    let phases_text = fs::read_to_string(dir.join("phases.toml"))
        .map_err(|e| format!("{MANIFEST}/phases.toml: {e}"))?;
    let phases: toml::Value =
        phases_text.parse().map_err(|e| format!("{MANIFEST}/phases.toml: {e}"))?;
    let mut order: BTreeMap<i64, String> = BTreeMap::new();
    for (name, p) in phases.get("phase").and_then(toml::Value::as_table).into_iter().flatten() {
        let n = p.get("order").and_then(toml::Value::as_integer).unwrap_or(0);
        order.insert(n, name.clone());
    }
    let mut files: Vec<String> = fs::read_dir(dir.join("markers"))
        .map_err(|e| format!("{MANIFEST}/markers: {e}"))?
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.ends_with(".toml"))
        .collect();
    files.sort();
    let mut count: BTreeMap<String, (usize, Vec<String>)> = BTreeMap::new();
    let mut total = 0;
    for f in &files {
        let text =
            fs::read_to_string(dir.join("markers").join(f)).map_err(|e| format!("{f}: {e}"))?;
        let v: toml::Value = text.parse().map_err(|e| format!("markers/{f}: {e}"))?;
        for (_, m) in v.get("marker").and_then(toml::Value::as_table).into_iter().flatten() {
            let phase = m.get("phase").and_then(toml::Value::as_str).unwrap_or("?").to_string();
            let entry = count.entry(phase).or_default();
            entry.0 += 1;
            if !entry.1.contains(f) {
                entry.1.push(f.clone());
            }
            total += 1;
        }
    }
    let mut out = String::from("| # | phase | markers | declared in |\n|---|---|---|---|\n");
    for (n, name) in &order {
        let (c, fs) = count.remove(name).unwrap_or_default();
        out.push_str(&format!("| {n} | `{name}` | {c} | {} |\n", fs.join(", ")));
    }
    for (name, (c, fs)) in count {
        out.push_str(&format!("| – | `{name}` (no phase entry) | {c} | {} |\n", fs.join(", ")));
    }
    out.push_str(&format!("\n{total} markers in {} files.\n", files.len()));
    Ok(out)
}

/// `| page | role | what |` for every doc, grouped by folder, linked from the doc holding it.
fn index(repo: &Repo, holder: &Doc) -> String {
    let from = holder.rel.rsplit_once('/').map_or("", |(d, _)| d);
    let mut docs: Vec<&Doc> = repo.of(Kind::Doc).filter(|d| d.rel != holder.rel).collect();
    docs.sort_by(|a, b| a.rel.cmp(&b.rel));
    let mut out = String::new();
    let mut folder = "";
    for d in docs {
        let inner = d.rel.strip_prefix("docs/").unwrap_or(&d.rel);
        let f = inner.rsplit_once('/').map_or("", |(dir, _)| dir);
        if f != folder || out.is_empty() {
            folder = f;
            let label = if f.is_empty() { "docs" } else { f };
            out.push_str(&format!("\n**{label}/**\n\n| page | role | what |\n|---|---|---|\n"));
        }
        let name = inner.rsplit('/').next().unwrap_or(inner);
        out.push_str(&format!(
            "| [{name}]({}) | {} | {} |\n",
            relative(from, &d.rel),
            d.text("role").unwrap_or("?"),
            cell(d.text("title").unwrap_or("")),
        ));
    }
    out.trim_start().to_string()
}
