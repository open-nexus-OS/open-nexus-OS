// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: The read side of `nx ledger`: `brief` (session start), `show`, `list` and `table`
//!          (the counted progress table). Compact by design — a few hundred tokens, not a ledger.
//! OWNERS: @tools-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: tools/nx/tests/ledger_cli.rs
//! ADR: docs/rfcs/RFC-0115-planning-as-data-milestones-sprints-phases-one-ledger-cli.md

use super::model::Kind;
use super::repo::{Doc, Repo};
use super::rules::phase_of;
use super::yaml::{get, get_text, Value};
use std::fmt::Write;

fn mark(state: &str) -> &'static str {
    match state {
        "done" => "✓",
        "active" | "building" => "▶",
        "proving" => "◆",
        "blocked" => "!",
        "dropped" => "✗",
        _ => "·",
    }
}

fn records<'a>(doc: &'a Doc, key: &str) -> &'a [Vec<(String, Value)>] {
    get(&doc.entries, key).map(Value::records).unwrap_or(&[])
}

pub(crate) fn show(repo: &Repo, ids: &[String]) -> Result<String, String> {
    let mut out = String::new();
    for id in ids {
        let doc = repo.get(id).ok_or_else(|| format!("{id}: no such document"))?;
        match doc.kind {
            Kind::Task => show_task(doc, &mut out),
            Kind::Milestone => show_milestone(repo, doc, &mut out),
            Kind::Sprint => show_sprint(repo, doc, &mut out),
            _ => show_plain(doc, &mut out),
        }
    }
    Ok(out.trim_end().to_string())
}

fn header_line(doc: &Doc) -> String {
    if doc.kind == Kind::Doc {
        let mut parts = vec![doc.id.clone(), doc.text("role").unwrap_or("?").to_string()];
        parts.extend(doc.text("subsystem").map(str::to_string));
        parts.extend(doc.text("verified").map(|v| format!("verified {v}")));
        return format!("{} · {}", parts.join(" · "), doc.text("title").unwrap_or(""));
    }
    let mut parts = vec![doc.id.clone(), doc.status().to_string()];
    for key in ["subsystem", "kind", "novelty"] {
        if let Some(v) = doc.text(key) {
            parts.push(v.to_string());
        }
    }
    if let Some(s) = doc.text("started") {
        parts.push(format!("started {s}"));
    }
    if let Some(d) = doc.text("done") {
        parts.push(format!("done {d}"));
    }
    if let Some(e) = doc.text("estimate") {
        parts.push(format!("est {e}d"));
    }
    if let Some(a) = doc.text("active_days") {
        parts.push(format!("{a}d active"));
    }
    format!("{} · {}", parts.join(" · "), doc.text("title").unwrap_or(""))
}

pub(crate) fn show_task(doc: &Doc, out: &mut String) {
    let _ = writeln!(out, "{}", header_line(doc));
    if let Some(n) = doc.text("status_note") {
        let _ = writeln!(out, "  note: {n}");
    }
    if let Some(Value::Map(goal)) = get(&doc.entries, "goal") {
        let g = |k| get_text(goal, k).unwrap_or("?");
        let _ = writeln!(
            out,
            "  goal: {}: {} → {} · {}",
            g("metric"),
            g("baseline"),
            g("target"),
            g("instrument")
        );
    }
    if let Some(c) = doc.text("consumer") {
        let _ = writeln!(out, "  consumer: {c}");
    }
    for p in records(doc, "phases") {
        phase_line(p, out);
    }
    let _ = writeln!(out, "  {}", doc.rel);
}

fn phase_line(p: &[(String, Value)], out: &mut String) {
    let t = |k| get_text(p, k).unwrap_or("");
    let state = t("state");
    let date = if state == "done" { t("done") } else { t("started") };
    let _ =
        writeln!(out, "  {} {:<4} {:<8} {:<10} {}", mark(state), t("id"), state, date, t("story"));
    if state != "done" && state != "dropped" {
        let _ = writeln!(out, "         gate: {}", t("gate"));
    }
    if let Some(m) = get_text(p, "metric") {
        let _ = writeln!(out, "         metric: {m}");
    }
}

fn show_plain(doc: &Doc, out: &mut String) {
    let _ = writeln!(out, "{}", header_line(doc));
    if let Some(n) = doc.text("status_note") {
        let _ = writeln!(out, "  note: {n}");
    }
    let tasks = get(&doc.entries, "tasks").map(Value::items).unwrap_or_default();
    if !tasks.is_empty() {
        let _ = writeln!(out, "  tasks: {}", tasks.join(", "));
    }
    let _ = writeln!(out, "  {}", doc.rel);
}

