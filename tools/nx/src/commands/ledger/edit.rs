// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: The write side of `nx ledger`: set a key, write a phase, write a milestone step, close
//!          a goal with its result, review a sprint (completed/slipped counted from the phases),
//!          mint a task or a sprint. Every edit stamps dates from the day, re-checks the document
//!          and refuses to write one the gate would reject. Only the frontmatter changes.
//! OWNERS: @tools-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: tools/nx/tests/ledger_cli.rs
//! ADR: docs/rfcs/RFC-0115-planning-as-data-milestones-sprints-phases-one-ledger-cli.md

use super::model::{self, fields, is_slug, order, Kind, Ty, PHASE, STEP};
use super::repo::{self, Doc, Repo};
use super::rules::{check_doc, check_refs, subsystems};
use super::yaml::{self, get_text, looks_numeric, Entries, Value};
use std::fmt::Write;

/// Turn a command-line value into the value the key's type expects. `-` removes the key.
pub(crate) fn value_for(ty: Ty, raw: &str) -> Option<Value> {
    if raw == "-" {
        return None;
    }
    let item = |s: &str| if looks_numeric(s) { Value::Num(s.to_string()) } else { Value::str(s) };
    Some(match ty {
        Ty::Num | Ty::Schema => Value::Num(raw.to_string()),
        Ty::Owners
        | Ty::TaskIds
        | Ty::RfcIds
        | Ty::TrackIds
        | Ty::Slugs
        | Ty::StoryRefs
        | Ty::PhaseIds => Value::List(
            raw.split(',').map(str::trim).filter(|s| !s.is_empty()).map(Value::str).collect(),
        ),
        _ => item(raw),
    })
}

fn field_ty(spec: &[model::Field], key: &str) -> Result<Ty, String> {
    spec.iter()
        .find(|f| f.key == key)
        .map(|f| f.ty)
        .ok_or_else(|| format!("`{key}` is not a key here (keys: {})", order(spec).join(", ")))
}

/// Check, then write. Nothing is written when the edited document breaks a rule.
fn commit(repo: &Repo, doc: &mut Doc, today: &str) -> Result<(), String> {
    if matches!(doc.kind, Kind::Task | Kind::Rfc | Kind::Track) {
        let order = order(fields(doc.kind));
        yaml::set(&mut doc.entries, "updated", Some(Value::str(today)), &order);
    }
    let mut problems = Vec::new();
    check_doc(doc, &mut problems);
    let mut notes = Vec::new();
    check_refs(repo, doc, &subsystems(repo), &mut problems, &mut notes);
    if !problems.is_empty() {
        return Err(format!("{}: not written —\n  {}", doc.id, problems.join("\n  ")));
    }
    repo::write(&repo.root, doc)
}

fn take(repo: &mut Repo, id: &str) -> Result<Doc, String> {
    let i = repo
        .docs
        .iter()
        .position(|d| d.id == id)
        .ok_or_else(|| format!("{id}: no such document"))?;
    Ok(repo.docs.swap_remove(i))
}

pub(crate) fn set(
    repo: &mut Repo,
    id: &str,
    key: &str,
    raw: &str,
    today: &str,
) -> Result<String, String> {
    let mut doc = take(repo, id)?;
    let spec = fields(doc.kind);
    let order = order(spec);
    let ty = field_ty(spec, key)?;
    if matches!(ty, Ty::Map(_) | Ty::Records(_)) {
        return Err(format!("`{key}` is a block; use `nx ledger phase`, `step` or `goal`"));
    }
    yaml::set(&mut doc.entries, key, value_for(ty, raw), &order);
    if doc.kind == Kind::Task && key == "status" {
        stamp_task_status(&mut doc, raw, today, &order);
    }
    if doc.kind == Kind::Milestone && key == "status" && raw == "done" && doc.text("done").is_none()
    {
        yaml::set(&mut doc.entries, "done", Some(Value::str(today)), &order);
    }
    commit(repo, &mut doc, today)?;
    Ok(format!("{id}: {key} = {raw}"))
}

fn stamp_task_status(doc: &mut Doc, status: &str, today: &str, order: &[&str]) {
    let stamp = |doc: &mut Doc, key: &str| {
        if doc.text(key).is_none() {
            yaml::set(&mut doc.entries, key, Some(Value::str(today)), order);
            if doc.text("dates_source").is_none() {
                yaml::set(&mut doc.entries, "dates_source", Some(Value::str("recorded")), order);
            }
        }
    };
    match status {
        "in_progress" => stamp(doc, "started"),
        "done" => {
            stamp(doc, "started");
            stamp(doc, "done");
        }
        _ => {}
    }
}

