// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: The doc rules of TASK-0349: a doc names the code it describes (`sources`) and the
//!          contracts it follows, every path and link in it resolves, it carries no task status
//!          (status lives in the ledger), and it stays inside the word budget of its role. Also the
//!          repo-wide scan: a `docs/…md` named from code comments, root files or `.claude/` exists;
//!          one named from a planning document is a note (fatal for new-process tasks).
//! OWNERS: @tools-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: tools/nx/tests/ledger_docs.rs (a `test_reject_*` per rule)
//! ADR: docs/rfcs/RFC-0115-planning-as-data-milestones-sprints-phases-one-ledger-cli.md

use super::gen;
use super::model::{is_adr_id, is_repo_path, Kind, CUTOVER};
use super::repo::{Doc, Repo};
use super::rules::Problem;
use super::yaml::{get, Value};
use std::fs;
use std::path::Path;

/// The most words a doc of a role may hold outside its generated blocks.
pub(crate) fn budget(role: &str) -> usize {
    match role {
        "index" => 800,
        "guide" => 1200,
        "reference" => 2000,
        _ => 4000,
    }
}

/// Status words that rot in prose; the ledger holds status (`nx ledger show <TASK>`).
const STATUS_WORDS: &[&str] = &[
    "✅",
    "⬜",
    "In Review",
    "as of 20",
    "As of 20",
    "Last Updated",
    "Last updated:",
    "Current status",
    "Current Status",
];

/// The top-level trees a backticked path may start with to be checked.
const TOPS: &[&str] = &[
    "source/",
    "userspace/",
    "tools/",
    "scripts/",
    "docs/",
    "tasks/",
    "recipes/",
    "policies/",
    "config/",
    "schemas/",
    "tests/",
    "resources/",
    "podman/",
    ".claude/",
    ".github/",
];

/// Rules a doc can be judged by alone (called from `rules::check_doc`).
pub(crate) fn alone(doc: &Doc, out: &mut Vec<String>) {
    let role = doc.text("role").unwrap_or("spec");
    let sources = get(&doc.entries, "sources").map(Value::items).unwrap_or_default();
    if role != "index" && sources.is_empty() {
        out.push(format!("a {role} names the code it describes in `sources`"));
    }
    let prose = match gen::strip(&doc.body) {
        Ok(p) => p,
        Err(e) => {
            out.push(e);
            return;
        }
    };
    let words = prose.split_whitespace().count();
    if words > budget(role) {
        out.push(format!(
            "{words} words, over the {role} budget of {}: cut it or split it",
            budget(role)
        ));
    }
    for (n, line) in prose_lines(&prose) {
        if let Some(w) = STATUS_WORDS.iter().find(|w| line.contains(**w)) {
            out.push(format!(
                "line {n}: `{w}` is task status; status lives in the ledger, not in a doc"
            ));
        }
    }
}

/// Rules that need the repository (called from `rules::check_refs`).
pub(crate) fn refs(repo: &Repo, doc: &Doc, fatal: &mut Vec<String>) {
    for src in get(&doc.entries, "sources").map(Value::items).unwrap_or_default() {
        if is_repo_path(src) && !repo.root.join(src).exists() {
            fatal.push(format!("`sources` names {src}, which does not exist"));
        }
    }
    for id in get(&doc.entries, "contracts").map(Value::items).unwrap_or_default() {
        let known = if is_adr_id(id) {
            repo.adrs.contains(id)
        } else {
            repo.get(id).is_some_and(|d| d.kind == Kind::Rfc)
        };
        if !known {
            fatal.push(format!("`contracts` names {id}, which has no file"));
        }
    }
    for (n, target) in dead_refs(&repo.root, &doc.rel, &doc.body, false) {
        fatal.push(format!("line {n}: {target} does not exist"));
    }
    match gen::render(repo, doc) {
        Ok(body) if body != doc.body => fatal
            .push(String::from("a generated block is stale; `nx ledger lint --fix` renders it")),
        Ok(_) => {}
        Err(e) => fatal.push(e),
    }
}