fn show_milestone(repo: &Repo, doc: &Doc, out: &mut String) {
    let steps = records(doc, "steps");
    let done = steps.iter().filter(|s| get_text(s, "state") == Some("done")).count();
    let _ = writeln!(
        out,
        "{} · {} · {}/{} steps · {}",
        doc.id,
        doc.status(),
        done,
        steps.len(),
        doc.text("title").unwrap_or("")
    );
    let _ = writeln!(out, "  goal: {}", doc.text("goal").unwrap_or(""));
    for section in records(doc, "sections") {
        let key = get_text(section, "key").unwrap_or("");
        let _ = writeln!(out, "  {key} {}", get_text(section, "title").unwrap_or(""));
        for s in steps.iter().filter(|s| get_text(s, "section") == Some(key)) {
            let t = |k| get_text(s, k).unwrap_or("");
            let tasks: Vec<String> = get(s, "tasks")
                .map(Value::items)
                .unwrap_or_default()
                .into_iter()
                .map(|id| match repo.get(id) {
                    Some(d) if d.status() != "done" => format!("{id}({})", d.status()),
                    _ => id.to_string(),
                })
                .collect();
            let tasks =
                if tasks.is_empty() { String::new() } else { format!(" [{}]", tasks.join(", ")) };
            let _ = writeln!(
                out,
                "    {} {:<6} {}{}{}",
                mark(t("state")),
                t("key"),
                t("story"),
                tasks,
                date_suffix(t("done"))
            );
        }
    }
    let _ = writeln!(out, "  {}", doc.rel);
}

fn date_suffix(d: &str) -> String {
    if d.is_empty() {
        String::new()
    } else {
        format!(" ({d})")
    }
}

fn show_sprint(repo: &Repo, doc: &Doc, out: &mut String) {
    let _ = writeln!(
        out,
        "Sprint {} · {} · capacity {}d · {}",
        doc.id,
        doc.text("milestone").unwrap_or(""),
        doc.text("capacity_days").unwrap_or("?"),
        doc.text("goal").unwrap_or("")
    );
    for p in records(doc, "planned") {
        let task_id = get_text(p, "task").unwrap_or("?");
        let range = match (get_text(p, "p50"), get_text(p, "p85")) {
            (Some(a), Some(b)) => {
                format!(" · P50 {a}d P85 {b}d (n={})", get_text(p, "sample_size").unwrap_or("?"))
            }
            _ => String::new(),
        };
        let task = repo.get(task_id);
        let title = task.and_then(|t| t.text("title")).unwrap_or("");
        let _ = writeln!(out, "  {task_id}{range} · {title}");
        for phase in get(p, "phases").map(Value::items).unwrap_or_default() {
            match task.and_then(|t| phase_of(t, phase)) {
                Some(ph) => phase_line(ph, out),
                None => {
                    let _ = writeln!(out, "    {phase}: no such phase");
                }
            }
        }
    }
    for key in ["completed", "slipped"] {
        let items = get(&doc.entries, key).map(Value::items).unwrap_or_default();
        if !items.is_empty() {
            let _ = writeln!(out, "  {key}: {}", items.join(", "));
        }
    }
    let _ = writeln!(out, "  {}", doc.rel);
}

pub(crate) struct Filter {
    pub(crate) kind: Option<Kind>,
    pub(crate) status: Option<String>,
    pub(crate) subsystem: Option<String>,
}

pub(crate) fn list(repo: &Repo, f: &Filter) -> String {
    let mut out = String::new();
    for d in &repo.docs {
        if f.kind.is_some_and(|k| k != d.kind)
            || f.status.as_deref().is_some_and(|s| s != d.status())
            || f.subsystem.as_deref().is_some_and(|s| d.text("subsystem") != Some(s))
        {
            continue;
        }
        let _ = writeln!(
            out,
            "{:<12} {:<11} {:<12} {}",
            d.id,
            if d.kind == Kind::Doc { d.text("role").unwrap_or("-") } else { d.status() },
            d.text("subsystem").unwrap_or("-"),
            d.text("title").unwrap_or("")
        );
    }
    out.trim_end().to_string()
}

fn active_milestone(repo: &Repo) -> Option<&Doc> {
    repo.of(Kind::Milestone).find(|m| m.status() == "active")
}

