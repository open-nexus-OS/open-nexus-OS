// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: The rules of RFC-0115 §"Rules the gate enforces": types per key, canonical order and
//!          rendering, lifecycle rules, the cutover rules, and cross-document references.
//! OWNERS: @tools-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: tools/nx/tests/ledger_cli.rs (a `test_reject_*` per rule)
//! ADR: docs/rfcs/RFC-0115-planning-as-data-milestones-sprints-phases-one-ledger-cli.md

use super::model::*;
use super::repo::{Doc, Repo};
use super::yaml::{get, get_text, Value};
use std::collections::BTreeSet;

pub(crate) struct Problem {
    pub(crate) rel: String,
    pub(crate) message: String,
    /// Fatal problems fail the gate; the others are reported.
    pub(crate) fatal: bool,
}

pub(crate) fn lint(repo: &Repo) -> Vec<Problem> {
    let mut out = Vec::new();
    for e in &repo.errors {
        out.push(Problem { rel: e.rel.clone(), message: e.message.clone(), fatal: true });
    }
    let subsystems: BTreeSet<&str> = repo
        .of(Kind::Subsystems)
        .flat_map(|d| get(&d.entries, "subsystems").map(Value::records).unwrap_or(&[]))
        .filter_map(|r| get_text(r, "slug"))
        .collect();
    for doc in &repo.docs {
        let mut fatal = Vec::new();
        let mut notes = Vec::new();
        check_doc(doc, &mut fatal);
        check_refs(repo, doc, &subsystems, &mut fatal, &mut notes);
        if fatal.is_empty() && doc.rendered() != doc.raw {
            fatal.push(String::from("not canonical; `nx ledger lint --fix` renders it"));
        }
        let rel = &doc.rel;
        out.extend(fatal.into_iter().map(|m| Problem {
            rel: rel.clone(),
            message: m,
            fatal: true,
        }));
        out.extend(notes.into_iter().map(|m| Problem {
            rel: rel.clone(),
            message: m,
            fatal: false,
        }));
    }
    out
}

/// Everything that can be judged from the document alone.
pub(crate) fn check_doc(doc: &Doc, out: &mut Vec<String>) {
    let spec = fields(doc.kind);
    let order = order(spec);
    let mut last = 0;
    for (key, value) in &doc.entries {
        let Some(pos) = order.iter().position(|k| k == key) else {
            out.push(format!("unknown key `{key}` for a {}", doc.kind.label()));
            continue;
        };
        if pos < last {
            out.push(format!("`{key}` is out of order (canonical: {})", order.join(", ")));
        }
        last = last.max(pos);
        check_value(key, spec[pos].ty, value, out);
    }
    for field in spec.iter().filter(|f| f.required) {
        if get(&doc.entries, field.key).is_none() {
            out.push(format!("`{}` is required", field.key));
        }
    }
    let id_field = get_text(&doc.entries, "id");
    if doc.kind != Kind::Subsystems && id_field != Some(doc.id.as_str()) {
        out.push(format!("`id` must be `{}` (the file name)", doc.id));
    }
    if let Some(note) = doc.text("status_note") {
        if note.chars().count() > STATUS_NOTE_MAX {
            out.push(format!("`status_note` is longer than {STATUS_NOTE_MAX} characters; history goes to the body"));
        }
    }
    if let Some(title) = doc.text("title") {
        if title.starts_with(&doc.id) {
            out.push(String::from("`title` repeats the id"));
        }
    }
    match doc.kind {
        Kind::Task => task_rules(doc, out),
        Kind::Milestone => milestone_rules(doc, out),
        Kind::Rfc | Kind::Track => {
            if doc.status() == "superseded"
                && get(&doc.entries, "superseded_by").is_none()
                && doc.kind == Kind::Rfc
            {
                out.push(String::from("a superseded RFC names its successor in `superseded_by`"));
            }
        }
        Kind::Sprint | Kind::Subsystems => {}
    }
}

