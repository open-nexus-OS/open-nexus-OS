// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The app structure gate (TASK-0357): every DSL app under `userspace/apps/` follows the
//! project layout of `docs/dev/dsl/project-layout.md`, and what does not follow it yet is listed
//! in `app_structure.baseline` — a list that may only shrink.
//!
//! The rules, per app (a folder with `ui/`):
//! - `README.md` exists and carries the template's sections ([`APP_SECTIONS`] for an app,
//!   [`LIBRARY_SECTIONS`] for a library) — what it is, how it is wired, how it is proven;
//! - an app (it has `ui/pages/`) carries `i18n/de.json`, and a `Routes.nx` once it has more than
//!   one page; its components live in feature folders (`ui/components/<feature>/`), never flat;
//!   a library (no pages — window-kit) is components only, flat, as the resolver reads it;
//! - files stay small, in logical lines (neither blank nor a `//` comment): a page at most
//!   [`PAGE_MAX`] (a page composes, it does not build), a component at most [`COMPONENT_MAX`], a
//!   store at most [`STORE_MAX`] (split by domain beyond that);
//! - a conformance test compiles it (`tests/dsl_apps_conformance/tests/*.rs` names it);
//! - no `.nx` file is a byte copy of another app's (one source per piece).
//!
//! A baseline line is `<rule> <path> [<recorded value>]`. A violation not in the baseline fails;
//! a size over its recorded value fails (it grew); an entry that no longer violates fails too —
//! delete it, the baseline only shrinks.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// The most logical lines a page may have.
const PAGE_MAX: usize = 100;
/// The most logical lines a component may have.
const COMPONENT_MAX: usize = 150;
/// The most logical lines a store (composable) may have.
const STORE_MAX: usize = 200;
/// The `## ` sections an app's README carries (the template: `userspace/apps/calculator/README.md`).
const APP_SECTIONS: [&str; 6] =
    ["Routes", "Stores and events", "Services", "Window", "Structure", "Tests"];
/// The `## ` sections a library's README carries (`userspace/apps/window-kit/README.md`).
const LIBRARY_SECTIONS: [&str; 2] = ["Components", "Tests"];

/// The template sections `readme` lacks, in template order.
fn missing_sections(readme: &str, is_app: bool) -> Vec<&'static str> {
    let headings: BTreeSet<&str> =
        readme.lines().filter_map(|l| l.strip_prefix("## ")).map(str::trim).collect();
    let wanted: &[&'static str] = if is_app { &APP_SECTIONS } else { &LIBRARY_SECTIONS };
    wanted.iter().copied().filter(|s| !headings.contains(s)).collect()
}

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("repo root")
}

fn rel(path: &Path) -> String {
    path.strip_prefix(repo()).unwrap_or(path).to_string_lossy().into_owned()
}

fn nx_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = read.filter_map(|e| e.ok().map(|e| e.path())).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            nx_files(&path, out);
        } else if path.extension().is_some_and(|x| x == "nx") {
            out.push(path);
        }
    }
}

fn logical_lines(path: &Path) -> usize {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("//"))
        .count()
}

/// Every violation of the rules, as `rule path` → the measured value (0 when it has none).
fn violations() -> BTreeMap<(String, String), usize> {
    let root = repo();
    let tests_dir = root.join("tests/dsl_apps_conformance/tests");
    let tests: String = std::fs::read_dir(&tests_dir)
        .expect("tests dir")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "rs"))
        .filter(|p| !p.ends_with("app_structure.rs"))
        .map(|p| std::fs::read_to_string(p).unwrap_or_default())
        .collect();
    let mut out = BTreeMap::new();
    let mut by_content: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut apps: Vec<PathBuf> = std::fs::read_dir(root.join("userspace/apps"))
        .expect("userspace/apps")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.join("ui").is_dir())
        .collect();
    apps.sort();
    for app in apps {
        let name = app.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_owned();
        let app_rel = rel(&app);
        let ui = app.join("ui");
        let is_app = ui.join("pages").is_dir();
        match std::fs::read_to_string(app.join("README.md")) {
            Err(_) => {
                out.insert(("readme".into(), app_rel.clone()), 0);
            }
            Ok(readme) => {
                let missing = missing_sections(&readme, is_app).len();
                if missing > 0 {
                    out.insert(("readme-sections".into(), app_rel.clone()), missing);
                }
            }
        }
        if !tests.contains(&format!("\"{name}\"")) {
            out.insert(("untested".into(), app_rel.clone()), 0);
        }
        if is_app {
            if !app.join("i18n/de.json").is_file() {
                out.insert(("i18n".into(), app_rel.clone()), 0);
            }
            let pages = std::fs::read_dir(ui.join("pages"))
                .map(|r| {
                    r.filter_map(|e| e.ok())
                        .filter(|e| e.file_name().to_string_lossy().ends_with("Page.nx"))
                        .count()
                })
                .unwrap_or(0);
            if pages > 1 && !ui.join("pages/Routes.nx").is_file() {
                out.insert(("routes".into(), app_rel.clone()), 0);
            }
            let flat = std::fs::read_dir(ui.join("components"))
                .map(|r| {
                    r.filter_map(|e| e.ok())
                        .filter(|e| e.path().extension().is_some_and(|x| x == "nx"))
                        .count()
                })
                .unwrap_or(0);
            if flat > 0 {
                out.insert(("flat-components".into(), app_rel.clone()), flat);
            }
        }
        let mut files = Vec::new();
        nx_files(&ui, &mut files);
        for file in files {
            let file_rel = rel(&file);
            let kind = file
                .strip_prefix(&ui)
                .ok()
                .and_then(|p| p.iter().next())
                .map(|k| k.to_string_lossy().into_owned());
            let limit = match kind.as_deref() {
                Some("pages") | Some("platform") => PAGE_MAX,
                Some("components") => COMPONENT_MAX,
                Some("composables") => STORE_MAX,
                _ => usize::MAX,
            };
            let n = logical_lines(&file);
            if n > limit {
                out.insert(("size".into(), file_rel.clone()), n);
            }
            let content = std::fs::read_to_string(&file).unwrap_or_default();
            by_content.entry(content).or_default().push(file_rel);
        }
    }
    for copies in by_content.values().filter(|c| c.len() > 1) {
        // Every copy after the first is a duplicate of the first.
        for copy in &copies[1..] {
            out.insert(("duplicate".into(), copy.clone()), 0);
        }
    }
    out
}