/// The counted progress table of a milestone (Markdown), for a report at the end of a run.
pub(crate) fn table(repo: &Repo, id: Option<&str>) -> Result<String, String> {
    let m = match id {
        Some(id) => repo
            .get(id)
            .filter(|d| d.kind == Kind::Milestone)
            .ok_or_else(|| format!("{id}: no such milestone"))?,
        None => active_milestone(repo).ok_or("no active milestone")?,
    };
    let steps = records(m, "steps");
    let mut out = format!("**{}** — {}\n\n| Section | Done | Active | Planned | Open steps |\n|---|---|---|---|---|\n", m.text("title").unwrap_or(""), m.id);
    let (mut td, mut ta, mut tp) = (0, 0, 0);
    for section in records(m, "sections") {
        let key = get_text(section, "key").unwrap_or("");
        let mine: Vec<_> = steps.iter().filter(|s| get_text(s, "section") == Some(key)).collect();
        let count = |st: &str| mine.iter().filter(|s| get_text(s, "state") == Some(st)).count();
        let (d, a, p) = (count("done"), count("active"), count("planned"));
        td += d;
        ta += a;
        tp += p;
        let open: Vec<&str> = mine
            .iter()
            .filter(|s| matches!(get_text(s, "state"), Some("active" | "planned")))
            .filter_map(|s| get_text(s, "key"))
            .collect();
        let _ = writeln!(
            out,
            "| {key} {} | {d} | {a} | {p} | {} |",
            get_text(section, "title").unwrap_or(""),
            open.join(" ")
        );
    }
    let _ = writeln!(out, "| **Total** | **{td}** | **{ta}** | **{tp}** | |");
    let mut seen = Vec::new();
    for task in steps.iter().flat_map(|s| get(s, "tasks").map(Value::items).unwrap_or_default()) {
        if seen.contains(&task) {
            continue;
        }
        seen.push(task);
    }
    let open: Vec<String> = seen
        .into_iter()
        .filter_map(|id| repo.get(id).filter(|t| t.status() == "in_progress"))
        .map(|t| {
            let phases = records(t, "phases");
            let pd = phases.iter().filter(|p| get_text(p, "state") == Some("done")).count();
            if phases.is_empty() {
                format!("{} (no phases)", t.id)
            } else {
                format!("{} ({pd}/{} phases)", t.id, phases.len())
            }
        })
        .collect();
    if !open.is_empty() {
        let _ = writeln!(out, "\nIn progress: {}", open.join(", "));
    }
    Ok(out.trim_end().to_string())
}

/// What a session needs first: the active milestone, this week's sprint, the work in progress.
pub(crate) fn brief(repo: &Repo, week: &str) -> String {
    let mut out = String::new();
    if let Some(m) = active_milestone(repo) {
        let steps = records(m, "steps");
        let done = steps.iter().filter(|s| get_text(s, "state") == Some("done")).count();
        let _ = writeln!(
            out,
            "Milestone {} · {}/{} steps done · {}",
            m.id,
            done,
            steps.len(),
            m.text("title").unwrap_or("")
        );
        for (label, state, max) in [("active", "active", usize::MAX), ("next", "planned", 3)] {
            for s in steps.iter().filter(|s| get_text(s, "state") == Some(state)).take(max) {
                let tasks = get(s, "tasks").map(Value::items).unwrap_or_default().join(", ");
                let tasks = if tasks.is_empty() { String::new() } else { format!(" [{tasks}]") };
                let _ = writeln!(
                    out,
                    "  {label:<6} {} {}{tasks}",
                    get_text(s, "key").unwrap_or(""),
                    get_text(s, "story").unwrap_or("")
                );
            }
        }
    } else {
        out.push_str("No active milestone.\n");
    }
    match repo.get(week).filter(|d| d.kind == Kind::Sprint) {
        Some(sprint) => show_sprint(repo, sprint, &mut out),
        None => {
            let _ = writeln!(out, "No sprint for {week} yet (tasks/sprints/{week}.md is planned in the Monday conversation).");
        }
    }
    let open: Vec<&Doc> = repo.of(Kind::Task).filter(|t| t.status() == "in_progress").collect();
    let _ = writeln!(out, "In progress ({}):", open.len());
    for t in open {
        let phases = records(t, "phases");
        let current: Vec<String> = phases
            .iter()
            .filter(|p| matches!(get_text(p, "state"), Some("building" | "proving" | "blocked")))
            .map(|p| {
                format!(
                    "{} {}",
                    get_text(p, "id").unwrap_or("?"),
                    get_text(p, "state").unwrap_or("")
                )
            })
            .collect();
        let title: String = t.text("title").unwrap_or("").chars().take(70).collect();
        let _ = writeln!(
            out,
            "  {} · {} · {title}",
            t.id,
            if current.is_empty() { String::from("no open phase") } else { current.join(", ") }
        );
    }
    match super::upkeep::stale(repo) {
        Ok(s) if !s.is_empty() => {
            let _ = writeln!(
                out,
                "Docs: {} stale — their sources changed after `verified` (`nx ledger stale`).",
                s.len()
            );
        }
        Ok(_) => {}
        Err(e) => {
            let _ = writeln!(out, "Docs: drift unknown ({e}).");
        }
    }
    out.push_str("Write state with `nx ledger phase|set|step`; read with `nx ledger show <ID>`.");
    out
}