/// `docs/…` paths a planning document names that do not exist: fatal for a task started under
/// the new process, a note for the grandfathered ones.
pub(crate) fn planning_refs(
    repo: &Repo,
    doc: &Doc,
    fatal: &mut Vec<String>,
    notes: &mut Vec<String>,
) {
    let new = doc.kind == Kind::Task && doc.text("started").is_some_and(|s| s >= CUTOVER);
    let mut text = doc.body.clone();
    for (_, v) in &doc.entries {
        collect_text(v, &mut text);
    }
    for (_, target) in dead_refs(&repo.root, &doc.rel, &text, true) {
        let msg = format!("names {target}, which does not exist");
        if new {
            fatal.push(msg);
        } else {
            notes.push(msg);
        }
    }
}

fn collect_text(v: &Value, out: &mut String) {
    match v {
        Value::Str(s) => {
            out.push('\n');
            out.push_str(&format!("`{s}`"));
        }
        Value::Num(_) => {}
        Value::List(items) => items.iter().for_each(|i| collect_text(i, out)),
        Value::Map(m) => m.iter().for_each(|(_, v)| collect_text(v, out)),
        Value::Records(rs) => rs.iter().flatten().for_each(|(_, v)| collect_text(v, out)),
    }
}

/// Lines outside fenced code, 1-based.
pub(crate) fn prose_lines(text: &str) -> Vec<(usize, &str)> {
    let mut fenced = false;
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if !fenced {
            out.push((i + 1, line));
        }
    }
    out
}

/// A reference found in markdown: a link target or a backticked path.
pub(crate) enum Ref<'a> {
    Link(&'a str),
    Tick(&'a str),
}

/// Every link target and backticked span in the prose of `text`, with its line.
pub(crate) fn scan(text: &str) -> Vec<(usize, Ref<'_>)> {
    let mut out = Vec::new();
    for (n, line) in prose_lines(text) {
        let mut rest = line;
        while let Some(i) = rest.find("](") {
            let after = &rest[i + 2..];
            let end = after.find(')').unwrap_or(after.len());
            out.push((n, Ref::Link(&after[..end])));
            rest = &after[end..];
        }
        let mut parts = line.split('`');
        parts.next();
        while let (Some(inner), Some(_)) = (parts.next(), parts.next()) {
            out.push((n, Ref::Tick(inner)));
        }
    }
    out
}

/// The repo-relative path a reference means, when it is one this gate checks.
/// `docs_only` keeps only targets below `docs/` (planning documents name planned code freely).
pub(crate) fn resolve(from_rel: &str, r: &Ref<'_>, docs_only: bool) -> Option<String> {
    let dir = from_rel.rsplit_once('/').map_or("", |(d, _)| d);
    let target = match r {
        Ref::Link(t) => {
            let t = t.split_whitespace().next().unwrap_or("");
            if t.is_empty() || t.starts_with('#') || t.contains("://") || t.starts_with("mailto:") {
                return None;
            }
            join(dir, t.split('#').next().unwrap_or(""))?
        }
        Ref::Tick(t) => {
            let t = t.trim().split([':', '#']).next().unwrap_or("").trim_end_matches(['.', ',']);
            if TOPS.iter().any(|top| t.starts_with(top)) {
                t.trim_end_matches('/').to_string()
            } else if t.ends_with(".md") && !t.contains('/') && from_rel.starts_with("docs/") {
                join(dir, t)?
            } else {
                return None;
            }
        }
    };
    if !is_repo_path(&target) || (docs_only && !target.starts_with("docs/")) {
        return None;
    }
    if docs_only && !target.ends_with(".md") {
        return None;
    }
    Some(target)
}

/// `dir` + a relative path, `..` folded; `None` when it climbs out of the repo.
pub(crate) fn join(dir: &str, rel: &str) -> Option<String> {
    if rel.is_empty() || rel.starts_with('/') {
        return None;
    }
    let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
    for p in rel.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            p => parts.push(p),
        }
    }
    Some(parts.join("/"))
}

