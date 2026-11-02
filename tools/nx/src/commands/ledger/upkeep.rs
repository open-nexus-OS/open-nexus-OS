// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Keeping docs true (TASK-0349). `stale`: the docs whose `sources` changed in a commit
//!          after their `verified` day — the docs sweep's worklist, counted in the brief. `relink`:
//!          a doc moved or merged; every planning document and every markdown file under `docs/`
//!          is rewritten to the new path (relative links recomputed), `-` drops `links` records
//!          that pointed at a deleted doc, and whatever else still names the old path is listed.
//! OWNERS: @tools-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: tools/nx/tests/ledger_docs.rs
//! ADR: docs/rfcs/RFC-0115-planning-as-data-milestones-sprints-phases-one-ledger-cli.md

use super::docs::{code_files, join, relative, resolve, scan, Ref};
use super::model::{is_repo_path, Kind};
use super::repo::{self, Doc, Repo};
use super::yaml::{get, Value};
use std::fs;
use std::path::Path;
use std::process::Command;

/// One doc whose sources moved on after it was verified.
pub(crate) struct Stale {
    pub(crate) rel: String,
    pub(crate) verified: String,
    /// `<date> <hash> <subject>` of each later commit on its sources, newest first.
    pub(crate) commits: Vec<String>,
}

/// Commits after `since` (a date), each with the files it touched.
fn history(
    root: &Path,
    since: &str,
    paths: &[&str],
) -> Result<Vec<(String, String, Vec<String>)>, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "log",
            &format!("--after={since} 23:59:59"),
            "--format=@%cs %h %s",
            "--name-only",
            "--",
        ])
        .args(paths)
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        return Err(format!("git log: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    let mut commits: Vec<(String, String, Vec<String>)> = Vec::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        if let Some(head) = line.strip_prefix('@') {
            let (date, rest) = head.split_once(' ').unwrap_or((head, ""));
            commits.push((date.to_string(), rest.to_string(), Vec::new()));
        } else if !line.is_empty() {
            if let Some(c) = commits.last_mut() {
                c.2.push(line.to_string());
            }
        }
    }
    Ok(commits)
}

pub(crate) fn stale(repo: &Repo) -> Result<Vec<Stale>, String> {
    let docs: Vec<(&Doc, &str, Vec<&str>)> = repo
        .of(Kind::Doc)
        .filter_map(|d| {
            let v = d.text("verified")?;
            let s = get(&d.entries, "sources").map(Value::items).unwrap_or_default();
            (!s.is_empty()).then_some((d, v, s))
        })
        .collect();
    let Some(since) = docs.iter().map(|(_, v, _)| *v).min() else { return Ok(Vec::new()) };
    let mut paths: Vec<&str> = docs.iter().flat_map(|(_, _, s)| s.iter().copied()).collect();
    paths.sort_unstable();
    paths.dedup();
    let commits = history(&repo.root, since, &paths)?;
    let under = |file: &str, src: &str| file == src || file.starts_with(&format!("{src}/"));
    let mut out = Vec::new();
    for (doc, verified, sources) in docs {
        let hits: Vec<String> = commits
            .iter()
            .filter(|(date, _, files)| {
                date.as_str() > verified
                    && files.iter().any(|f| sources.iter().any(|s| under(f, s)))
            })
            .map(|(date, rest, _)| format!("{date} {rest}"))
            .collect();
        if !hits.is_empty() {
            out.push(Stale { rel: doc.rel.clone(), verified: verified.to_string(), commits: hits });
        }
    }
    Ok(out)
}

pub(crate) fn stale_report(repo: &Repo) -> Result<String, String> {
    let list = stale(repo)?;
    let mut text = String::new();
    for s in &list {
        text.push_str(&format!(
            "{} · verified {} · {} commit(s) on its sources since\n",
            s.rel,
            s.verified,
            s.commits.len()
        ));
        for c in s.commits.iter().take(3) {
            text.push_str(&format!("    {c}\n"));
        }
    }
    text.push_str(&format!(
        "stale: {} doc(s); read the commits, fix the doc, then `nx ledger set <doc> verified <today>`",
        list.len()
    ));
    Ok(text)
}

/// Rewrite every relative link in `body` (of a file in `dir`) that resolves to `old`.
fn rewrite_links(body: &str, dir_rel: &str, old: &str, new: &str) -> String {
    let file = format!("{dir_rel}/x.md");
    let mut out = body.to_string();
    for (_, r) in scan(body) {
        let Ref::Link(t) = r else { continue };
        if resolve(&file, &Ref::Link(t), false).as_deref() == Some(old) {
            let anchor = t.split_once('#').map(|(_, a)| format!("#{a}")).unwrap_or_default();
            let target = t.split_whitespace().next().unwrap_or(t);
            out = out.replace(
                &format!("]({target})"),
                &format!("]({}{anchor})", relative(dir_rel, new)),
            );
        }
    }
    out
}

