// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Process-boundary tests for `nx ledger series` (TASK-0351 P8): a board cycle's gate
//!          lines recorded from its UART log, the table and the device server's constants
//!          rendered by `lint --fix`, the gate refusing a stale rendering, a log without a gate
//!          scene refused.
//! OWNERS: @tools-team
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 4 integration tests.
//!
//! TEST_SCOPE:
//!   - add: the cycle's values from the log, its id from the `board--<id>` folder or `--id`
//!   - lint: stale rendered files are fatal, `--fix` renders them, then the gate passes
//!   - reject: a log without a gate scene line, a path that names no cycle
//!
//! ADR: docs/adr/0094-one-kernel-three-executors-an-effect-s-mathematics-is-defined-once-in-nexus-gfx.md

use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::{tempdir, TempDir};

const SUBSYSTEMS: &str = "---\nschema: 1\nsubsystems:\n  - slug: gpu\n    name: GPU\n    tier: production-floor\n    release: consumer\n---\n\n# Subsystems\n";

const LOG: &str = "gpud: gpu frame us (n=32 avg=1 max=2)\ngpud: FAIL gpu gate scene (cold_us=48505 pipelined_us=35803 bar=23300 hover_us=12739 hover_bar=8300: the scene above its bar; warm_us=12739/16306/13531 passes=4/1 pass_wait_us=15286 iters=11 irqs=4)\ngpud: gpu work (slot_px=1 scene_px=2 blur_px=3 ns_per_px=13 opaque_px=4 straight_px=5 rounded_px=6 shadow_px=7 layers=8)\ngpud: gpu work (slot_px=1 scene_px=2 blur_px=3 ns_per_px=11 opaque_px=4 straight_px=5 rounded_px=6 shadow_px=7 layers=8)\ngpud: ux frame FAIL (dropped=3 of=32 max_us=120228 budget_us=16700)\ngpud: gpu pass sweep (base_us=700 q16_us=900 q240_us=1000 q480_us=1200 q960_us=2000 full_us=4000 area16_us=3900 blended_us=7000 frames=5)\n";

fn repo() -> TempDir {
    let dir = tempdir().expect("tempdir");
    let p = dir.path();
    for d in ["tasks", "docs/rfcs", "docs/adr", "build/logs/board--2026-11-03T19-13-30"] {
        fs::create_dir_all(p.join(d)).expect("dir");
    }
    fs::write(p.join("tasks/SUBSYSTEMS.md"), SUBSYSTEMS).expect("subsystems");
    fs::write(p.join("build/logs/board--2026-11-03T19-13-30/uart.log"), LOG).expect("log");
    dir
}

fn nx(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_nx"))
        .arg("ledger")
        .args(args)
        .env("NX_LEDGER_ROOT", root)
        .env("NX_TODAY", "2026-11-04")
        .output()
        .expect("nx runs")
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

#[test]
fn a_cycle_is_recorded_from_its_log_and_rendered_by_lint_fix() {
    let dir = repo();
    let root = dir.path();
    let log = root.join("build/logs/board--2026-11-03T19-13-30/uart.log");
    let o = nx(root, &["series", "add", log.to_str().expect("utf8")]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("recorded cycle 2026-11-03T19-13-30 (1 in"));
    let toml = fs::read_to_string(root.join("docs/board/measurements/series.toml")).expect("toml");
    assert!(toml.contains("id = \"2026-11-03T19-13-30\"\ncold_us = 48505\n"), "{toml}");
    assert!(toml.contains("ns_per_px = 11\n") || toml.contains("ns_per_px = 13\n"), "{toml}");
    assert!(toml.contains("q16_us = 900\narea16_us = 3900\n"), "{toml}");
    // The rendered files are owed: the gate says so, `--fix` renders them, then it passes.
    let o = nx(root, &["lint"]);
    assert!(!o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("series.rs: stale against the series"), "{}", text(&o));
    let o = nx(root, &["lint", "--fix"]);
    assert!(o.status.success(), "{}", text(&o));
    let rs = fs::read_to_string(root.join("source/drivers/gpud/src/backend/gpu/series.rs"))
        .expect("constants");
    assert!(rs.contains("cycle: \"2026-11-03T19-13-30\",\n    cold_us: 48505,"), "{rs}");
    let md = fs::read_to_string(root.join("docs/board/measurements/series.md")).expect("table");
    assert!(md.contains("| 2026-11-03T19-13-30 | 48505 | 35803 | 12739 |"), "{md}");
    let o = nx(root, &["lint"]);
    assert!(o.status.success(), "{}", text(&o));
    let o = nx(root, &["series", "show"]);
    assert!(o.status.success() && text(&o).contains("| 2026-11-03T19-13-30 |"), "{}", text(&o));
}

#[test]
fn the_same_cycle_added_twice_is_one_row_and_an_explicit_id_wins() {
    let dir = repo();
    let root = dir.path();
    let log = root.join("build/logs/board--2026-11-03T19-13-30/uart.log");
    let log = log.to_str().expect("utf8");
    assert!(nx(root, &["series", "add", log]).status.success());
    assert!(nx(root, &["series", "add", log]).status.success());
    let o = nx(root, &["series", "add", log, "--id", "2026-11-04T08-00-00"]);
    assert!(o.status.success() && text(&o).contains("(2 in"), "{}", text(&o));
    let toml = fs::read_to_string(root.join("docs/board/measurements/series.toml")).expect("toml");
    assert_eq!(toml.matches("[[cycle]]").count(), 2, "{toml}");
    assert!(nx(root, &["lint", "--fix"]).status.success());
    let rs = fs::read_to_string(root.join("source/drivers/gpud/src/backend/gpu/series.rs"))
        .expect("constants");
    assert!(rs.contains("cycle: \"2026-11-04T08-00-00\""), "the newest id is the last: {rs}");
}

#[test]
fn test_reject_a_log_without_a_gate_scene() {
    let dir = repo();
    let root = dir.path();
    let log = root.join("build/logs/board--2026-11-03T19-13-30/uart.log");
    fs::write(&log, "gpud: gpu work (ns_per_px=13)\n").expect("log");
    let o = nx(root, &["series", "add", log.to_str().expect("utf8")]);
    assert!(!o.status.success());
    assert!(text(&o).contains("no `gpud: gpu gate scene` line"), "{}", text(&o));
    assert!(!root.join("docs/board/measurements/series.toml").exists());
}

#[test]
fn test_reject_a_log_whose_folder_names_no_cycle() {
    let dir = repo();
    let root = dir.path();
    fs::write(root.join("build/logs/uart.log"), LOG).expect("log");
    let log = root.join("build/logs/uart.log");
    let o = nx(root, &["series", "add", log.to_str().expect("utf8")]);
    assert!(!o.status.success());
    assert!(text(&o).contains("pass `--id`"), "{}", text(&o));
}
