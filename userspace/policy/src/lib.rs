//! CONTEXT: Userspace policy domain library
//! INTENT: Policy document parsing, capability checking, TOML loading
//! IDL (target): check(required_caps, subject), loadDir(path)
//! DEPS: serde, toml, std::fs (file operations)
//! READINESS: Library ready; no service dependencies
//! TESTS: Allow/deny checks; directory loading; canonicalization
// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Clone, Default)]
pub struct PolicyDoc {
    allow: BTreeMap<String, BTreeSet<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Denied {
    pub missing: Vec<String>,
}

impl fmt::Display for Denied {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "missing capabilities: {}", self.missing.join(", "))
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("policy directory not found: {0}")]
    MissingDir(PathBuf),
    #[error("failed to read policy file {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse policy file {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
}

impl PolicyDoc {
    /// Returns the number of subjects with explicit entries in the policy.
    pub fn subject_count(&self) -> usize {
        self.allow.len()
    }

    /// Returns the total number of capabilities across all subjects.
    pub fn capability_count(&self) -> usize {
        self.allow.values().map(|caps| caps.len()).sum()
    }

    pub fn check(&self, required: &[&str], subject: &str) -> Result<(), Denied> {
        let subject_key = canonical(subject);
        let allowed_caps = self.allow.get(&subject_key);
        let mut missing = Vec::new();
        for cap in required.iter().map(|cap| canonical(cap)) {
            let is_allowed = allowed_caps.map(|caps| caps.contains(&cap)).unwrap_or(false);
            if !is_allowed {
                missing.push(cap);
            }
        }
        if missing.is_empty() {
            Ok(())
        } else {
            Err(Denied { missing })
        }
    }

    pub fn load_dir(dir: &Path) -> Result<Self, Error> {
        if !dir.exists() {
            return Err(Error::MissingDir(dir.to_path_buf()));
        }
        let mut entries = Vec::new();
        for entry in
            fs::read_dir(dir).map_err(|source| Error::Read { path: dir.to_path_buf(), source })?
        {
            let entry = entry.map_err(|source| Error::Read { path: dir.to_path_buf(), source })?;
            entries.push(entry.path());
        }
        entries.sort();

        let mut doc = PolicyDoc::default();
        for path in entries {
            if !path.is_file() || path.extension().and_then(|s| s.to_str()) != Some("toml") {
                continue;
            }
            let data = fs::read_to_string(&path)
                .map_err(|source| Error::Read { path: path.clone(), source })?;
            let parsed: RawPolicy = toml::from_str(&data)
                .map_err(|source| Error::Parse { path: path.clone(), source })?;
            doc.merge(parsed);
        }
        Ok(doc)
    }

    fn merge(&mut self, raw: RawPolicy) {
        for (service, caps) in raw.allow {
            let service_key = canonical(&service);
            let mut set = BTreeSet::new();
            for cap in caps {
                set.insert(canonical(&cap));
            }
            self.allow.insert(service_key, set);
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawPolicy {
    #[serde(default)]
    allow: BTreeMap<String, Vec<String>>,
}

fn canonical(input: &str) -> String {
    input.trim().to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn check_allows_and_denies() {
        let mut doc = PolicyDoc::default();
        doc.merge(RawPolicy {
            allow: BTreeMap::from([(
                "Example".to_string(),
                vec!["IPC.Core".to_string(), "time.read".to_string()],
            )]),
        });

        assert!(doc.check(&["ipc.core"], "EXAMPLE").is_ok());
        let err = doc.check(&["fs.write"], "example").unwrap_err();
        assert_eq!(err.missing, vec!["fs.write".to_string()]);
    }

    #[test]
    fn load_dir_merges_files_with_override() {
        let temp = TempDir::new().unwrap();
        let path = temp.path();
        let mut file_a = std::fs::File::create(path.join("a.toml")).unwrap();
        writeln!(file_a, "[allow]\nfoo = ['cap.a']\nbar = ['cap.b']").unwrap();
        let mut file_b = std::fs::File::create(path.join("b.toml")).unwrap();
        writeln!(file_b, "[allow]\nbar = ['cap.c']").unwrap();

        let doc = PolicyDoc::load_dir(path).unwrap();
        assert!(doc.check(&["cap.a"], "foo").is_ok());
        let err = doc.check(&["cap.b"], "bar").unwrap_err();
        assert_eq!(err.missing, vec!["cap.b".to_string()]);
        assert!(doc.check(&["cap.c"], "bar").is_ok());
    }
}
