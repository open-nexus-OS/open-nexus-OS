// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Finds the repository, reads every planning document (tasks, tracks, RFCs, milestones,
//!          sprints, the subsystem list) and writes one back atomically, body untouched.
//! OWNERS: @tools-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: tools/nx/tests/ledger_cli.rs
//! ADR: docs/rfcs/RFC-0115-planning-as-data-milestones-sprints-phases-one-ledger-cli.md

use super::model::{classify, Kind};
use super::yaml::{self, Entries};
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) struct Doc {
    pub(crate) kind: Kind,
    pub(crate) id: String,
    /// Repo-relative, `/`-separated.
    pub(crate) rel: String,
    pub(crate) entries: Entries,
    pub(crate) body: String,
    /// The file as read, to tell whether rendering changes it.
    pub(crate) raw: String,
}

impl Doc {
    pub(crate) fn text(&self, key: &str) -> Option<&str> {
        yaml::get_text(&self.entries, key)
    }

    pub(crate) fn status(&self) -> &str {
        self.text("status").unwrap_or("")
    }

    pub(crate) fn rendered(&self) -> String {
        yaml::render_file(&self.entries, &self.body)
    }
}

pub(crate) struct LoadError {
    pub(crate) rel: String,
    pub(crate) message: String,
}

pub(crate) struct Repo {
    pub(crate) root: PathBuf,
    pub(crate) docs: Vec<Doc>,
    pub(crate) errors: Vec<LoadError>,
}

impl Repo {
    pub(crate) fn get(&self, id: &str) -> Option<&Doc> {
        self.docs.iter().find(|d| d.id == id)
    }

    pub(crate) fn of(&self, kind: Kind) -> impl Iterator<Item = &Doc> {
        self.docs.iter().filter(move |d| d.kind == kind)
    }
}

/// The repository root: `NX_LEDGER_ROOT`, else the first ancestor of the working directory that
/// holds both `tasks/` and `docs/rfcs/`.
pub(crate) fn find_root() -> Result<PathBuf, String> {
    if let Some(root) = std::env::var_os("NX_LEDGER_ROOT") {
        return Ok(PathBuf::from(root));
    }
    let cwd = std::env::current_dir().map_err(|e| format!("no working directory: {e}"))?;
    cwd.ancestors()
        .find(|d| d.join("tasks").is_dir() && d.join("docs/rfcs").is_dir())
        .map(Path::to_path_buf)
        .ok_or_else(|| String::from("not inside a repository with tasks/ and docs/rfcs/"))
}

pub(crate) fn load(root: &Path) -> Repo {
    let mut docs = Vec::new();
    let mut errors = Vec::new();
    for dir in ["tasks", "tasks/milestones", "tasks/sprints", "docs/rfcs"] {
        let Ok(read) = fs::read_dir(root.join(dir)) else { continue };
        let mut names: Vec<String> =
            read.filter_map(|e| e.ok()).filter_map(|e| e.file_name().into_string().ok()).collect();
        names.sort();
        for name in names {
            let rel = format!("{dir}/{name}");
            let Some((kind, id)) = classify(&rel) else { continue };
            match read_doc(root, &rel, kind, id) {
                Ok(doc) => docs.push(doc),
                Err(message) => errors.push(LoadError { rel, message }),
            }
        }
    }
    Repo { root: root.to_path_buf(), docs, errors }
}

fn read_doc(root: &Path, rel: &str, kind: Kind, id: String) -> Result<Doc, String> {
    let raw = fs::read_to_string(root.join(rel)).map_err(|e| format!("unreadable: {e}"))?;
    let split = yaml::split(&raw)?;
    let entries = yaml::parse(split.front)?;
    let body = split.body.to_string();
    Ok(Doc { kind, id, rel: rel.to_string(), entries, body, raw })
}

/// Write a document back (temp file + rename). The body is written exactly as read.
pub(crate) fn write(root: &Path, doc: &Doc) -> Result<(), String> {
    write_text(root, &doc.rel, &doc.rendered())
}

pub(crate) fn write_text(root: &Path, rel: &str, text: &str) -> Result<(), String> {
    let path = root.join(rel);
    let tmp = path.with_extension("md.nx-tmp");
    fs::write(&tmp, text).map_err(|e| format!("{rel}: {e}"))?;
    fs::rename(&tmp, &path).map_err(|e| format!("{rel}: {e}"))
}
