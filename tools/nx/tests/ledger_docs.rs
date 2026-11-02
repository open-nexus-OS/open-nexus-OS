// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Process-boundary tests for the doc rules of `nx ledger` (TASK-0349): a doc's
//!          frontmatter, sources, contracts, links, status words and word budget; generated
//!          blocks; dead doc references from code comments; `relink` and `stale`.
//! OWNERS: @tools-team
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 20 integration tests.
//!
//! TEST_SCOPE:
//!   - lint: a canonical doc passes, every doc rule rejects its violation
//!   - gen: syscalls, capabilities, markers and the docs index render from their sources
//!   - relink / stale: references follow a moved doc; a later commit on a source lists its doc
//!
//! ADR: docs/rfcs/RFC-0115-planning-as-data-milestones-sprints-phases-one-ledger-cli.md

use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::{tempdir, TempDir};

const SUBSYSTEMS: &str = "---\nschema: 1\nsubsystems:\n  - slug: storage\n    name: Storage\n    tier: production-floor\n    release: consumer\n---\n\n# Subsystems\n";

const TASK: &str = "---\nschema: 1\nid: TASK-0001\ntitle: \"A store\"\nstatus: draft\nowner: [\"@storage\"]\ncreated: 2026-10-01\nlinks:\n  - label: Doc\n    target: docs/storage/old.md\n---\n\n# TASK-0001\n\nSee `docs/storage/old.md`.\n";

fn doc(front_extra: &str, body: &str) -> String {
    format!("---\nschema: 1\ntitle: \"The store\"\nrole: guide\nsubsystem: storage\nsources: [userspace/store]\n{front_extra}verified: 2026-11-01\n---\n\n<!-- Copyright 2026 Open Nexus OS Contributors -->\n\n# The store\n\n{body}\n")
}