pub(crate) fn goal(
    repo: &mut Repo,
    id: &str,
    parts: [&str; 4],
    today: &str,
) -> Result<String, String> {
    let mut doc = take(repo, id)?;
    if doc.kind != Kind::Task {
        return Err(format!("{id}: only a task has a goal"));
    }
    let map = ["metric", "baseline", "target", "instrument"]
        .iter()
        .zip(parts)
        .map(|(k, v)| (k.to_string(), Value::str(v)))
        .collect();
    let order = order(fields(Kind::Task));
    yaml::set(&mut doc.entries, "goal", Some(Value::Map(map)), &order);
    commit(repo, &mut doc, today)?;
    Ok(format!("{id}: goal set"))
}

/// Close the goal: the measured value against the target (rule 10).
pub(crate) fn result(
    repo: &mut Repo,
    id: &str,
    value: &str,
    today: &str,
) -> Result<String, String> {
    let mut doc = take(repo, id)?;
    if doc.kind != Kind::Task {
        return Err(format!("{id}: only a task has a goal"));
    }
    let Some(Value::Map(goal)) = yaml::get(&doc.entries, "goal") else {
        return Err(format!("{id}: no goal yet; `nx ledger goal` first"));
    };
    let mut goal = goal.clone();
    let gorder = ["metric", "baseline", "target", "instrument", "result"];
    yaml::set(&mut goal, "result", Some(Value::str(value)), &gorder);
    let order = order(fields(Kind::Task));
    yaml::set(&mut doc.entries, "goal", Some(Value::Map(goal)), &order);
    commit(repo, &mut doc, today)?;
    Ok(format!("{id}: goal.result = {value}"))
}

/// The Monday review: `completed` and `slipped` counted from the planned phases' states, written
/// into the sprint, and the review printed (what closed, what slipped, estimate against actual).
pub(crate) fn review(repo: &mut Repo, week: &str, today: &str) -> Result<String, String> {
    let mut doc = take(repo, week)?;
    if doc.kind != Kind::Sprint {
        return Err(format!("{week}: not a sprint"));
    }
    let mut completed = Vec::new();
    let mut slipped = Vec::new();
    let mut lines = Vec::new();
    for plan in yaml::get(&doc.entries, "planned").map(Value::records).unwrap_or(&[]) {
        let task_id = get_text(plan, "task").unwrap_or("?");
        let task = repo
            .get(task_id)
            .ok_or_else(|| format!("{week}: planned task {task_id} has no file"))?;
        for pid in yaml::get(plan, "phases").map(Value::items).unwrap_or_default() {
            let phase = super::rules::phase_of(task, pid)
                .ok_or_else(|| format!("{week}: {task_id} has no phase {pid}"))?;
            let state = get_text(phase, "state").unwrap_or("planned");
            let story = format!("{task_id}/{pid}");
            match state {
                "done" => completed.push(story),
                "dropped" => {}
                _ => slipped.push(story),
            }
            let date =
                if state == "done" { get_text(phase, "done") } else { get_text(phase, "started") };
            lines.push(format!(
                "  {:<14} {:<8} {:<10} {}",
                format!("{task_id}/{pid}"),
                state,
                date.unwrap_or(""),
                get_text(phase, "story").unwrap_or("")
            ));
        }
        if let (Some(p50), Some(p85)) = (get_text(plan, "p50"), get_text(plan, "p85")) {
            let actual = task.text("active_days").unwrap_or("open");
            lines
                .push(format!("  {task_id}: planned P50 {p50}d P85 {p85}d · active days {actual}"));
        }
    }
    let order = order(fields(Kind::Sprint));
    let refs = |v: &[String]| Value::List(v.iter().map(Value::str).collect());
    yaml::set(&mut doc.entries, "completed", Some(refs(&completed)), &order);
    yaml::set(&mut doc.entries, "slipped", Some(refs(&slipped)), &order);
    commit(repo, &mut doc, today)?;
    let mut out = format!(
        "Review {week} · completed {} · slipped {}
",
        completed.len(),
        slipped.len()
    );
    out.push_str(&lines.join(
        "
",
    ));
    if !slipped.is_empty() {
        out.push_str(&format!(
            "
  slipped: {}",
            slipped.join(", ")
        ));
    }
    out.push_str(&format!(
        "
  {}",
        doc.rel
    ));
    Ok(out)
}

/// Upsert one record of a block (`phases` of a task, `steps` of a milestone) by its id key.
fn upsert(
    entries: &mut Entries,
    block: &str,
    id_key: &str,
    id: &str,
    spec: &[model::Field],
    updates: &[(&str, &str)],
    order_top: &[&str],
) -> Result<bool, String> {
    let mut records: Vec<Entries> =
        yaml::get(entries, block).map(|v| v.records().to_vec()).unwrap_or_default();
    let created = !records.iter().any(|r| get_text(r, id_key) == Some(id));
    if created {
        records.push(vec![(id_key.to_string(), Value::str(id))]);
    }
    let record =
        records.iter_mut().find(|r| get_text(r, id_key) == Some(id)).ok_or("lost the record")?;
    let rorder = order(spec);
    for (k, v) in updates {
        let ty = field_ty(spec, k)?;
        yaml::set(record, k, value_for(ty, v), &rorder);
    }
    yaml::set(entries, block, Some(Value::Records(records)), order_top);
    Ok(created)
}

