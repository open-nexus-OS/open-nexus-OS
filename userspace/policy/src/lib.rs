// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Policy domain library for capability-based access control
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 18 unit tests covering policy tree, evaluator, manifest, and reject contracts.
//!
//! PUBLIC API:
//!   - Policy: Capability-based access control
//!   - PolicyError: Policy error types
//!
//! DEPENDENCIES:
//!   - serde: Serialization/deserialization
//!   - toml: TOML file parsing
//!   - std::collections: Ordered collections
//!   - thiserror: Error types
//!
//! ADR: docs/adr/0014-policy-architecture.md

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};
use thiserror::Error;

pub const MAX_POLICY_FILE_BYTES: usize = 64 * 1024;
pub const MAX_POLICY_INCLUDES: usize = 128;
pub const DEFAULT_EXPLAIN_TRACE_LIMIT: usize = 32;

#[derive(Debug, Clone, Default, Serialize)]
pub struct PolicyDoc {
    allow: BTreeMap<String, BTreeSet<String>>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    abi_profile: BTreeMap<String, AbiProfile>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct AbiProfile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    statefs_put_allow_prefix: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    net_bind_min_port: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyVersion(String);

impl PolicyVersion {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PolicyVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone)]
pub struct PolicyTree {
    version: PolicyVersion,
    policy: PolicyDoc,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyManifest {
    pub version: u32,
    pub tree_sha256: String,
    pub generated_at_ns: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PolicyMode {
    Enforce,
    DryRun,
    Learn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasonCode {
    ExplicitAllow,
    MissingCapabilities,
}

impl ReasonCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ExplicitAllow => "explicit_allow",
            Self::MissingCapabilities => "missing_capabilities",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExplainStep {
    pub subject: String,
    pub capability: String,
    pub matched: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Decision {
    pub allow: bool,
    pub reason_code: ReasonCode,
    pub trace: Vec<ExplainStep>,
    pub mode: PolicyMode,
    pub would_deny: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct LearnObservation {
    pub subject: String,
    pub capability: String,
    pub reason_code: String,
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
    #[error("policy root missing: {0}")]
    MissingRoot(PathBuf),
    #[error("policy root is ambiguous: policies={policies} recipes={recipes}")]
    AmbiguousRoot { policies: String, recipes: String },
    #[error("invalid policy root {path}: {message}")]
    InvalidRoot { path: PathBuf, message: String },
    #[error("policy include escapes root: {path}")]
    IncludeTraversal { path: PathBuf },
    #[error("policy file too large: {path} len={len} max={max}")]
    Oversize {
        path: PathBuf,
        len: usize,
        max: usize,
    },
    #[error("unknown policy section in {path}: {section}")]
    UnknownSection { path: PathBuf, section: String },
    #[error("failed to canonicalize policy tree: {0}")]
    Canonical(String),
    #[error("policy explain trace over budget: required={required} max={max}")]
    TraceBudgetExceeded { required: usize, max: usize },
    #[error("policy manifest mismatch: expected={expected} actual={actual}")]
    ManifestMismatch { expected: String, actual: String },
}

impl Error {
    pub fn code(&self) -> &'static str {
        match self {
            Self::MissingDir(_) | Self::MissingRoot(_) => "policy.missing_root",
            Self::Read { .. } => "policy.read",
            Self::Parse { .. } => "policy.parse",
            Self::AmbiguousRoot { .. } => "policy.ambiguous_root",
            Self::InvalidRoot { .. } => "policy.invalid_root",
            Self::IncludeTraversal { .. } => "policy.include_traversal",
            Self::Oversize { .. } => "policy.oversize",
            Self::UnknownSection { .. } => "policy.unknown_section",
            Self::Canonical(_) => "policy.canonical",
            Self::TraceBudgetExceeded { .. } => "policy.explain_trace_over_budget",
            Self::ManifestMismatch { .. } => "policy.manifest_mismatch",
        }
    }
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
            let is_allowed = allowed_caps
                .map(|caps| caps.contains(&cap))
                .unwrap_or(false);
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

    pub fn evaluate(
        &self,
        required: &[&str],
        subject: &str,
        mode: PolicyMode,
    ) -> Result<Decision, Error> {
        self.evaluate_with_trace_limit(required, subject, mode, DEFAULT_EXPLAIN_TRACE_LIMIT)
    }

    pub fn evaluate_with_trace_limit(
        &self,
        required: &[&str],
        subject: &str,
        mode: PolicyMode,
        max_trace_steps: usize,
    ) -> Result<Decision, Error> {
        if required.len() > max_trace_steps {
            return Err(Error::TraceBudgetExceeded {
                required: required.len(),
                max: max_trace_steps,
            });
        }

        let subject_key = canonical(subject);
        let allowed_caps = self.allow.get(&subject_key);
        let mut missing = Vec::new();
        let mut trace = Vec::with_capacity(required.len());

        for cap in required.iter().map(|cap| canonical(cap)) {
            let matched = allowed_caps
                .map(|caps| caps.contains(&cap))
                .unwrap_or(false);
            if !matched {
                missing.push(cap.clone());
            }
            trace.push(ExplainStep {
                subject: subject_key.clone(),
                capability: cap,
                matched,
            });
        }

        let allow = missing.is_empty();
        let reason_code = if allow {
            ReasonCode::ExplicitAllow
        } else {
            ReasonCode::MissingCapabilities
        };
        Ok(Decision {
            allow,
            reason_code,
            trace,
            mode,
            would_deny: !allow,
        })
    }

    pub fn learn_observations(decision: &Decision) -> Vec<LearnObservation> {
        if decision.mode != PolicyMode::Learn || !decision.would_deny {
            return Vec::new();
        }
        decision
            .trace
            .iter()
            .filter(|step| !step.matched)
            .map(|step| LearnObservation {
                subject: step.subject.clone(),
                capability: step.capability.clone(),
                reason_code: decision.reason_code.as_str().to_string(),
            })
            .collect()
    }

    pub fn normalize_learn_log<I>(observations: I) -> Vec<LearnObservation>
    where
        I: IntoIterator<Item = LearnObservation>,
    {
        observations
            .into_iter()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    pub fn load_dir(dir: &Path) -> Result<Self, Error> {
        if !dir.exists() {
            return Err(Error::MissingDir(dir.to_path_buf()));
        }
        let mut entries = Vec::new();
        for entry in fs::read_dir(dir).map_err(|source| Error::Read {
            path: dir.to_path_buf(),
            source,
        })? {
            let entry = entry.map_err(|source| Error::Read {
                path: dir.to_path_buf(),
                source,
            })?;
            entries.push(entry.path());
        }
        entries.sort();

        let mut doc = PolicyDoc::default();
        for path in entries {
            if !path.is_file() || path.extension().and_then(|s| s.to_str()) != Some("toml") {
                continue;
            }
            let data = fs::read_to_string(&path).map_err(|source| Error::Read {
                path: path.clone(),
                source,
            })?;
            let parsed: RawPolicy = toml::from_str(&data).map_err(|source| Error::Parse {
                path: path.clone(),
                source,
            })?;
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
        for (service, mut profile) in raw.abi_profile {
            let service_key = canonical(&service);
            profile.statefs_put_allow_prefix = profile
                .statefs_put_allow_prefix
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(ToString::to_string);
            self.abi_profile.insert(service_key, profile);
        }
    }
}

impl PolicyTree {
    pub fn load_single_authority(repo_root: &Path) -> Result<Self, Error> {
        let policies = repo_root.join("policies");
        let recipes = repo_root.join("recipes/policy");
        match (policies.exists(), has_toml_files(&recipes)?) {
            (true, true) => Err(Error::AmbiguousRoot {
                policies: policies.display().to_string(),
                recipes: recipes.display().to_string(),
            }),
            (true, false) => Self::load_root(&policies),
            (false, true) => Self::load_legacy_recipes(&recipes),
            (false, false) => Err(Error::MissingRoot(policies)),
        }
    }

    pub fn load_root(root: &Path) -> Result<Self, Error> {
        let root_file = root.join("nexus.policy.toml");
        if !root_file.exists() {
            return Err(Error::MissingRoot(root_file));
        }
        let root_raw = read_bounded(&root_file)?;
        let root_doc: RawPolicyRoot = toml::from_str(&root_raw).map_err(|source| Error::Parse {
            path: root_file.clone(),
            source,
        })?;
        if root_doc.version != 1 {
            return Err(Error::InvalidRoot {
                path: root_file,
                message: "unsupported policy root version".to_string(),
            });
        }
        if root_doc.include.is_empty() || root_doc.include.len() > MAX_POLICY_INCLUDES {
            return Err(Error::InvalidRoot {
                path: root.join("nexus.policy.toml"),
                message: "include list must be non-empty and bounded".to_string(),
            });
        }

        let mut policy = PolicyDoc::default();
        for include in root_doc.include {
            let include_path = validate_include(root, &include)?;
            let data = read_bounded(&include_path)?;
            reject_unknown_policy_sections(&include_path, &data)?;
            let parsed: RawPolicy = toml::from_str(&data).map_err(|source| Error::Parse {
                path: include_path.clone(),
                source,
            })?;
            policy.merge(parsed);
        }
        let version = policy_version(&policy)?;
        Ok(Self { version, policy })
    }

    pub fn version(&self) -> &PolicyVersion {
        &self.version
    }

    pub fn policy(&self) -> &PolicyDoc {
        &self.policy
    }

    pub fn manifest(&self) -> PolicyManifest {
        PolicyManifest {
            version: 1,
            tree_sha256: self.version.as_str().to_string(),
            generated_at_ns: 0,
        }
    }

    pub fn write_manifest(&self, root: &Path) -> Result<(), Error> {
        let manifest = serde_json::to_string_pretty(&self.manifest())
            .map_err(|err| Error::Canonical(err.to_string()))?;
        fs::write(root.join("manifest.json"), format!("{manifest}\n")).map_err(|source| {
            Error::Read {
                path: root.join("manifest.json"),
                source,
            }
        })
    }

    pub fn validate_manifest(&self, root: &Path) -> Result<(), Error> {
        let path = root.join("manifest.json");
        let data = read_bounded(&path)?;
        let manifest: PolicyManifest = serde_json::from_str(&data)
            .map_err(|err| Error::Canonical(format!("manifest parse failed: {err}")))?;
        let expected = self.version.as_str();
        if manifest.version != 1
            || manifest.generated_at_ns != 0
            || manifest.tree_sha256 != expected
        {
            return Err(Error::ManifestMismatch {
                expected: expected.to_string(),
                actual: manifest.tree_sha256,
            });
        }
        Ok(())
    }

    fn load_legacy_recipes(dir: &Path) -> Result<Self, Error> {
        let policy = PolicyDoc::load_dir(dir)?;
        let version = policy_version(&policy)?;
        Ok(Self { version, policy })
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPolicy {
    #[serde(default)]
    allow: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    abi_profile: BTreeMap<String, AbiProfile>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPolicyRoot {
    version: u32,
    #[serde(default)]
    include: Vec<String>,
}

fn canonical(input: &str) -> String {
    input.trim().to_ascii_lowercase()
}

fn read_bounded(path: &Path) -> Result<String, Error> {
    let metadata = fs::metadata(path).map_err(|source| Error::Read {
        path: path.into(),
        source,
    })?;
    let len = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
    if len > MAX_POLICY_FILE_BYTES {
        return Err(Error::Oversize {
            path: path.into(),
            len,
            max: MAX_POLICY_FILE_BYTES,
        });
    }
    fs::read_to_string(path).map_err(|source| Error::Read {
        path: path.into(),
        source,
    })
}

fn validate_include(root: &Path, include: &str) -> Result<PathBuf, Error> {
    let include_path = Path::new(include);
    if include_path.is_absolute()
        || include_path.components().any(|c| {
            matches!(
                c,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(Error::IncludeTraversal {
            path: include_path.to_path_buf(),
        });
    }
    Ok(root.join(include_path))
}

fn reject_unknown_policy_sections(path: &Path, data: &str) -> Result<(), Error> {
    let value = data.parse::<toml::Value>().map_err(|source| Error::Parse {
        path: path.to_path_buf(),
        source,
    })?;
    let allowed = ["allow", "abi_profile"];
    if let Some(table) = value.as_table() {
        for key in table.keys() {
            if !allowed.contains(&key.as_str()) {
                return Err(Error::UnknownSection {
                    path: path.into(),
                    section: key.to_string(),
                });
            }
        }
    }
    Ok(())
}

fn policy_version(policy: &PolicyDoc) -> Result<PolicyVersion, Error> {
    let canonical = serde_json::to_vec(policy).map_err(|err| Error::Canonical(err.to_string()))?;
    let digest = Sha256::digest(&canonical);
    Ok(PolicyVersion(format!("{digest:x}")))
}

fn has_toml_files(dir: &Path) -> Result<bool, Error> {
    if !dir.exists() {
        return Ok(false);
    }
    let entries = fs::read_dir(dir).map_err(|source| Error::Read {
        path: dir.into(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| Error::Read {
            path: dir.into(),
            source,
        })?;
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("toml") {
            return Ok(true);
        }
    }
    Ok(false)
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
            abi_profile: BTreeMap::new(),
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

    #[test]
    fn policy_tree_version_is_deterministic_for_same_inputs() {
        let temp = TempDir::new().unwrap();
        let policies = temp.path().join("policies");
        fs::create_dir_all(&policies).unwrap();
        fs::write(
            policies.join("nexus.policy.toml"),
            "version = 1\ninclude = ['base.toml']\n",
        )
        .unwrap();
        fs::write(
            policies.join("base.toml"),
            "[allow]\nExample = ['IPC.Core', 'time.read']\n",
        )
        .unwrap();

        let first = PolicyTree::load_root(&policies).unwrap();
        let second = PolicyTree::load_root(&policies).unwrap();

        assert_eq!(first.version(), second.version());
        assert!(first.policy().check(&["ipc.core"], "example").is_ok());
    }

    #[test]
    fn test_reject_invalid_policy_tree() {
        let temp = TempDir::new().unwrap();
        let policies = temp.path().join("policies");
        fs::create_dir_all(&policies).unwrap();
        fs::write(
            policies.join("nexus.policy.toml"),
            "version = 2\ninclude = ['base.toml']\n",
        )
        .unwrap();
        fs::write(policies.join("base.toml"), "[allow]\nsvc = ['ipc.core']\n").unwrap();

        let err = PolicyTree::load_root(&policies).unwrap_err();
        assert_eq!(err.code(), "policy.invalid_root");
    }

    #[test]
    fn test_reject_oversize_policy_tree() {
        let temp = TempDir::new().unwrap();
        let policies = temp.path().join("policies");
        fs::create_dir_all(&policies).unwrap();
        fs::write(
            policies.join("nexus.policy.toml"),
            "version = 1\ninclude = ['base.toml']\n",
        )
        .unwrap();
        fs::write(
            policies.join("base.toml"),
            "x".repeat(MAX_POLICY_FILE_BYTES + 1),
        )
        .unwrap();

        let err = PolicyTree::load_root(&policies).unwrap_err();
        assert_eq!(err.code(), "policy.oversize");
    }

    #[test]
    fn test_reject_policy_include_traversal() {
        let temp = TempDir::new().unwrap();
        let policies = temp.path().join("policies");
        fs::create_dir_all(&policies).unwrap();
        fs::write(
            policies.join("nexus.policy.toml"),
            "version = 1\ninclude = ['../base.toml']\n",
        )
        .unwrap();

        let err = PolicyTree::load_root(&policies).unwrap_err();
        assert_eq!(err.code(), "policy.include_traversal");
    }

    #[test]
    fn test_reject_ambiguous_policy_root() {
        let temp = TempDir::new().unwrap();
        fs::create_dir_all(temp.path().join("policies")).unwrap();
        fs::create_dir_all(temp.path().join("recipes/policy")).unwrap();
        fs::write(
            temp.path().join("recipes/policy/base.toml"),
            "[allow]\nsvc = ['ipc.core']\n",
        )
        .unwrap();

        let err = PolicyTree::load_single_authority(temp.path()).unwrap_err();
        assert_eq!(err.code(), "policy.ambiguous_root");
    }

    #[test]
    fn test_reject_unknown_policy_section() {
        let temp = TempDir::new().unwrap();
        let policies = temp.path().join("policies");
        fs::create_dir_all(&policies).unwrap();
        fs::write(
            policies.join("nexus.policy.toml"),
            "version = 1\ninclude = ['base.toml']\n",
        )
        .unwrap();
        fs::write(policies.join("base.toml"), "[unknown]\nsvc = true\n").unwrap();

        let err = PolicyTree::load_root(&policies).unwrap_err();
        assert_eq!(err.code(), "policy.unknown_section");
    }

    #[test]
    fn evaluator_returns_bounded_explain_trace_and_stable_reason() {
        let mut doc = PolicyDoc::default();
        doc.merge(RawPolicy {
            allow: BTreeMap::from([(
                "Example".to_string(),
                vec!["IPC.Core".to_string(), "time.read".to_string()],
            )]),
            abi_profile: BTreeMap::new(),
        });

        let decision = doc
            .evaluate(&["ipc.core", "time.read"], "EXAMPLE", PolicyMode::Enforce)
            .expect("evaluation");

        assert!(decision.allow);
        assert!(!decision.would_deny);
        assert_eq!(decision.reason_code.as_str(), "explicit_allow");
        assert_eq!(decision.trace.len(), 2);
        assert!(decision.trace.iter().all(|step| step.matched));
    }

    #[test]
    fn evaluator_is_deny_by_default_with_stable_missing_reason() {
        let doc = PolicyDoc::default();
        let decision = doc
            .evaluate(&["fs.write"], "unknown", PolicyMode::Enforce)
            .expect("evaluation");

        assert!(!decision.allow);
        assert!(decision.would_deny);
        assert_eq!(decision.reason_code.as_str(), "missing_capabilities");
        assert_eq!(decision.trace[0].capability, "fs.write");
        assert!(!decision.trace[0].matched);
    }

    #[test]
    fn dry_run_and_learn_do_not_bypass_enforce_denies() {
        let doc = PolicyDoc::default();

        for mode in [PolicyMode::DryRun, PolicyMode::Learn] {
            let decision = doc
                .evaluate(&["crypto.sign"], "demo", mode)
                .expect("evaluation");
            assert!(!decision.allow);
            assert!(decision.would_deny);
            assert_eq!(decision.reason_code, ReasonCode::MissingCapabilities);
            assert_eq!(decision.mode, mode);
        }
    }

    #[test]
    fn test_reject_unbounded_explain_trace() {
        let doc = PolicyDoc::default();
        let err = doc
            .evaluate_with_trace_limit(&["cap.a", "cap.b"], "demo", PolicyMode::Enforce, 1)
            .unwrap_err();

        assert_eq!(err.code(), "policy.explain_trace_over_budget");
    }

    #[test]
    fn evaluator_covers_abi_egress_and_signing_domain_shapes() {
        let mut doc = PolicyDoc::default();
        doc.merge(RawPolicy {
            allow: BTreeMap::from([
                (
                    "selftest-client".to_string(),
                    vec!["abi.statefs.put".to_string()],
                ),
                ("netstackd".to_string(), vec!["net.egress".to_string()]),
                ("keystored".to_string(), vec!["crypto.sign".to_string()]),
            ]),
            abi_profile: BTreeMap::new(),
        });

        assert!(
            doc.evaluate(&["abi.statefs.put"], "selftest-client", PolicyMode::Enforce)
                .expect("abi")
                .allow
        );
        assert!(
            doc.evaluate(&["net.egress"], "netstackd", PolicyMode::Enforce)
                .expect("egress")
                .allow
        );
        assert!(
            doc.evaluate(&["crypto.sign"], "keystored", PolicyMode::Enforce)
                .expect("signing")
                .allow
        );
    }

    #[test]
    fn learn_log_normalization_is_deterministic() {
        let doc = PolicyDoc::default();
        let decision = doc
            .evaluate(&["net.egress", "crypto.sign"], "demo", PolicyMode::Learn)
            .expect("learn eval");
        let observations = PolicyDoc::learn_observations(&decision);
        let mut reversed = observations.clone();
        reversed.reverse();

        assert_eq!(
            PolicyDoc::normalize_learn_log(observations),
            PolicyDoc::normalize_learn_log(reversed)
        );
    }

    #[test]
    fn policy_manifest_is_deterministic_and_validates_tree_hash() {
        let temp = TempDir::new().unwrap();
        let policies = temp.path().join("policies");
        fs::create_dir_all(&policies).unwrap();
        fs::write(
            policies.join("nexus.policy.toml"),
            "version = 1\ninclude = ['base.toml']\n",
        )
        .unwrap();
        fs::write(policies.join("base.toml"), "[allow]\ndemo = ['ipc.core']\n").unwrap();
        let tree = PolicyTree::load_root(&policies).unwrap();

        tree.write_manifest(&policies).unwrap();

        let manifest = fs::read_to_string(policies.join("manifest.json")).unwrap();
        assert!(manifest.contains("\"version\": 1"));
        assert!(manifest.contains("\"generated_at_ns\": 0"));
        assert!(manifest.contains(tree.version().as_str()));
        tree.validate_manifest(&policies).unwrap();
    }

    #[test]
    fn test_reject_policy_manifest_mismatch() {
        let temp = TempDir::new().unwrap();
        let policies = temp.path().join("policies");
        fs::create_dir_all(&policies).unwrap();
        fs::write(
            policies.join("nexus.policy.toml"),
            "version = 1\ninclude = ['base.toml']\n",
        )
        .unwrap();
        fs::write(policies.join("base.toml"), "[allow]\ndemo = ['ipc.core']\n").unwrap();
        fs::write(
            policies.join("manifest.json"),
            r#"{"version":1,"tree_sha256":"bad","generated_at_ns":0}"#,
        )
        .unwrap();
        let tree = PolicyTree::load_root(&policies).unwrap();

        let err = tree.validate_manifest(&policies).unwrap_err();

        assert_eq!(err.code(), "policy.manifest_mismatch");
    }

    #[test]
    fn adapter_parity_signing_capability_matches_legacy_check() {
        let mut doc = PolicyDoc::default();
        doc.merge(RawPolicy {
            allow: BTreeMap::from([("keystored".to_string(), vec!["crypto.sign".to_string()])]),
            abi_profile: BTreeMap::new(),
        });

        let legacy_allow = doc.check(&["crypto.sign"], "keystored").is_ok();
        let unified_allow = doc
            .evaluate(&["crypto.sign"], "keystored", PolicyMode::Enforce)
            .expect("unified eval")
            .allow;
        let legacy_deny = doc.check(&["crypto.verify"], "keystored").is_err();
        let unified_deny = !doc
            .evaluate(&["crypto.verify"], "keystored", PolicyMode::Enforce)
            .expect("unified eval")
            .allow;

        assert_eq!(unified_allow, legacy_allow);
        assert_eq!(unified_deny, legacy_deny);
    }

    #[test]
    fn adapter_parity_exec_capability_matches_legacy_check() {
        let mut doc = PolicyDoc::default();
        doc.merge(RawPolicy {
            allow: BTreeMap::from([("execd".to_string(), vec!["proc.spawn".to_string()])]),
            abi_profile: BTreeMap::new(),
        });

        let legacy_allow = doc.check(&["proc.spawn"], "execd").is_ok();
        let unified_allow = doc
            .evaluate(&["proc.spawn"], "execd", PolicyMode::Enforce)
            .expect("unified eval")
            .allow;
        let legacy_deny = doc.check(&["fs.write"], "execd").is_err();
        let unified_deny = !doc
            .evaluate(&["fs.write"], "execd", PolicyMode::Enforce)
            .expect("unified eval")
            .allow;

        assert_eq!(unified_allow, legacy_allow);
        assert_eq!(unified_deny, legacy_deny);
    }
}