/// The baseline: `rule path` → the recorded value (0 when it has none).
fn baseline() -> BTreeMap<(String, String), usize> {
    let text = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("app_structure.baseline"),
    )
    .expect("app_structure.baseline");
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let mut parts = l.split_whitespace();
            let rule = parts.next().unwrap_or_default().to_owned();
            let path = parts.next().unwrap_or_default().to_owned();
            let value = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
            ((rule, path), value)
        })
        .collect()
}

#[test]
fn every_app_follows_the_project_layout_or_is_in_the_shrinking_baseline() {
    let now = violations();
    let base = baseline();
    let mut errors = Vec::new();
    for ((rule, path), value) in &now {
        match base.get(&(rule.clone(), path.clone())) {
            None => errors.push(format!("new violation: {rule} {path} {value}")),
            Some(recorded) if rule == "size" && value > recorded => {
                errors.push(format!("grew: {rule} {path} {value} (baseline {recorded})"))
            }
            Some(_) => {}
        }
    }
    let live: BTreeSet<_> = now.keys().collect();
    for key in base.keys().filter(|k| !live.contains(k)) {
        errors.push(format!("resolved, delete the baseline entry: {} {}", key.0, key.1));
    }
    let listing: Vec<String> = now
        .iter()
        .map(
            |((rule, path), v)| {
                if *v > 0 {
                    format!("{rule} {path} {v}")
                } else {
                    format!("{rule} {path}")
                }
            },
        )
        .collect();
    assert!(
        errors.is_empty(),
        "app structure (docs/dev/dsl/project-layout.md):\n  {}\n\nthe violations today, in baseline form:\n{}",
        errors.join("\n  "),
        listing.join("\n")
    );
}

/// A size limit is a limit: a file one logical line over it is a violation, a file at it is not.
#[test]
fn test_reject_a_file_over_its_limit_and_accept_one_at_it() {
    let dir = std::env::temp_dir().join(format!("app-structure-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let at = dir.join("at.nx");
    let over = dir.join("over.nx");
    let body =
        |n: usize| (0..n).map(|i| format!("// note {i}\nText(\"{i}\")\n\n")).collect::<String>();
    std::fs::write(&at, body(PAGE_MAX)).expect("write");
    std::fs::write(&over, body(PAGE_MAX + 1)).expect("write");
    assert_eq!(logical_lines(&at), PAGE_MAX, "comments and blank lines do not count");
    assert!(logical_lines(&over) > PAGE_MAX);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A README section is a `## ` heading with the template's name: a README missing one (or
/// spelling it differently) is a violation; one with every section, in any order, is not.
#[test]
fn test_reject_a_readme_without_its_sections() {
    let full =
        "# x\n\n## Tests\n## Routes\n## Stores and events\n## Services\n## Window\n## Structure\n";
    assert!(missing_sections(full, true).is_empty(), "every section, any order");
    let short = "# x\n\n## Routes\n## Store and events\n## Tests\n";
    assert_eq!(
        missing_sections(short, true),
        vec!["Stores and events", "Services", "Window", "Structure"],
        "a misspelt and three missing sections"
    );
    assert!(missing_sections("## Components\n## Tests\n", false).is_empty(), "a library");
    assert_eq!(missing_sections("## Components\n", false), vec!["Tests"]);
}