pub(crate) struct PhaseEdit<'a> {
    pub(crate) state: Option<&'a str>,
    pub(crate) story: Option<&'a str>,
    pub(crate) gate: Option<&'a str>,
    pub(crate) metric: Option<&'a str>,
    pub(crate) evidence: Option<&'a str>,
    pub(crate) note: Option<&'a str>,
}

pub(crate) fn phase(
    repo: &mut Repo,
    id: &str,
    pid: &str,
    e: &PhaseEdit<'_>,
    today: &str,
) -> Result<String, String> {
    let mut doc = take(repo, id)?;
    if doc.kind != Kind::Task {
        return Err(format!("{id}: phases belong to tasks"));
    }
    let order = order(fields(Kind::Task));
    let existing = super::rules::phase_of(&doc, pid).map(<[_]>::to_vec);
    let mut updates: Vec<(&str, &str)> = Vec::new();
    for (k, v) in [
        ("story", e.story),
        ("gate", e.gate),
        ("metric", e.metric),
        ("evidence", e.evidence),
        ("note", e.note),
    ] {
        if let Some(v) = v {
            updates.push((k, v));
        }
    }
    let state = e.state.unwrap_or_else(|| if existing.is_none() { "planned" } else { "" });
    if !state.is_empty() {
        updates.insert(0, ("state", state));
        let has = |k: &str| existing.as_ref().and_then(|r| get_text(r, k)).is_some();
        if matches!(state, "building" | "proving" | "done") && !has("started") {
            updates.push(("started", today));
        }
        if state == "done" && !has("done") {
            updates.push(("done", today));
        }
    }
    upsert(&mut doc.entries, "phases", "id", pid, PHASE, &updates, &order)?;
    // Work on a phase starts the task, and the task's planned milestone steps become active.
    let starts = matches!(state, "building" | "proving" | "done") && doc.status() == "draft";
    if starts {
        yaml::set(&mut doc.entries, "status", Some(Value::str("in_progress")), &order);
        stamp_task_status(&mut doc, "in_progress", today, &order);
    }
    commit(repo, &mut doc, today)?;
    let mut out = String::new();
    super::view::show_task(&doc, &mut out);
    if starts {
        repo.docs.push(doc);
        for key in activate_steps(repo, id, today)? {
            let _ = writeln!(out, "{key}: step active");
        }
    }
    Ok(out.trim_end().to_string())
}