fn check_value(key: &str, ty: Ty, value: &Value, out: &mut Vec<String>) {
    let text = value.as_text();
    let bad = |what: &str| format!("`{key}` must be {what}");
    let each = |ok: fn(&str) -> bool, what: &str, out: &mut Vec<String>| match value {
        Value::List(items) => {
            for item in items {
                match item {
                    Value::Str(s) if ok(s) => {}
                    other => out.push(format!(
                        "`{key}`: `{}` is not {what}",
                        other.as_text().unwrap_or("?")
                    )),
                }
            }
        }
        _ => out.push(format!("`{key}` must be a list of {what}")),
    };
    match ty {
        Ty::Text => {
            if !matches!(value, Value::Str(s) if !s.trim().is_empty()) {
                out.push(bad("non-empty text"));
            }
        }
        Ty::Schema => {
            if !matches!(value, Value::Num(n) if n == "1") {
                out.push(bad("1"));
            }
        }
        Ty::Num => {
            if !matches!(value, Value::Num(n) if n.parse::<f64>().is_ok_and(|x| x >= 0.0)) {
                out.push(bad("a non-negative number"));
            }
        }
        Ty::Slug => scalar_ok(text, is_slug, &bad("a lowercase slug"), out),
        Ty::Date => scalar_ok(text, is_date, &bad("a date YYYY-MM-DD"), out),
        Ty::Week => scalar_ok(text, is_week, &bad("an ISO week YYYY-Www"), out),
        Ty::TaskId => scalar_ok(text, is_task_id, &bad("a task id"), out),
        Ty::PhaseId => scalar_ok(text, is_phase_id, &bad("a phase id (P0, P3b)"), out),
        Ty::StepKey => scalar_ok(text, is_step_key, &bad("a step key (T0, B1.4, L6)"), out),
        Ty::Enum(allowed) => {
            if !matches!(value, Value::Str(s) if allowed.contains(&s.as_str())) {
                out.push(bad(&format!("one of {}", allowed.join(" | "))));
            }
        }
        Ty::Owners => {
            each(is_owner, "an owner (@handle)", out);
            if value.items().is_empty() {
                out.push(bad("at least one owner"));
            }
        }
        Ty::TaskIds => each(is_task_id, "a task id", out),
        Ty::RfcIds => each(is_rfc_id, "an RFC id", out),
        Ty::TrackIds => each(is_track_id, "a track id", out),
        Ty::Slugs => each(is_slug, "a slug", out),
        Ty::StoryRefs => each(is_story_ref, "a story (TASK-NNNN/P3)", out),
        Ty::PhaseIds => each(is_phase_id, "a phase id", out),
        Ty::Map(spec) => match value {
            Value::Map(map) => check_record(key, spec, map, out),
            _ => out.push(bad("a block of `key: value` lines")),
        },
        Ty::Records(spec) => match value {
            Value::Records(records) => {
                for (i, r) in records.iter().enumerate() {
                    check_record(&format!("{key}[{}]", i + 1), spec, r, out);
                }
            }
            Value::List(items) if items.is_empty() => {}
            _ => out.push(bad("a list of `- key: value` records")),
        },
    }
}

fn scalar_ok(text: Option<&str>, ok: fn(&str) -> bool, msg: &str, out: &mut Vec<String>) {
    if !text.is_some_and(ok) {
        out.push(msg.to_string());
    }
}

fn check_record(key: &str, spec: &[Field], record: &[(String, Value)], out: &mut Vec<String>) {
    let order = order(spec);
    let mut last = 0;
    for (k, v) in record {
        let Some(pos) = order.iter().position(|o| o == k) else {
            out.push(format!("`{key}`: unknown key `{k}`"));
            continue;
        };
        if pos < last {
            out.push(format!("`{key}`: `{k}` is out of order (canonical: {})", order.join(", ")));
        }
        last = last.max(pos);
        check_value(&format!("{key}.{k}"), spec[pos].ty, v, out);
    }
    for field in spec.iter().filter(|f| f.required) {
        if get(record, field.key).is_none() {
            out.push(format!("`{key}`: `{}` is required", field.key));
        }
    }
}

fn task_rules(doc: &Doc, out: &mut Vec<String>) {
    let t = |k| doc.text(k);
    let status = doc.status();
    let has = |k: &str| get(&doc.entries, k).is_some();
    let phases = get(&doc.entries, "phases").map(Value::records).unwrap_or(&[]);
    if status == "in_progress" && !has("started") {
        out.push(String::from("an in_progress task needs `started`"));
    }
    if status == "done" {
        if !has("done") {
            out.push(String::from("a done task needs `done`"));
        }
        for p in phases {
            let state = get_text(p, "state").unwrap_or("");
            if state != "done" && state != "dropped" {
                out.push(format!(
                    "a done task has phase {} in `{state}`",
                    get_text(p, "id").unwrap_or("?")
                ));
            }
        }
    }
    if status == "superseded" && !has("superseded_by") {
        out.push(String::from("a superseded task names its successor in `superseded_by`"));
    }
    if (has("started") || has("done")) && !has("dates_source") {
        out.push(String::from("`started`/`done` need `dates_source`"));
    }
    if let (Some(s), Some(d)) = (t("started"), t("done")) {
        if d < s {
            out.push(String::from("`done` is before `started`"));
        }
    }
    let mut ids = BTreeSet::new();
    for p in phases {
        let id = get_text(p, "id").unwrap_or("?");
        if !ids.insert(id) {
            out.push(format!("phase {id} appears twice"));
        }
        let state = get_text(p, "state").unwrap_or("");
        if matches!(state, "building" | "proving") && get(p, "started").is_none() {
            out.push(format!("phase {id} is {state} without `started`"));
        }
        if state == "done" && get(p, "done").is_none() {
            out.push(format!("phase {id} is done without `done`"));
        }
    }
    // RFC-0115 rule 5: work started on or after the cutover carries the planning fields.
    let new_process = t("started").is_some_and(|s| s >= CUTOVER);
    if new_process && matches!(status, "in_progress" | "done") {
        for key in ["goal", "consumer", "estimate", "kind", "novelty", "subsystem"] {
            if !has(key) {
                out.push(format!("work started from {CUTOVER} on needs `{key}` (RFC-0115 rule 5)"));
            }
        }
        if phases.is_empty() {
            out.push(format!("work started from {CUTOVER} on needs at least one phase"));
        }
        for p in phases {
            if get_text(p, "state") == Some("done") && get(p, "evidence").is_none() {
                out.push(format!(
                    "phase {} is done without `evidence`",
                    get_text(p, "id").unwrap_or("?")
                ));
            }
        }
        if status == "done" {
            if !has("active_days") {
                out.push(String::from("a done task needs `active_days`"));
            }
            if has("estimate") && !has("variance_note") {
                out.push(String::from("a done task with an estimate needs a `variance_note`"));
            }
        }
    }
}