/// The path from directory `from_dir` to the repo-relative `to`.
pub(crate) fn relative(from_dir: &str, to: &str) -> String {
    let a: Vec<&str> = from_dir.split('/').filter(|p| !p.is_empty()).collect();
    let b: Vec<&str> = to.split('/').filter(|p| !p.is_empty()).collect();
    let common = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let mut parts: Vec<&str> = vec![".."; a.len() - common];
    parts.extend(&b[common..]);
    parts.join("/")
}

fn dead_refs(root: &Path, rel: &str, text: &str, docs_only: bool) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (n, r) in scan(text) {
        if let Some(target) = resolve(rel, &r, docs_only) {
            if !root.join(&target).exists() && !out.iter().any(|(_, t)| *t == target) {
                out.push((n, target));
            }
        }
    }
    out
}

/// Trees and root files whose comments may name a doc (vendored and build trees skipped).
const CODE_TREES: &[&str] =
    &["source", "userspace", "tools", "scripts", "recipes", "policies", ".claude", ".github"];
const ROOT_FILES: &[&str] =
    &["CLAUDE.md", "AGENTS.md", "README.md", "CONTRIBUTING.md", "SECURITY.md", "justfile"];
const SKIP_DIRS: &[&str] = &["target", "qemu-src", "node_modules", ".git", "build"];
const CODE_EXT: &[&str] = &["rs", "sh", "toml", "py", "md", "yml", "yaml"];

/// Every file outside `docs/` and `tasks/` that may name a doc, repo-relative.
pub(crate) fn code_files(root: &Path) -> Vec<String> {
    let mut out: Vec<String> =
        ROOT_FILES.iter().filter(|f| root.join(f).is_file()).map(|f| f.to_string()).collect();
    for tree in CODE_TREES {
        walk(root, tree, &mut out);
    }
    out.sort();
    out
}

fn walk(root: &Path, rel: &str, out: &mut Vec<String>) {
    let Ok(read) = fs::read_dir(root.join(rel)) else { return };
    for entry in read.filter_map(|e| e.ok()) {
        let Ok(name) = entry.file_name().into_string() else { continue };
        let child = format!("{rel}/{name}");
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            if !SKIP_DIRS.contains(&name.as_str()) {
                walk(root, &child, out);
            }
        } else if name.rsplit_once('.').is_some_and(|(_, ext)| CODE_EXT.contains(&ext)) {
            out.push(child);
        }
    }
}

/// The lines of a file that may name a doc: every line of markdown, comment lines elsewhere
/// (a string literal that builds a path is code, not a reference).
pub(crate) fn reference_lines(rel: &str, text: &str) -> Vec<(usize, String)> {
    let md = rel.ends_with(".md");
    text.lines()
        .enumerate()
        .filter(|(_, l)| {
            let t = l.trim_start();
            md || t.starts_with("//") || t.starts_with('#') || t.starts_with("* ")
        })
        .map(|(i, l)| (i + 1, l.to_string()))
        .collect()
}

/// `docs/…md` tokens in a line.
pub(crate) fn doc_tokens(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(i) = rest.find("docs/") {
        let tail = &rest[i..];
        let end = tail
            .find(|c: char| !(c.is_ascii_alphanumeric() || "_./-".contains(c)))
            .unwrap_or(tail.len());
        let token = tail[..end].trim_end_matches('.');
        let boundary = i == 0 || !rest.as_bytes()[i - 1].is_ascii_alphanumeric();
        if boundary && token.ends_with(".md") {
            out.push(token);
        }
        rest = &tail[end.max(1)..];
    }
    out
}

/// Dead doc references from code comments, root files and `.claude/` — fatal.
pub(crate) fn code_refs(root: &Path) -> Vec<Problem> {
    let mut out = Vec::new();
    for rel in code_files(root) {
        let Ok(text) = fs::read_to_string(root.join(&rel)) else { continue };
        if !text.contains("docs/") {
            continue;
        }
        for (n, line) in reference_lines(&rel, &text) {
            for token in doc_tokens(&line) {
                if !root.join(token).exists() {
                    out.push(Problem {
                        rel: format!("{rel}:{n}"),
                        message: format!("names {token}, which does not exist"),
                        fatal: true,
                    });
                }
            }
        }
    }
    out
}