fn repo(doc_text: &str) -> TempDir {
    let dir = tempdir().expect("tempdir");
    let p = dir.path();
    for d in ["tasks", "docs/rfcs", "docs/adr", "docs/storage", "userspace/store/src"] {
        fs::create_dir_all(p.join(d)).expect("dir");
    }
    fs::write(p.join("tasks/SUBSYSTEMS.md"), SUBSYSTEMS).expect("subsystems");
    fs::write(p.join("docs/adr/0007-a-decision.md"), "# ADR-0007\n").expect("adr");
    fs::write(p.join("userspace/store/src/lib.rs"), "//! CONTEXT: a store\n").expect("src");
    fs::write(p.join("docs/storage/README.md"), doc_text).expect("doc");
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

fn lint_ok(root: &Path) {
    let o = nx(root, &["lint"]);
    assert!(o.status.success(), "lint should pass: {}", text(&o));
}

fn lint_rejects(root: &Path, needle: &str) {
    let o = nx(root, &["lint"]);
    assert_eq!(o.status.code(), Some(3), "lint should fail: {}", text(&o));
    assert!(text(&o).contains(needle), "expected `{needle}` in: {}", text(&o));
}

#[test]
fn canonical_doc_passes() {
    let d = repo(&doc("contracts: [ADR-0007]\n", "The code is `userspace/store/src/lib.rs`."));
    lint_ok(d.path());
    let o = nx(d.path(), &["show", "docs/storage/README"]);
    assert!(
        text(&o).contains("docs/storage/README · guide · storage · verified 2026-11-01"),
        "{}",
        text(&o)
    );
}

#[test]
fn test_reject_doc_without_frontmatter() {
    let d = repo("# The store\n");
    lint_rejects(d.path(), "does not start with `---`");
}

#[test]
fn test_reject_missing_source() {
    let d = repo(&doc("", "x").replace("userspace/store]", "userspace/gone]"));
    lint_rejects(d.path(), "`sources` names userspace/gone, which does not exist");
}

#[test]
fn test_reject_guide_without_sources() {
    let d = repo(&doc("", "x").replace("sources: [userspace/store]\n", ""));
    lint_rejects(d.path(), "a guide names the code it describes in `sources`");
}

#[test]
fn test_reject_unknown_contract() {
    let d = repo(&doc("contracts: [ADR-0099]\n", "x"));
    lint_rejects(d.path(), "`contracts` names ADR-0099, which has no file");
}

#[test]
fn test_reject_dead_markdown_link() {
    let d = repo(&doc("", "Read [the old page](../storage/old.md#top)."));
    lint_rejects(d.path(), "docs/storage/old.md does not exist");
}

#[test]
fn test_reject_dead_backticked_path() {
    let d = repo(&doc("", "The loader is `userspace/store/src/loader.rs:12`."));
    lint_rejects(d.path(), "userspace/store/src/loader.rs does not exist");
}

#[test]
fn placeholders_and_fences_are_not_paths() {
    let d = repo(&doc(
        "",
        "Each `source/services/<name>d/` adapts.\n\n```\ncat userspace/none.rs\n```",
    ));
    lint_ok(d.path());
}

#[test]
fn test_reject_task_status_in_a_doc() {
    let d = repo(&doc("", "| TASK-0001 | ✅ |"));
    lint_rejects(d.path(), "is task status; status lives in the ledger");
}

#[test]
fn test_reject_over_the_word_budget() {
    let d = repo(&doc("", &"word ".repeat(1300)));
    lint_rejects(d.path(), "over the guide budget of 1200");
}

#[test]
fn test_reject_unknown_generated_block() {
    let d = repo(&doc("", "<!-- gen:weather -->\n<!-- /gen:weather -->"));
    lint_rejects(d.path(), "unknown generated block `weather`");
}

#[test]
fn test_reject_stale_generated_block_and_fix_renders_it() {
    let d = repo(&doc("", "<!-- gen:syscalls -->\nold\n<!-- /gen:syscalls -->"));
    let p = d.path();
    fs::create_dir_all(p.join("source/kernel/neuron/src/syscall")).expect("dir");
    fs::write(
        p.join("source/kernel/neuron/src/syscall/mod.rs"),
        "/// Gives up the CPU. More text.\npub const SYSCALL_YIELD: usize = 0;\npub const SYSCALL_SEND: usize = 2;\n",
    )
    .expect("syscalls");
    lint_rejects(p, "a generated block is stale");
    let o = nx(p, &["lint", "--fix"]);
    assert!(o.status.success(), "{}", text(&o));
    let body = fs::read_to_string(p.join("docs/storage/README.md")).expect("read");
    assert!(body.contains("| 0 | `yield` | Gives up the CPU. |\n| 2 | `send` |  |\n"), "{body}");
}

#[test]
fn capabilities_and_markers_render_from_their_sources() {
    let d = repo(&doc("", "<!-- gen:capabilities -->\n<!-- /gen:capabilities -->\n\n<!-- gen:markers -->\n<!-- /gen:markers -->"));
    let p = d.path();
    let m = p.join("source/apps/selftest-client/proof-manifest/markers");
    fs::create_dir_all(&m).expect("dir");
    fs::create_dir_all(p.join("policies")).expect("dir");
    fs::write(
        p.join("policies/base.toml"),
        "[allow]\nb = [\"ipc.core\"]\na = [\"ipc.core\", \"fs.read\"]\n",
    )
    .expect("policy");
    fs::write(m.parent().expect("parent").join("phases.toml"), "[phase.bringup]\norder = 1\n")
        .expect("phases");
    fs::write(
        m.join("bringup.toml"),
        "[marker.\"init: up\"]\nphase = \"bringup\"\n[marker.\"x: ok\"]\nphase = \"bringup\"\n",
    )
    .expect("markers");
    assert!(nx(p, &["lint", "--fix"]).status.success());
    let body = fs::read_to_string(p.join("docs/storage/README.md")).expect("read");
    assert!(body.contains("| `fs.read` | a |\n| `ipc.core` | a, b |\n"), "{body}");
    assert!(
        body.contains("| 1 | `bringup` | 2 | bringup.toml |\n\n2 markers in 1 files.\n"),
        "{body}"
    );
}

#[test]
fn the_docs_index_lists_every_doc_from_its_frontmatter() {
    let d = repo(&doc("", "x"));
    let p = d.path();
    let index = "---\nschema: 1\ntitle: \"Documentation index\"\nrole: index\nverified: 2026-11-01\n---\n\n<!-- gen:docs-index -->\n<!-- /gen:docs-index -->\n";
    fs::write(p.join("docs/README.md"), index).expect("index");
    assert!(nx(p, &["lint", "--fix"]).status.success());
    let body = fs::read_to_string(p.join("docs/README.md")).expect("read");
    assert!(body.contains("| [README.md](storage/README.md) | guide | The store |"), "{body}");
}

#[test]
fn test_reject_dead_doc_named_from_a_code_comment() {
    let d = repo(&doc("", "x"));
    let p = d.path();
    fs::write(
        p.join("userspace/store/src/lib.rs"),
        "//! ADR: docs/adr/0008-gone.md\nconst P: &str = \"docs/stubs/README.md\";\n",
    )
    .expect("src");
    lint_rejects(p, "userspace/store/src/lib.rs:1: names docs/adr/0008-gone.md");
    let o = nx(p, &["lint"]);
    assert!(!text(&o).contains("docs/stubs/README.md"), "a string literal is code: {}", text(&o));
}

#[test]
fn a_planning_document_naming_a_dead_doc_is_a_note() {
    let d = repo(&doc("", "x"));
    let p = d.path();
    fs::write(p.join("tasks/TASK-0001-a-store.md"), TASK).expect("task");
    lint_ok(p);
    let o = nx(p, &["lint", "--notes"]);
    assert!(
        text(&o).contains("note  tasks/TASK-0001-a-store.md: names docs/storage/old.md"),
        "{}",
        text(&o)
    );
}

#[test]
fn relink_follows_a_moved_doc() {
    let d = repo(&doc("", "Older: [old](old.md)."));
    let p = d.path();
    fs::write(p.join("tasks/TASK-0001-a-store.md"), TASK).expect("task");
    fs::create_dir_all(p.join("docs/vault")).expect("dir");
    let moved = doc("", "Back to [the store](README.md).").replace("The store", "Old store");
    fs::write(p.join("docs/vault/new.md"), moved).expect("moved");
    fs::write(p.join("userspace/store/src/lib.rs"), "//! See docs/storage/old.md\n").expect("src");
    let o = nx(p, &["relink", "docs/storage/old.md", "docs/vault/new.md"]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(
        text(&o).contains("still names it (edit by hand): userspace/store/src/lib.rs"),
        "{}",
        text(&o)
    );
    let task = fs::read_to_string(p.join("tasks/TASK-0001-a-store.md")).expect("task");
    assert!(
        task.contains("target: docs/vault/new.md") && task.contains("See `docs/vault/new.md`."),
        "{task}"
    );
    let readme = fs::read_to_string(p.join("docs/storage/README.md")).expect("readme");
    assert!(readme.contains("[old](../vault/new.md)"), "{readme}");
    let new = fs::read_to_string(p.join("docs/vault/new.md")).expect("new");
    assert!(new.contains("[the store](../storage/README.md)"), "{new}");
}

#[test]
fn relink_to_nothing_drops_the_link_records() {
    let d = repo(&doc("", "x"));
    let p = d.path();
    fs::write(p.join("tasks/TASK-0001-a-store.md"), TASK).expect("task");
    let o = nx(p, &["relink", "docs/storage/old.md", "-"]);
    assert!(o.status.success(), "{}", text(&o));
    let task = fs::read_to_string(p.join("tasks/TASK-0001-a-store.md")).expect("task");
    assert!(!task.contains("target: docs/storage/old.md"), "{task}");
    assert!(
        text(&o).contains("still names it (edit by hand): tasks/TASK-0001-a-store.md"),
        "{}",
        text(&o)
    );
}

#[test]
fn stale_lists_a_doc_whose_source_changed_after_it_was_verified() {
    let d = repo(&doc("", "x"));
    let p = d.path();
    let git = |args: &[&str], date: &str| {
        let o = Command::new("git")
            .arg("-C")
            .arg(p)
            .args(args)
            .env("GIT_AUTHOR_DATE", date)
            .env("GIT_COMMITTER_DATE", date)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@t")
            .output()
            .expect("git");
        assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
    };
    git(&["init", "-q"], "2026-10-30T10:00:00");
    git(&["add", "."], "2026-10-30T10:00:00");
    git(&["commit", "-qm", "the store"], "2026-10-30T10:00:00");
    let quiet = nx(p, &["stale"]);
    assert!(text(&quiet).contains("stale: 0 doc(s)"), "{}", text(&quiet));
    fs::write(p.join("userspace/store/src/lib.rs"), "//! CONTEXT: a store, changed\n")
        .expect("src");
    git(&["commit", "-qam", "store: a new path"], "2026-11-02T10:00:00");
    let o = nx(p, &["stale"]);
    assert!(
        text(&o).contains("docs/storage/README.md · verified 2026-11-01 · 1 commit(s)"),
        "{}",
        text(&o)
    );
    assert!(text(&o).contains("store: a new path"), "{}", text(&o));
    assert!(text(&nx(p, &["brief"])).contains("Docs: 1 stale"));
}

#[test]
fn relink_into_a_track_drops_the_tracks_link_to_itself() {
    let d = repo(&doc("", "x"));
    let p = d.path();
    let track = "---\nschema: 1\nid: TRACK-STORE\ntitle: \"Store track\"\nstatus: living\nowner: [\"@storage\"]\ncreated: 2026-10-01\nlinks:\n  - label: Notes\n    target: docs/storage/old.md\n---\n\n# Store track\n";
    fs::write(p.join("tasks/TRACK-STORE.md"), track).expect("track");
    fs::write(p.join("tasks/TASK-0001-a-store.md"), TASK).expect("task");
    let o = nx(p, &["relink", "docs/storage/old.md", "tasks/TRACK-STORE.md"]);
    assert!(o.status.success(), "{}", text(&o));
    let t = fs::read_to_string(p.join("tasks/TRACK-STORE.md")).expect("track");
    assert!(!t.contains("target:"), "a track never links itself: {t}");
    let task = fs::read_to_string(p.join("tasks/TASK-0001-a-store.md")).expect("task");
    assert!(task.contains("target: tasks/TRACK-STORE.md"), "{task}");
}