fn milestone_rules(doc: &Doc, out: &mut Vec<String>) {
    let sections: BTreeSet<&str> = get(&doc.entries, "sections")
        .map(Value::records)
        .unwrap_or(&[])
        .iter()
        .filter_map(|r| get_text(r, "key"))
        .collect();
    let steps = get(&doc.entries, "steps").map(Value::records).unwrap_or(&[]);
    let mut keys = BTreeSet::new();
    for s in steps {
        let key = get_text(s, "key").unwrap_or("?");
        if !keys.insert(key) {
            out.push(format!("step {key} appears twice"));
        }
        if let Some(section) = get_text(s, "section") {
            if !sections.contains(section) {
                out.push(format!("step {key} names an unknown section {section}"));
            }
        }
        let state = get_text(s, "state").unwrap_or("");
        if doc.status() == "done" && state != "done" && state != "dropped" {
            out.push(format!("a done milestone has step {key} in `{state}`"));
        }
    }
    if doc.status() == "active" && doc.text("started").is_none() {
        out.push(String::from("an active milestone needs `started`"));
    }
    if doc.status() == "done" && doc.text("done").is_none() {
        out.push(String::from("a done milestone needs `done`"));
    }
}

fn check_refs(
    repo: &Repo,
    doc: &Doc,
    subsystems: &BTreeSet<&str>,
    fatal: &mut Vec<String>,
    notes: &mut Vec<String>,
) {
    if let Some(s) = doc.text("subsystem") {
        if !subsystems.is_empty() && !subsystems.contains(s) {
            fatal.push(format!("subsystem `{s}` is not in tasks/SUBSYSTEMS.md"));
        }
    }
    let exists = |id: &str| repo.get(id).is_some();
    for key in [
        "depends_on",
        "follow_ups",
        "superseded_by",
        "implements",
        "tracks",
        "tasks",
        "extends",
        "amends",
    ] {
        for id in get(&doc.entries, key).map(Value::items).unwrap_or_default() {
            if !exists(id) {
                notes.push(format!("`{key}` names {id}, which has no file"));
            }
        }
    }
    match doc.kind {
        Kind::Milestone => {
            for step in get(&doc.entries, "steps").map(Value::records).unwrap_or(&[]) {
                for id in get(step, "tasks").map(Value::items).unwrap_or_default() {
                    if !exists(id) {
                        notes.push(format!(
                            "step {} names {id}, which has no file",
                            get_text(step, "key").unwrap_or("?")
                        ));
                    }
                }
            }
        }
        Kind::Sprint => {
            if let Some(m) = doc.text("milestone") {
                if repo.get(m).is_none_or(|d| d.kind != Kind::Milestone) {
                    fatal.push(format!("milestone `{m}` has no file in tasks/milestones/"));
                }
            }
            for p in get(&doc.entries, "planned").map(Value::records).unwrap_or(&[]) {
                let task = get_text(p, "task").unwrap_or("?");
                match repo.get(task) {
                    None => fatal.push(format!("planned task {task} has no file")),
                    Some(t) => {
                        for phase in get(p, "phases").map(Value::items).unwrap_or_default() {
                            if phase_of(t, phase).is_none() {
                                fatal.push(format!("{task} has no phase {phase}"));
                            }
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

pub(crate) fn phase_of<'a>(task: &'a Doc, id: &str) -> Option<&'a [(String, Value)]> {
    get(&task.entries, "phases")
        .map(Value::records)
        .unwrap_or(&[])
        .iter()
        .find(|p| get_text(p, "id") == Some(id))
        .map(Vec::as_slice)
}
