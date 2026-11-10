// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Process-boundary tests for `nx ledger` (RFC-0115): the canonical round trip, the
//!          rules the gate enforces (one `test_reject_*` per rule), and edits that stamp dates,
//!          keep the body byte for byte and refuse what the gate would reject.
//! OWNERS: @tools-team
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 27 integration tests.
//!
//! TEST_SCOPE:
//!   - lint: canonical files pass, every rule rejects its violation (rules 1–12)
//!   - set/phase/step/plan/goal/result/review/new: dates stamped from NX_TODAY, body preserved
//!   - brief/show/table: compact read views
//!
//! ADR: docs/rfcs/RFC-0115-planning-as-data-milestones-sprints-phases-one-ledger-cli.md

use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::{tempdir, TempDir};

const SUBSYSTEMS: &str = "---\nschema: 1\nsubsystems:\n  - slug: ui\n    name: Windowing\n    tier: production-floor\n    release: consumer\n---\n\n# Subsystems\n";

const TASK: &str = "---\nschema: 1\nid: TASK-0001\ntitle: \"A window opens at once\"\nstatus: draft\nowner: [\"@ui\"]\ncreated: 2026-10-01\nsubsystem: ui\n---\n\n# TASK-0001\n\nBody text that must survive: `code`, ünïcode — and a trailing line.\n";

const MILESTONE: &str = "---\nschema: 1\nid: fast-lane\ntitle: \"Fast lane\"\nstatus: active\nstarted: 2026-10-01\ngoal: \"a goal\"\nsections:\n  - key: B0\n    title: \"Block zero\"\nsteps:\n  - key: T0\n    section: B0\n    story: \"the first step\"\n    state: planned\n    tasks: [TASK-0001]\n---\n\n# Fast lane\n";

fn repo() -> TempDir {
    let dir = tempdir().expect("tempdir");
    fs::create_dir_all(dir.path().join("tasks/milestones")).expect("tasks");
    fs::create_dir_all(dir.path().join("docs/rfcs")).expect("rfcs");
    fs::write(dir.path().join("tasks/SUBSYSTEMS.md"), SUBSYSTEMS).expect("subsystems");
    fs::write(dir.path().join("tasks/TASK-0001-a-window.md"), TASK).expect("task");
    fs::write(dir.path().join("tasks/milestones/fast-lane.md"), MILESTONE).expect("milestone");
    dir
}