/// Starting a task activates every planned milestone step that names it (rule 12 derived, not
/// typed). Returns `milestone/KEY` per activated step.
fn activate_steps(repo: &mut Repo, task_id: &str, today: &str) -> Result<Vec<String>, String> {
    let mut activated = Vec::new();
    let milestones: Vec<String> = repo.of(Kind::Milestone).map(|d| d.id.clone()).collect();
    for mid in milestones {
        let keys: Vec<String> = repo
            .get(&mid)
            .map(|m| {
                yaml::get(&m.entries, "steps")
                    .map(Value::records)
                    .unwrap_or(&[])
                    .iter()
                    .filter(|s| get_text(s, "state") == Some("planned"))
                    .filter(|s| {
                        yaml::get(s, "tasks")
                            .map(Value::items)
                            .unwrap_or_default()
                            .contains(&task_id)
                    })
                    .filter_map(|s| get_text(s, "key").map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        for key in keys {
            step(repo, &mid, &key, &[("state", "active")], today)?;
            activated.push(format!("{mid}/{key}"));
        }
    }
    Ok(activated)
}

pub(crate) fn step(
    repo: &mut Repo,
    mid: &str,
    key: &str,
    updates: &[(&str, &str)],
    today: &str,
) -> Result<String, String> {
    let mut doc = take(repo, mid)?;
    if doc.kind != Kind::Milestone {
        return Err(format!("{mid}: not a milestone"));
    }
    let order = order(fields(Kind::Milestone));
    let mut updates = updates.to_vec();
    if updates.iter().any(|(k, v)| *k == "state" && *v == "done")
        && !updates.iter().any(|(k, _)| *k == "done")
    {
        let had = yaml::get(&doc.entries, "steps")
            .map(Value::records)
            .unwrap_or(&[])
            .iter()
            .any(|r| get_text(r, "key") == Some(key) && get_text(r, "done").is_some());
        if !had {
            updates.push(("done", today));
        }
    }
    let created = upsert(&mut doc.entries, "steps", "key", key, STEP, &updates, &order)?;
    commit(repo, &mut doc, today)?;
    repo.docs.push(doc);
    Ok(format!("{mid}: step {key} {}", if created { "added" } else { "updated" }))
}

/// Add or change one planned story set of a sprint (a task and the phases chosen this week).
pub(crate) fn plan(
    repo: &mut Repo,
    week: &str,
    task: &str,
    updates: &[(&str, &str)],
    today: &str,
) -> Result<String, String> {
    let mut doc = take(repo, week)?;
    if doc.kind != Kind::Sprint {
        return Err(format!("{week}: not a sprint"));
    }
    let planned: &[model::Field] = match field_ty(fields(Kind::Sprint), "planned")? {
        Ty::Records(spec) => spec,
        _ => return Err("planned is not a block".into()),
    };
    let order = order(fields(Kind::Sprint));
    upsert(&mut doc.entries, "planned", "task", task, planned, updates, &order)?;
    commit(repo, &mut doc, today)?;
    Ok(format!("{week}: {task} planned"))
}

fn slugify(title: &str) -> String {
    let mut s = String::new();
    for c in title.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            s.push(c);
        } else if !s.ends_with('-') && !s.is_empty() {
            s.push('-');
        }
    }
    let s = s.trim_end_matches('-');
    s.chars().take(80).collect::<String>().trim_end_matches('-').to_string()
}

pub(crate) fn new_task(
    repo: &Repo,
    title: &str,
    subsystem: &str,
    owners: &str,
    today: &str,
) -> Result<String, String> {
    if !is_slug(subsystem) {
        return Err(format!("`{subsystem}` is not a subsystem slug"));
    }
    let next = repo
        .of(Kind::Task)
        .filter_map(|d| d.id.get(5..9).and_then(|n| n.parse::<u32>().ok()))
        .max()
        .unwrap_or(0)
        + 1;
    let id = format!("TASK-{next:04}");
    let rel = format!("tasks/{id}-{}.md", slugify(title));
    let entries: Entries = vec![
        ("schema".into(), Value::Num("1".into())),
        ("id".into(), Value::str(&id)),
        ("title".into(), Value::str(title)),
        ("status".into(), Value::str("draft")),
        ("owner".into(), value_for(Ty::Owners, owners).unwrap_or(Value::List(Vec::new()))),
        ("created".into(), Value::str(today)),
        ("subsystem".into(), Value::str(subsystem)),
    ];
    let body = template_body(repo);
    let doc = Doc {
        kind: Kind::Task,
        id: id.clone(),
        rel: rel.clone(),
        entries,
        body: body.clone(),
        raw: String::new(),
    };
    let mut problems = Vec::new();
    check_doc(&doc, &mut problems);
    if !problems.is_empty() {
        return Err(format!("{id}: not written —\n  {}", problems.join("\n  ")));
    }
    repo::write_text(&repo.root, &rel, &yaml::render_file(&doc.entries, &body))?;
    Ok(format!("{id}: {rel}"))
}

fn template_body(repo: &Repo) -> String {
    std::fs::read_to_string(repo.root.join("tasks/TASK-TEMPLATE.md"))
        .ok()
        .and_then(|t| yaml::split(&t).ok().map(|s| s.body.to_string()))
        .unwrap_or_else(|| String::from("\n## The ask\n\n## Measured before the cut\n\n## Not doing\n\n## Definition of Done\n"))
}

pub(crate) fn new_sprint(
    repo: &Repo,
    week: &str,
    milestone: &str,
    capacity: &str,
    goal: Option<&str>,
) -> Result<String, String> {
    let rel = format!("tasks/sprints/{week}.md");
    if repo.root.join(&rel).exists() {
        return Err(format!("{rel} exists"));
    }
    let mut entries: Entries = vec![
        ("schema".into(), Value::Num("1".into())),
        ("id".into(), Value::str(week)),
        ("milestone".into(), Value::str(milestone)),
        ("capacity_days".into(), Value::Num(capacity.into())),
    ];
    if let Some(g) = goal {
        entries.push(("goal".into(), Value::str(g)));
    }
    entries.push(("planned".into(), Value::Records(Vec::new())));
    let body = String::from("\n# Sprint ") + week + "\n\n## Plan\n\n## Review\n";
    let doc = Doc {
        kind: Kind::Sprint,
        id: week.into(),
        rel: rel.clone(),
        entries,
        body,
        raw: String::new(),
    };
    let mut problems = Vec::new();
    check_doc(&doc, &mut problems);
    if !problems.is_empty() {
        return Err(format!("{week}: not written —\n  {}", problems.join("\n  ")));
    }
    std::fs::create_dir_all(repo.root.join("tasks/sprints")).map_err(|e| e.to_string())?;
    repo::write(&repo.root, &doc)?;
    Ok(format!("{week}: {rel}"))
}
