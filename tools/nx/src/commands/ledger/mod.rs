// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: `nx ledger` — the one writer of planning state (RFC-0115): milestones, sprints, tasks,
//!          tracks and RFCs as canonical frontmatter; brief/show/list/table to read, set/phase/
//!          step/goal/result/plan/review/new to write, lint as the gate.
//! OWNERS: @tools-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: tools/nx/tests/ledger_cli.rs
//! ADR: docs/rfcs/RFC-0115-planning-as-data-milestones-sprints-phases-one-ledger-cli.md

mod edit;
mod model;
mod repo;
mod rules;
mod view;
mod yaml;

use crate::cli_ledger::{LedgerAction, LedgerArgs, NewKind};
use crate::error::{ExecResult, ExitClass, NxError};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) fn handle_ledger(args: LedgerArgs) -> ExecResult {
    let root = repo::find_root().map_err(|e| NxError::new(ExitClass::Usage, e))?;
    let mut repo = repo::load(&root);
    let today = today().map_err(|e| NxError::new(ExitClass::Usage, e))?;
    let ok = |text: String| Ok((ExitClass::Success, text, false, None));
    let fail = |e: String| NxError::new(ExitClass::ValidationReject, e);
    match args.action {
        LedgerAction::Brief => ok(view::brief(&repo, &iso_week(&today))),
        LedgerAction::Show { ids } => view::show(&repo, &ids).map_err(fail).and_then(ok),
        LedgerAction::List { kind, status, subsystem } => {
            let kind = match kind.as_deref() {
                Some(k) => {
                    Some(model::Kind::parse(k).ok_or_else(|| fail(format!("unknown kind `{k}`")))?)
                }
                None => None,
            };
            ok(view::list(&repo, &view::Filter { kind, status, subsystem }))
        }
        LedgerAction::Table { milestone } => {
            view::table(&repo, milestone.as_deref()).map_err(fail).and_then(ok)
        }
        LedgerAction::Set { id, key, value } => {
            edit::set(&mut repo, &id, &key, &value, &today).map_err(fail).and_then(ok)
        }
        LedgerAction::Goal { id, metric, baseline, target, instrument } => {
            edit::goal(&mut repo, &id, [&metric, &baseline, &target, &instrument], &today)
                .map_err(fail)
                .and_then(ok)
        }
        LedgerAction::Result { id, value } => {
            edit::result(&mut repo, &id, &value, &today).map_err(fail).and_then(ok)
        }
        LedgerAction::Review { week } => {
            let week = week.unwrap_or_else(|| iso_week(&today));
            edit::review(&mut repo, &week, &today).map_err(fail).and_then(ok)
        }
        LedgerAction::Phase { id, phase, state, story, gate, metric, evidence, note } => {
            let e = edit::PhaseEdit {
                state: state.as_deref(),
                story: story.as_deref(),
                gate: gate.as_deref(),
                metric: metric.as_deref(),
                evidence: evidence.as_deref(),
                note: note.as_deref(),
            };
            edit::phase(&mut repo, &id, &phase, &e, &today).map_err(fail).and_then(ok)
        }
        LedgerAction::Step { milestone, key, state, story, section, tasks, gate } => {
            let mut updates = Vec::new();
            for (k, v) in [
                ("section", &section),
                ("story", &story),
                ("state", &state),
                ("tasks", &tasks),
                ("gate", &gate),
            ] {
                if let Some(v) = v {
                    updates.push((k, v.as_str()));
                }
            }
            edit::step(&mut repo, &milestone, &key, &updates, &today).map_err(fail).and_then(ok)
        }
        LedgerAction::Plan { week, task, phases, p50, p85, sample_size } => {
            let mut updates = vec![("phases", phases.as_str())];
            for (k, v) in [("p50", &p50), ("p85", &p85), ("sample_size", &sample_size)] {
                if let Some(v) = v {
                    updates.push((k, v.as_str()));
                }
            }
            let week = week.unwrap_or_else(|| iso_week(&today));
            edit::plan(&mut repo, &week, &task, &updates, &today).map_err(fail).and_then(ok)
        }
        LedgerAction::New { kind } => match kind {
            NewKind::Task { title, subsystem, owner } => {
                edit::new_task(&repo, &title, &subsystem, &owner, &today).map_err(fail).and_then(ok)
            }
            NewKind::Sprint { week, milestone, capacity, goal } => {
                let week = week.unwrap_or_else(|| iso_week(&today));
                edit::new_sprint(&repo, &week, &milestone, &capacity, goal.as_deref())
                    .map_err(fail)
                    .and_then(ok)
            }
        },
        LedgerAction::Lint { fix, notes } => lint(&mut repo, fix, notes),
    }
}

fn lint(repo: &mut repo::Repo, fix: bool, notes: bool) -> ExecResult {
    if fix {
        let mut fixed = 0;
        for doc in &mut repo.docs {
            let order = model::order(model::fields(doc.kind));
            doc.entries
                .sort_by_key(|(k, _)| order.iter().position(|o| o == k).unwrap_or(usize::MAX));
            if doc.rendered() != doc.raw {
                repo::write(&repo.root, doc).map_err(|e| NxError::new(ExitClass::Internal, e))?;
                doc.raw = doc.rendered();
                fixed += 1;
            }
        }
        if fixed > 0 {
            println!("rendered {fixed} document(s) canonically");
        }
    }
    let problems = rules::lint(repo);
    let fatal = problems.iter().filter(|p| p.fatal).count();
    let mut text = String::new();
    for p in problems.iter().filter(|p| p.fatal || notes) {
        text.push_str(&format!(
            "{} {}: {}\n",
            if p.fatal { "error" } else { "note " },
            p.rel,
            p.message
        ));
    }
    let summary = format!(
        "ledger: {} documents, {fatal} error(s), {} note(s)",
        repo.docs.len() + repo.errors.len(),
        problems.len() - fatal
    );
    text.push_str(&summary);
    if fatal > 0 {
        Err(NxError::new(ExitClass::ValidationReject, text))
    } else {
        Ok((ExitClass::Success, text, false, None))
    }
}

/// Today (UTC) as `YYYY-MM-DD`; `NX_TODAY` overrides it for tests and replays.
fn today() -> Result<String, String> {
    if let Ok(d) = std::env::var("NX_TODAY") {
        return if model::is_date(&d) {
            Ok(d)
        } else {
            Err(format!("NX_TODAY `{d}` is not a date"))
        };
    }
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| e.to_string())?.as_secs();
    let (y, m, d) = civil(i64::try_from(secs / 86_400).map_err(|e| e.to_string())?);
    Ok(format!("{y:04}-{m:02}-{d:02}"))
}

/// Days since 1970-01-01 → (year, month, day), proleptic Gregorian.
fn civil(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

fn days(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = i64::from(if m > 2 { m - 3 } else { m + 9 });
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The ISO week (`YYYY-Www`) of a date; the week belongs to the year of its Thursday.
pub(crate) fn iso_week(date: &str) -> String {
    let p = |a: usize, b: usize| date.get(a..b).and_then(|s| s.parse::<i64>().ok()).unwrap_or(1);
    let n = days(p(0, 4), p(5, 7) as u32, p(8, 10) as u32);
    let weekday = (n + 3).rem_euclid(7); // Monday = 0
    let thursday = n - weekday + 3;
    let (year, _, _) = civil(thursday);
    let week = (thursday - days(year, 1, 1)) / 7 + 1;
    format!("{year:04}-W{week:02}")
}