/// The links of a moved file, rebased from the directory it was read in to the one it is in.
fn rebase_links(body: &str, from_dir: &str, to_dir: &str, root: &Path) -> String {
    let mut out = body.to_string();
    for (_, r) in scan(body) {
        let Ref::Link(t) = r else { continue };
        let target = t.split_whitespace().next().unwrap_or(t);
        let (path, anchor) =
            target.split_once('#').map_or((target, String::new()), |(p, a)| (p, format!("#{a}")));
        if path.is_empty() || path.contains("://") || root.join(to_dir).join(path).exists() {
            continue;
        }
        if let Some(abs) = join(from_dir, path).filter(|a| root.join(a).exists()) {
            out = out.replace(
                &format!("]({target})"),
                &format!("]({}{anchor})", relative(to_dir, &abs)),
            );
        }
    }
    out
}

fn retarget(v: &mut Value, old: &str, new: &str) -> bool {
    match v {
        Value::Str(s) if s.contains(old) && new != "-" => {
            *s = s.replace(old, new);
            true
        }
        Value::List(items) => items.iter_mut().fold(false, |c, i| retarget(i, old, new) | c),
        Value::Map(m) => m.iter_mut().fold(false, |c, (_, v)| retarget(v, old, new) | c),
        Value::Records(rs) => {
            let before = rs.len();
            if new == "-" {
                rs.retain(|r| get(r, "target").and_then(Value::as_text) != Some(old));
            }
            let changed =
                rs.iter_mut().flatten().fold(false, |c, (_, v)| retarget(v, old, new) | c);
            changed || rs.len() != before
        }
        _ => false,
    }
}

/// Every markdown file under `docs/` the ledger does not load (ADRs, dev guides, the board).
fn foreign_markdown(root: &Path, rel: &str, out: &mut Vec<String>) {
    let Ok(read) = fs::read_dir(root.join(rel)) else { return };
    for e in read.filter_map(|e| e.ok()) {
        let Ok(name) = e.file_name().into_string() else { continue };
        let child = format!("{rel}/{name}");
        if e.file_type().is_ok_and(|t| t.is_dir()) {
            foreign_markdown(root, &child, out);
        } else if name.ends_with(".md") {
            out.push(child);
        }
    }
}

pub(crate) fn relink(repo: &mut Repo, old: &str, new: &str) -> Result<String, String> {
    if !(is_repo_path(old) && old.starts_with("docs/")) {
        return Err(format!("`{old}` is not a repo-relative path under docs/"));
    }
    if new != "-" && !is_repo_path(new) {
        return Err(format!("`{new}` is not a repo-relative path"));
    }
    if repo.root.join(old).exists() {
        return Err(format!("{old} still exists: move or merge it first, then relink"));
    }
    if new != "-" && !repo.root.join(new).exists() {
        return Err(format!("{new} does not exist"));
    }
    let old_dir = old.rsplit_once('/').map_or("", |(d, _)| d).to_string();
    let mut written = Vec::new();
    for doc in &mut repo.docs {
        let dir = doc.rel.rsplit_once('/').map_or("", |(d, _)| d).to_string();
        // A document never links itself: where the old doc went into this one, its records drop.
        let to = if doc.rel == new { "-" } else { new };
        let mut changed = doc.entries.iter_mut().fold(false, |c, (_, v)| retarget(v, old, to) | c);
        let mut body = doc.body.clone();
        if to != "-" {
            body = body.replace(old, new);
            body = rewrite_links(&body, &dir, old, new);
        } else if doc.rel == new {
            body = rebase_links(&body, &old_dir, &dir, &repo.root);
        }
        if body != doc.body {
            doc.body = body;
            changed = true;
        }
        if changed {
            repo::write(&repo.root, doc)?;
            written.push(doc.rel.clone());
        }
    }
    let mut foreign = Vec::new();
    for tree in ["docs/adr", "docs/dev", "docs/board"] {
        foreign_markdown(&repo.root, tree, &mut foreign);
    }
    // Docs that do not parse yet (no frontmatter) are still markdown whose links must follow.
    foreign.extend(repo.errors.iter().map(|e| e.rel.clone()).filter(|r| r.ends_with(".md")));
    if new != "-" {
        for rel in &foreign {
            let Ok(text) = fs::read_to_string(repo.root.join(rel)) else { continue };
            let dir = rel.rsplit_once('/').map_or("", |(d, _)| d);
            let next = if rel == new {
                rebase_links(&text, &old_dir, dir, &repo.root)
            } else {
                rewrite_links(&text.replace(old, new), dir, old, new)
            };
            if next != text {
                repo::write_text(&repo.root, rel, &next)?;
                written.push(rel.clone());
            }
        }
    }
    let mut left = Vec::new();
    let others =
        code_files(&repo.root).into_iter().chain(if new == "-" { foreign } else { Vec::new() });
    for rel in others.chain(repo.docs.iter().map(|d| d.rel.clone())) {
        let Ok(text) = fs::read_to_string(repo.root.join(&rel)) else { continue };
        if text.contains(old) && !left.contains(&rel) {
            left.push(rel);
        }
    }
    let mut out = format!("relink {old} → {new}: {} file(s) rewritten\n", written.len());
    for rel in &left {
        out.push_str(&format!("  still names it (edit by hand): {rel}\n"));
    }
    Ok(out.trim_end().to_string())
}