fn nx(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_nx"))
        .arg("ledger")
        .args(args)
        .env("NX_LEDGER_ROOT", root)
        .env("NX_TODAY", "2026-11-03")
        .output()
        .expect("nx runs")
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

fn task(root: &Path) -> String {
    fs::read_to_string(root.join("tasks/TASK-0001-a-window.md")).expect("read task")
}

fn lint_rejects(root: &Path, needle: &str) {
    let o = nx(root, &["lint"]);
    assert_eq!(o.status.code(), Some(3), "lint should fail: {}", text(&o));
    assert!(text(&o).contains(needle), "expected `{needle}` in: {}", text(&o));
}

fn with_task(front_edit: &str, replace: &str) -> TempDir {
    let dir = repo();
    let t = TASK.replace(front_edit, replace);
    fs::write(dir.path().join("tasks/TASK-0001-a-window.md"), t).expect("write");
    dir
}

/// A task planned the RFC-0115 way and started today (the new process): goal, consumer (the
/// next milestone step, T1), estimate, reference class, P0 building — which activates T0.
fn planned_task(root: &Path) {
    for args in [
        vec![
            "step",
            "fast-lane",
            "T1",
            "--section",
            "B0",
            "--story",
            "the next step",
            "--state",
            "planned",
        ],
        vec!["goal", "TASK-0001", "frame p99", "20 ms", "16 ms", "windowd: frame p99="],
        vec!["set", "TASK-0001", "consumer", "fast-lane/T1"],
        vec!["set", "TASK-0001", "estimate", "2"],
        vec!["set", "TASK-0001", "kind", "ui"],
        vec!["set", "TASK-0001", "novelty", "variant"],
        vec![
            "phase",
            "TASK-0001",
            "P0",
            "--story",
            "measured",
            "--gate",
            "a folder",
            "--state",
            "building",
        ],
    ] {
        let o = nx(root, &args);
        assert!(o.status.success(), "{args:?}: {}", text(&o));
    }
}

#[test]
fn canonical_repo_lints_clean() {
    let dir = repo();
    let o = nx(dir.path(), &["lint"]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("0 error(s)"));
}

#[test]
fn test_reject_unquoted_text_with_spaces() {
    let dir = with_task("title: \"A window opens at once\"", "title: A window opens at once");
    lint_rejects(dir.path(), "must be quoted");
}

#[test]
fn test_reject_unknown_key() {
    let dir = with_task("subsystem: ui\n", "subsystem: ui\nsize: M\n");
    lint_rejects(dir.path(), "unknown key `size`");
}

#[test]
fn test_reject_keys_out_of_order() {
    let dir = with_task("status: draft\nowner: [\"@ui\"]\n", "owner: [\"@ui\"]\nstatus: draft\n");
    lint_rejects(dir.path(), "out of order");
}

#[test]
fn test_reject_id_not_matching_file_name() {
    let dir = with_task("id: TASK-0001", "id: TASK-0002");
    lint_rejects(dir.path(), "the file name");
}

#[test]
fn test_reject_long_status_note() {
    let note = "x".repeat(161);
    let dir = with_task("status: draft\n", &format!("status: draft\nstatus_note: \"{note}\"\n"));
    lint_rejects(dir.path(), "longer than 160");
}

#[test]
fn test_reject_in_progress_without_started() {
    let dir = with_task("status: draft", "status: in_progress");
    lint_rejects(dir.path(), "needs `started`");
}

#[test]
fn test_reject_superseded_without_successor() {
    let dir = with_task("status: draft", "status: superseded");
    lint_rejects(dir.path(), "names its successor");
}

#[test]
fn test_reject_unknown_subsystem() {
    let dir = with_task("subsystem: ui", "subsystem: gpu");
    lint_rejects(dir.path(), "not in tasks/SUBSYSTEMS.md");
}

#[test]
fn test_reject_new_work_without_goal_and_phases() {
    let dir = with_task("status: draft\n", "status: in_progress\n");
    let t = fs::read_to_string(dir.path().join("tasks/TASK-0001-a-window.md")).expect("read");
    let t = t
        .replace("subsystem: ui\n", "subsystem: ui\nstarted: 2026-11-03\ndates_source: recorded\n");
    fs::write(dir.path().join("tasks/TASK-0001-a-window.md"), t).expect("write");
    lint_rejects(dir.path(), "needs `goal` (RFC-0115 rule 5)");
}

#[test]
fn test_reject_done_task_with_open_phase() {
    let dir = repo();
    assert!(nx(dir.path(), &["phase", "TASK-0001", "P0", "--story", "s", "--gate", "g"])
        .status
        .success());
    let o = nx(dir.path(), &["set", "TASK-0001", "status", "done"]);
    assert_eq!(o.status.code(), Some(3), "{}", text(&o));
    assert!(text(&o).contains("phase P0 in `planned`"), "{}", text(&o));
    assert!(task(dir.path()).contains("status: draft"), "nothing written on a refused edit");
}

#[test]
fn test_reject_milestone_step_in_unknown_section() {
    let dir = repo();
    let o = nx(
        dir.path(),
        &["step", "fast-lane", "T1", "--section", "B9", "--story", "s", "--state", "planned"],
    );
    assert_eq!(o.status.code(), Some(3), "{}", text(&o));
    assert!(text(&o).contains("unknown section B9"));
}

#[test]
fn test_reject_sprint_with_unknown_phase() {
    let dir = repo();
    assert!(nx(dir.path(), &["new", "sprint", "--milestone", "fast-lane", "--capacity", "5"])
        .status
        .success());
    let o = nx(dir.path(), &["plan", "TASK-0001", "P4"]);
    assert_eq!(o.status.code(), Some(3), "a plan names phases that exist: {}", text(&o));
    assert!(text(&o).contains("TASK-0001 has no phase P4"), "{}", text(&o));
    assert!(nx(dir.path(), &["lint"]).status.success(), "nothing written on a refused plan");
}

#[test]
fn test_reject_in_progress_with_a_planned_phase_and_none_open() {
    let dir = repo();
    planned_task(dir.path());
    assert!(nx(
        dir.path(),
        &["phase", "TASK-0001", "P1", "--story", "built", "--gate", "lane green"]
    )
    .status
    .success());
    let o =
        nx(dir.path(), &["phase", "TASK-0001", "P0", "--state", "done", "--evidence", "logs/x"]);
    assert_eq!(o.status.code(), Some(3), "{}", text(&o));
    assert!(text(&o).contains("(rule 8)"), "{}", text(&o));
    // Closing the last phase is not a gap: the task waits for its closing.
    assert!(nx(dir.path(), &["phase", "TASK-0001", "P1", "--state", "dropped"]).status.success());
    let o =
        nx(dir.path(), &["phase", "TASK-0001", "P0", "--state", "done", "--evidence", "logs/x"]);
    assert!(o.status.success(), "{}", text(&o));
}

#[test]
fn test_reject_consumer_that_is_prose_or_the_task_itself() {
    let dir = repo();
    planned_task(dir.path());
    let o = nx(dir.path(), &["set", "TASK-0001", "consumer", "every app on the desktop"]);
    assert_eq!(o.status.code(), Some(3), "{}", text(&o));
    assert!(text(&o).contains("(rule 9)"), "{}", text(&o));
    let o = nx(dir.path(), &["set", "TASK-0001", "consumer", "TASK-0001"]);
    assert!(text(&o).contains("must not be the task itself (rule 9)"), "{}", text(&o));
    assert!(task(dir.path()).contains("consumer: fast-lane/T1"), "nothing written");
}

#[test]
fn test_reject_done_without_result_or_for_a_consumer_that_never_started() {
    let dir = repo();
    planned_task(dir.path());
    for args in [
        ["phase", "TASK-0001", "P0", "--state", "done", "--evidence", "logs/x"],
        ["set", "TASK-0001", "active_days", "3", "", "", ""],
        ["set", "TASK-0001", "variance_note", "the board needed a second cycle", "", "", ""],
    ] {
        let args: Vec<&str> = args.iter().copied().filter(|a| !a.is_empty()).collect();
        assert!(nx(dir.path(), &args).status.success(), "{args:?}");
    }
    let o = nx(dir.path(), &["set", "TASK-0001", "status", "done"]);
    assert_eq!(o.status.code(), Some(3), "{}", text(&o));
    assert!(text(&o).contains("`goal.result`"), "{}", text(&o));
    assert!(text(&o).contains("(rule 10)"), "{}", text(&o));
    assert!(nx(dir.path(), &["result", "TASK-0001", "15.8 ms"]).status.success());
    assert!(
        task(dir.path()).contains("  instrument: \"windowd: frame p99=\"\n  result: \"15.8 ms\"\n")
    );
    let o = nx(dir.path(), &["set", "TASK-0001", "status", "done"]);
    assert_eq!(o.status.code(), Some(3), "{}", text(&o));
    assert!(text(&o).contains("consumer fast-lane/T1 has not started"), "{}", text(&o));
    assert!(nx(dir.path(), &["step", "fast-lane", "T1", "--state", "active"]).status.success());
    let o = nx(dir.path(), &["set", "TASK-0001", "status", "done"]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(nx(dir.path(), &["lint"]).status.success());
}

#[test]
fn test_reject_step_done_while_its_task_runs() {
    let dir = repo();
    planned_task(dir.path());
    let m = fs::read_to_string(dir.path().join("tasks/milestones/fast-lane.md")).expect("read");
    assert!(
        m.contains(
            "  - key: T0\n    section: B0\n    story: \"the first step\"\n    state: active\n"
        ),
        "starting the task activates its step: {m}"
    );
    let o = nx(dir.path(), &["step", "fast-lane", "T0", "--state", "done"]);
    assert_eq!(o.status.code(), Some(3), "{}", text(&o));
    assert!(
        text(&o).contains("step T0 is done while TASK-0001 is in_progress (rule 12)"),
        "{}",
        text(&o)
    );
}

#[test]
fn review_counts_completed_and_slipped_from_the_phases() {
    let dir = repo();
    planned_task(dir.path());
    assert!(nx(dir.path(), &["phase", "TASK-0001", "P1", "--story", "built", "--gate", "g"])
        .status
        .success());
    assert!(nx(dir.path(), &["new", "sprint", "--milestone", "fast-lane", "--capacity", "5"])
        .status
        .success());
    assert!(nx(dir.path(), &["plan", "TASK-0001", "P0,P1", "--p50", "2", "--p85", "3"])
        .status
        .success());
    assert!(nx(dir.path(), &["phase", "TASK-0001", "P1", "--state", "building"]).status.success());
    assert!(nx(
        dir.path(),
        &["phase", "TASK-0001", "P0", "--state", "done", "--evidence", "logs/x"]
    )
    .status
    .success());
    let o = nx(dir.path(), &["review"]);
    let t = text(&o);
    assert!(o.status.success(), "{t}");
    assert!(t.contains("Review 2026-W45 · completed 1 · slipped 1"), "{t}");
    assert!(t.contains("TASK-0001: planned P50 2d P85 3d · active days open"), "{t}");
    let sprint = fs::read_to_string(dir.path().join("tasks/sprints/2026-W45.md")).expect("read");
    assert!(
        sprint.contains("completed: [TASK-0001/P0]\nslipped: [TASK-0001/P1]\n---\n"),
        "{sprint}"
    );
    assert!(nx(dir.path(), &["lint"]).status.success());
}

#[test]
fn lint_fix_renders_canonically() {
    let dir = with_task("title: \"A window opens at once\"", "title: \"Window\"");
    let t = task(dir.path())
        .replace("status: draft\nowner: [\"@ui\"]\n", "owner: [\"@ui\"]\nstatus: draft\n");
    fs::write(dir.path().join("tasks/TASK-0001-a-window.md"), t).expect("write");
    let o = nx(dir.path(), &["lint", "--fix"]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(task(dir.path()).contains("title: Window\nstatus: draft\nowner:"));
}

#[test]
fn phase_building_starts_the_task_and_keeps_the_body() {
    let dir = repo();
    assert!(nx(dir.path(), &["goal", "TASK-0001", "open time", "300 ms", "100 ms", "ux launch"])
        .status
        .success());
    for (k, v) in
        [("consumer", "fast-lane/T0"), ("estimate", "2"), ("kind", "ui"), ("novelty", "first")]
    {
        assert!(nx(dir.path(), &["set", "TASK-0001", k, v]).status.success());
    }
    let o = nx(
        dir.path(),
        &[
            "phase",
            "TASK-0001",
            "P1",
            "--story",
            "the window opens",
            "--gate",
            "ux launch ok",
            "--state",
            "building",
        ],
    );
    assert!(o.status.success(), "{}", text(&o));
    let t = task(dir.path());
    assert!(t.contains("status: in_progress"));
    assert!(t.contains("started: 2026-11-03"));
    assert!(t.contains("dates_source: recorded"));
    assert!(t.contains("updated: 2026-11-03"));
    assert!(t.ends_with("ünïcode — and a trailing line.\n"), "body preserved");
    assert!(nx(dir.path(), &["lint"]).status.success());
}

#[test]
fn new_work_cannot_start_without_its_goal() {
    let dir = repo();
    let o = nx(
        dir.path(),
        &["phase", "TASK-0001", "P1", "--story", "s", "--gate", "g", "--state", "building"],
    );
    assert_eq!(o.status.code(), Some(3));
    assert!(text(&o).contains("needs `goal`"));
    assert!(task(dir.path()).contains("status: draft"));
}

#[test]
fn step_done_stamps_its_date_and_table_counts_it() {
    let dir = repo();
    assert!(nx(dir.path(), &["step", "fast-lane", "T0", "--state", "done"]).status.success());
    let m = fs::read_to_string(dir.path().join("tasks/milestones/fast-lane.md")).expect("read");
    assert!(m.contains("    state: done\n    done: 2026-11-03\n"));
    let o = nx(dir.path(), &["table"]);
    assert!(text(&o).contains("| **Total** | **1** | **0** | **0** | |"), "{}", text(&o));
}

#[test]
fn step_after_places_a_step_in_the_order() {
    let dir = repo();
    let add = |key: &str, extra: &[&str]| {
        let mut args =
            vec!["step", "fast-lane", key, "--section", "B0", "--story", "s", "--state", "planned"];
        args.extend_from_slice(extra);
        let o = nx(dir.path(), &args);
        assert!(o.status.success(), "{}", text(&o));
    };
    add("T2", &[]);
    add("T1", &["--after", "T0"]);
    let m = fs::read_to_string(dir.path().join("tasks/milestones/fast-lane.md")).expect("read");
    let at = |k: &str| m.find(&format!("  - key: {k}\n")).expect("step");
    assert!(at("T0") < at("T1") && at("T1") < at("T2"), "{m}");
    assert!(nx(dir.path(), &["lint"]).status.success());
}

#[test]
fn test_reject_step_after_an_unknown_step_or_itself() {
    let dir = repo();
    let before =
        fs::read_to_string(dir.path().join("tasks/milestones/fast-lane.md")).expect("read");
    for after in ["T9", "T0"] {
        let o = nx(dir.path(), &["step", "fast-lane", "T0", "--after", after]);
        assert_eq!(o.status.code(), Some(3), "{}", text(&o));
    }
    let m = fs::read_to_string(dir.path().join("tasks/milestones/fast-lane.md")).expect("read");
    assert_eq!(m, before);
}

#[test]
fn new_task_takes_the_next_number() {
    let dir = repo();
    let o = nx(
        dir.path(),
        &["new", "task", "--title", "The caret at once", "--subsystem", "ui", "--owner", "@ui"],
    );
    assert!(o.status.success(), "{}", text(&o));
    assert!(dir.path().join("tasks/TASK-0002-the-caret-at-once.md").exists());
    assert!(nx(dir.path(), &["lint"]).status.success());
}

#[test]
fn brief_names_the_milestone_the_sprint_gap_and_open_work() {
    let dir = repo();
    let o = nx(dir.path(), &["brief"]);
    let t = text(&o);
    assert!(t.contains("Milestone fast-lane · 0/1 steps done"), "{t}");
    assert!(t.contains("No sprint for 2026-W45"), "{t}");
    assert!(t.contains("In progress (0):"), "{t}");
}

#[test]
fn show_is_compact() {
    let dir = repo();
    let o = nx(dir.path(), &["show", "TASK-0001", "fast-lane"]);
    let t = text(&o);
    assert!(t.starts_with("TASK-0001 · draft · ui · A window opens at once"), "{t}");
    assert!(t.contains("· T0     the first step [TASK-0001(draft)]"), "{t}");
}
