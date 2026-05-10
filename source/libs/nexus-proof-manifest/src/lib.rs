// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Host-only schema + parser for the `proof-manifest`, the SSOT
//! that promotes the `selftest-client` marker ladder, harness profiles, and
//! runtime selftest profiles to a single declarative artifact.
//!
//! Cut P4-01 introduced the skeleton (`[meta]` + 12 `[phase.*]` blocks +
//! placeholder `[profile.full]`). Cut P4-03 extended the schema with
//! `[marker."<literal>"]` entries (with `phase`, optional `proves`,
//! `introduced_in`, `emit_when`, `emit_when_not`, `forbidden_when`).
//! Cuts P4-05 (profile bodies) and P4-08 (runtime profiles) extended
//! `Profile` further. Cut **P5-00** introduces schema **v2**: a per-phase
//! split layout with `[include]` directives in the root manifest. The
//! parser dispatches on `[meta].schema_version`:
//!   - `"1"` (legacy single-file): consumed via [`parse`] from an inline
//!     `&str`, or via [`parse_path`] from a single TOML file. `[include]`
//!     is rejected.
//!   - `"2"` (per-phase split): only valid via [`parse_path`]; the root
//!     manifest contains `[meta]` + `[include]` and nothing else; included
//!     files are categorically restricted (`phases`/`markers`/`profiles`).
//!
//! Phase-4 invariant carried forward: field names on [`Manifest`],
//! [`Phase`], [`Profile`], [`Marker`], [`ProfileGate`] are append-only.
//! The set of [`ParseError`] variants is append-only across cuts.
//!
//! OWNERS: @runtime
//! STATUS: Functional (schema v1 + v2 from P5-00)
//! API_STABILITY: Unstable (Phase 5 evolves shape between cuts)
//! TEST_COVERAGE: see `tests/parse_skeleton.rs` (P4-01) +
//!                `tests/parse_markers.rs` (P4-03) +
//!                `tests/parse_v2.rs` (P5-00)
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod error;

pub use error::ParseError;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use serde::Deserialize;

/// Parsed view of a `proof-manifest` (single file v1 or split v2).
///
/// Phase-4 invariant: field names are append-only between cuts. Phase 5
/// adds [`Self::source_files`] for v2 build-script `cargo:rerun-if-changed=`
/// emission; existing field names never rename.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    /// Top-level `[meta]` table.
    pub meta: Meta,
    /// `[phase.X]` blocks indexed by phase name.
    pub phases: BTreeMap<String, Phase>,
    /// `[profile.X]` blocks indexed by profile name.
    pub profiles: BTreeMap<String, Profile>,
    /// `[marker."…"]` entries in declaration order. Order matters for the
    /// generated `markers_generated.rs` constant emission and for the
    /// harness's marker-ladder traversal.
    pub markers: Vec<Marker>,
    /// Absolute paths to every TOML file the parser read while building
    /// this `Manifest`. For v1 single-file inputs this is one entry (or
    /// empty for inline-string parses); for v2 inputs this is the root
    /// manifest plus every file resolved via `[include]`. Used by
    /// `selftest-client/build.rs` to emit one `cargo:rerun-if-changed=`
    /// per source file. Order is deterministic (root first, then
    /// includes in lexicographic glob-expansion order).
    pub source_files: Vec<PathBuf>,
}

/// `[meta]` table contents (closed schema; unknown keys reject).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Meta {
    /// Manifest schema version. Phase-4 ships `"1"`.
    pub schema_version: String,
    /// Profile that the harness defaults to when no `--profile=…` flag /
    /// `PROFILE=…` env is supplied.
    pub default_profile: String,
}

/// `[phase.X]` block contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Phase {
    /// 1-based numeric phase order (must be unique across the manifest).
    pub order: u8,
}

/// `[profile.X]` block contents.
///
/// `runner` is the harness script that owns the profile (e.g.
/// `scripts/qemu-test.sh` for single-VM, `tools/os2vm.sh` for the 2-VM
/// case); `extends` chains profile inheritance (e.g. `os2vm` extends
/// `full`); `env` is the flat env dictionary forwarded to the runner
/// (e.g. `REQUIRE_DSOFTBUS = "1"`). Unset fields signal "inherit from
/// parent" for `extends` resolution. Runtime-only profiles (P4-08) leave
/// `runner` unset and set `runtime_only = true`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Profile {
    /// Harness script that owns this profile (e.g.
    /// `scripts/qemu-test.sh`). Optional for runtime-only profiles.
    pub runner: Option<String>,
    /// Parent profile to inherit `runner` + `env` from. Cycles reject.
    pub extends: Option<String>,
    /// Flat env dictionary forwarded to the runner. Child entries
    /// override parent entries during `resolve_env_chain`.
    pub env: BTreeMap<String, String>,
    /// `true` for P4-08 runtime-only profiles (`bringup|quick|ota|net|none`).
    /// Runtime-only profiles MUST NOT carry a `runner`.
    pub runtime_only: bool,
    /// P4-08: ordered subset of `[phase.*]` names this profile enables.
    /// Empty = all phases (the implicit default for harness profiles).
    /// Each entry must reference a declared `[phase.X]`. Order is
    /// preserved as authored; `os_lite::run()` uses it to derive the
    /// dispatch order under `runtime_only = true` profiles.
    pub phases: Vec<String>,
}

/// `[marker."<literal>"]` entry.
///
/// `literal` is the exact UART-emitted byte sequence the harness expects
/// (e.g. `"SELFTEST: ipc routing keystored ok"` or `"dsoftbusd: ready"`).
/// `phase` is the manifest phase that this marker counts towards (gating
/// + future P4-08 profile filtering). `emit_when` / `emit_when_not` /
///   `forbidden_when` are profile-conditional clauses; `proves` and
///   `introduced_in` are free-form traceability annotations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Marker {
    /// The exact UART-emitted byte sequence.
    pub literal: String,
    /// Phase this marker belongs to (must reference a `[phase.X]`).
    pub phase: String,
    /// Optional free-form description of what asserting this marker proves.
    pub proves: Option<String>,
    /// Optional task ID that introduced this marker (for traceability).
    pub introduced_in: Option<String>,
    /// Profile that this marker is **only** emitted under (e.g.
    /// `quic-required`). When absent, the marker is profile-unconditional.
    pub emit_when: Option<ProfileGate>,
    /// Profile that this marker is **suppressed** under.
    pub emit_when_not: Option<ProfileGate>,
    /// Profile under which this marker MUST NOT appear (deny-by-default
    /// post-P4-09).
    pub forbidden_when: Option<ProfileGate>,
}

/// Profile selector for marker-emission clauses (`emit_when`,
/// `emit_when_not`, `forbidden_when`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileGate {
    /// Profile name that the clause references; must match a declared
    /// `[profile.X]` block.
    pub profile: String,
}

/// Parse an inline `proof-manifest` source string into a [`Manifest`].
///
/// This entry point handles **schema v1 only** (legacy single-file form).
/// Schema-v2 manifests live across multiple files on disk and require
/// [`parse_path`] for `[include]` resolution.
///
/// Returns a [`ParseError`] for any schema violation. The parser uses a
/// closed schema: any top-level key other than `meta`, `phase`,
/// `profile`, or `marker` rejects with [`ParseError::UnknownTopLevelKey`].
///
/// # Errors
///
/// See [`ParseError`] for the full list of rejection categories.
pub fn parse(source: &str) -> Result<Manifest, ParseError> {
    parse_with_sources(source, Vec::new())
}

/// Parse a `proof-manifest` from a file path, dispatching on
/// `[meta].schema_version` (`"1"` for legacy single-file, `"2"` for
/// per-phase split with `[include]` directives).
///
/// `Manifest::source_files` records every file the parser read so that
/// `selftest-client/build.rs` can emit one `cargo:rerun-if-changed=` per
/// source file. For v1 inputs this is just `[path]`; for v2 inputs this
/// is `[root, ...includes]` in deterministic, lexicographically-sorted
/// glob-expansion order.
///
/// # Errors
///
/// See [`ParseError`] for the full list of rejection categories.
pub fn parse_path(path: &Path) -> Result<Manifest, ParseError> {
    let src = read_to_string(path)?;
    let peek = peek_schema_version(&src)?;
    match peek.as_str() {
        "1" => {
            // v1: same body as `parse`, but record the source file so the
            // build script can rerun on disk edits.
            parse_with_sources(&src, vec![path.to_path_buf()])
        }
        "2" => parse_v2_root(path, &src),
        other => Err(ParseError::SchemaVersionUnsupported(other.to_string())),
    }
}

/// Internal: parse `source` as v1, recording `sources` on the resulting
/// [`Manifest`] for downstream `cargo:rerun-if-changed=` emission.
fn parse_with_sources(source: &str, sources: Vec<PathBuf>) -> Result<Manifest, ParseError> {
    let raw: RawManifest = toml::from_str(source).map_err(|e| ParseError::Toml(e.to_string()))?;

    if let Some(unknown) = raw.first_unknown_top_level_key() {
        return Err(ParseError::UnknownTopLevelKey(unknown));
    }
    if raw.include.is_some() {
        return Err(ParseError::IncludeInV1Source);
    }

    // -- meta ---------------------------------------------------------------
    let raw_meta = raw.meta.ok_or(ParseError::MissingMeta)?;
    if let Some(unknown) = raw_meta.first_unknown_key() {
        return Err(ParseError::UnknownMetaKey(unknown));
    }
    let schema_version = raw_meta
        .schema_version
        .filter(|s| !s.is_empty())
        .ok_or(ParseError::MissingSchemaVersion)?;
    let default_profile = raw_meta
        .default_profile
        .filter(|s| !s.is_empty())
        .ok_or(ParseError::MissingDefaultProfile)?;
    if schema_version == "2" {
        // Inline v2 sources are not allowed: v2 only makes sense across
        // multiple files on disk. Surface as a stable schema-mismatch rather
        // than letting the closed-schema reject fire on `[include]`.
        return Err(ParseError::IncludeInV1Source);
    }
    if schema_version != "1" {
        return Err(ParseError::SchemaVersionUnsupported(schema_version));
    }

    finish_parse(
        raw.phase.unwrap_or_default(),
        raw.profile.unwrap_or_default(),
        raw.marker.unwrap_or_default(),
        Meta {
            schema_version,
            default_profile,
        },
        sources,
    )
}

/// Take pre-merged `phase` / `profile` / `marker` raw maps (already
/// duplicate-checked across files for v2) and finish validation.
fn finish_parse(
    raw_phases: BTreeMap<String, RawPhase>,
    raw_profiles: BTreeMap<String, toml::Value>,
    raw_markers: IndexMap<String, RawMarker>,
    meta: Meta,
    source_files: Vec<PathBuf>,
) -> Result<Manifest, ParseError> {
    // -- phases -------------------------------------------------------------
    let mut phases: BTreeMap<String, Phase> = BTreeMap::new();
    let mut order_seen: BTreeMap<u8, String> = BTreeMap::new();
    for (name, body) in raw_phases {
        if phases.contains_key(&name) {
            return Err(ParseError::DuplicatePhase(name));
        }
        if let Some(prev) = order_seen.get(&body.order) {
            return Err(ParseError::PhaseOrderConflict {
                order: body.order,
                first: prev.clone(),
                second: name,
            });
        }
        order_seen.insert(body.order, name.clone());
        phases.insert(name, Phase { order: body.order });
    }

    // -- profiles -----------------------------------------------------------
    let mut profiles: BTreeMap<String, Profile> = BTreeMap::new();
    for (name, value) in raw_profiles {
        let raw_profile: RawProfile =
            value
                .try_into()
                .map_err(|e: toml::de::Error| ParseError::ProfileBodyInvalid {
                    profile: name.clone(),
                    detail: e.to_string(),
                })?;
        if raw_profile.runtime_only && raw_profile.runner.is_some() {
            return Err(ParseError::ProfileRuntimeOnlyWithRunner(name));
        }
        let phases_opt = raw_profile.phases.unwrap_or_default();
        // Reject `phases = […]` on harness profiles to preserve the
        // invariant that the harness ladder always covers every declared
        // phase. Runtime-only profiles use it to scope dispatch.
        if !raw_profile.runtime_only && !phases_opt.is_empty() {
            return Err(ParseError::ProfileBodyInvalid {
                profile: name.clone(),
                detail: "`phases = [...]` only allowed on `runtime_only = true` profiles".into(),
            });
        }
        // Each declared phase must reference a known [phase.X].
        for ph in &phases_opt {
            if !phases.contains_key(ph) {
                return Err(ParseError::ProfileBodyInvalid {
                    profile: name.clone(),
                    detail: format!("unknown phase reference `{ph}` in `phases = [...]`"),
                });
            }
        }
        profiles.insert(
            name,
            Profile {
                runner: raw_profile.runner,
                extends: raw_profile.extends,
                env: raw_profile.env.unwrap_or_default(),
                runtime_only: raw_profile.runtime_only,
                phases: phases_opt,
            },
        );
    }
    // Validate `extends` references + reject cycles (post-collection so we
    // can resolve forward references in any declaration order).
    for (name, p) in &profiles {
        if let Some(parent) = &p.extends {
            if !profiles.contains_key(parent) {
                return Err(ParseError::ProfileUnknownParent {
                    profile: name.clone(),
                    parent: parent.clone(),
                });
            }
        }
    }
    detect_extends_cycle(&profiles)?;

    // -- markers ------------------------------------------------------------
    let mut markers: Vec<Marker> = Vec::new();
    let mut seen_marker: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (literal, raw_marker) in raw_markers {
        if !seen_marker.insert(literal.clone()) {
            return Err(ParseError::DuplicateMarker(literal));
        }
        let phase = raw_marker
            .phase
            .ok_or_else(|| ParseError::MarkerMissingPhase(literal.clone()))?;
        if !phases.contains_key(&phase) {
            return Err(ParseError::MarkerUnknownPhase {
                marker: literal.clone(),
                phase,
            });
        }
        check_profile_ref(&literal, &raw_marker.emit_when, "emit_when", &profiles)?;
        check_profile_ref(
            &literal,
            &raw_marker.emit_when_not,
            "emit_when_not",
            &profiles,
        )?;
        check_profile_ref(
            &literal,
            &raw_marker.forbidden_when,
            "forbidden_when",
            &profiles,
        )?;
        markers.push(Marker {
            literal,
            phase,
            proves: raw_marker.proves,
            introduced_in: raw_marker.introduced_in,
            emit_when: raw_marker.emit_when.map(into_gate),
            emit_when_not: raw_marker.emit_when_not.map(into_gate),
            forbidden_when: raw_marker.forbidden_when.map(into_gate),
        });
    }

    Ok(Manifest {
        meta,
        phases,
        profiles,
        markers,
        source_files,
    })
}

/// Read a UTF-8 file, mapping I/O errors to a stable [`ParseError::Io`].
fn read_to_string(path: &Path) -> Result<String, ParseError> {
    std::fs::read_to_string(path).map_err(|e| ParseError::Io {
        path: path.display().to_string(),
        detail: e.to_string(),
    })
}

/// Peek `[meta].schema_version` from a raw TOML source without running
/// the closed-schema validator. Used by [`parse_path`] to dispatch v1/v2
/// before the full structural reject pipeline runs.
fn peek_schema_version(source: &str) -> Result<String, ParseError> {
    #[derive(Deserialize)]
    struct Peek {
        meta: Option<PeekMeta>,
    }
    #[derive(Deserialize)]
    struct PeekMeta {
        #[serde(default)]
        schema_version: Option<String>,
    }
    let peek: Peek = toml::from_str(source).map_err(|e| ParseError::Toml(e.to_string()))?;
    let meta = peek.meta.ok_or(ParseError::MissingMeta)?;
    meta.schema_version
        .filter(|s| !s.is_empty())
        .ok_or(ParseError::MissingSchemaVersion)
}

// ---------------------------------------------------------------------------
// P5-00: schema-v2 (`[include]` directives) resolver
// ---------------------------------------------------------------------------

/// Categories supported by `[include]` in v2 root manifests. The order
/// here is the order the resolver loads files in, which is also the order
/// `Manifest::source_files` records after the root entry.
const INCLUDE_CATEGORIES: [&str; 3] = ["phases", "markers", "profiles"];

/// Parse a v2 root manifest (`source` already known to declare
/// `schema_version = "2"`). Resolves `[include]` globs deterministically
/// against the root file's parent directory, parses each included file
/// under its categorical restriction, and merges into a flat
/// [`Manifest`].
fn parse_v2_root(root_path: &Path, source: &str) -> Result<Manifest, ParseError> {
    let raw: RawManifest = toml::from_str(source).map_err(|e| ParseError::Toml(e.to_string()))?;

    if let Some(unknown) = raw.first_unknown_top_level_key() {
        return Err(ParseError::UnknownTopLevelKey(unknown));
    }
    // v2 root must contain only [meta] + [include]; phases/markers/profiles
    // belong in included files. This is the "MixedSchemaInRoot" gate.
    if raw.phase.as_ref().is_some_and(|m| !m.is_empty()) {
        return Err(ParseError::MixedSchemaInRoot("phase".into()));
    }
    if raw.profile.as_ref().is_some_and(|m| !m.is_empty()) {
        return Err(ParseError::MixedSchemaInRoot("profile".into()));
    }
    if raw.marker.as_ref().is_some_and(|m| !m.is_empty()) {
        return Err(ParseError::MixedSchemaInRoot("marker".into()));
    }

    // -- meta ---------------------------------------------------------------
    let raw_meta = raw.meta.ok_or(ParseError::MissingMeta)?;
    if let Some(unknown) = raw_meta.first_unknown_key() {
        return Err(ParseError::UnknownMetaKey(unknown));
    }
    let schema_version = raw_meta
        .schema_version
        .filter(|s| !s.is_empty())
        .ok_or(ParseError::MissingSchemaVersion)?;
    debug_assert_eq!(
        schema_version, "2",
        "parse_v2_root invoked on non-v2 source"
    );
    let default_profile = raw_meta
        .default_profile
        .filter(|s| !s.is_empty())
        .ok_or(ParseError::MissingDefaultProfile)?;
    let meta = Meta {
        schema_version,
        default_profile,
    };

    // -- include resolution -------------------------------------------------
    let include = raw.include.ok_or_else(|| {
        // A v2 manifest with no [include] is structurally pointless: the
        // root file declares schema_version="2" but provides no content.
        // Surface this as IncludeGlobEmpty for the canonical category, so
        // operators see a clear "include is missing" error rather than an
        // empty manifest silently passing.
        ParseError::IncludeGlobEmpty {
            category: "phases",
            pattern: "<missing [include] table>".into(),
        }
    })?;
    if let Some(unknown) = include.first_unknown_key() {
        return Err(ParseError::UnknownTopLevelKey(format!("include.{unknown}")));
    }

    let root_dir = root_path.parent().unwrap_or_else(|| Path::new("."));
    let mut source_files: Vec<PathBuf> = vec![root_path.to_path_buf()];
    let mut merged_phases: BTreeMap<String, RawPhase> = BTreeMap::new();
    let mut merged_phase_origin: BTreeMap<String, PathBuf> = BTreeMap::new();
    let mut merged_profiles: BTreeMap<String, toml::Value> = BTreeMap::new();
    let mut merged_profile_origin: BTreeMap<String, PathBuf> = BTreeMap::new();
    let mut merged_markers: IndexMap<String, RawMarker> = IndexMap::new();
    let mut merged_marker_origin: BTreeMap<String, PathBuf> = BTreeMap::new();

    for category in INCLUDE_CATEGORIES {
        let pattern = match category {
            "phases" => include.phases.as_deref(),
            "markers" => include.markers.as_deref(),
            "profiles" => include.profiles.as_deref(),
            _ => unreachable!(),
        };
        let Some(pattern) = pattern else { continue };

        let files = expand_include_glob(root_dir, category, pattern)?;
        for file in files {
            let file_src = read_to_string(&file)?;
            let raw_part: RawManifest = toml::from_str(&file_src)
                .map_err(|e| ParseError::Toml(format!("{}: {e}", file.display())))?;
            // Each included file must NOT itself be a v2 root.
            if raw_part.meta.is_some() || raw_part.include.is_some() {
                return Err(ParseError::NestedInclude(file.display().to_string()));
            }
            // Closed-schema check on included files: top-level keys other
            // than the four schema-defined ones reject. This catches typos
            // like `[markers]` (plural) before they silently parse.
            if let Some(unknown) = raw_part.first_unknown_top_level_key() {
                return Err(ParseError::UnknownTopLevelKey(format!(
                    "{} (in {})",
                    unknown,
                    file.display()
                )));
            }
            // Categorical restriction: the include directive declared
            // intent; the file must honor it. This makes per-phase splits
            // unambiguous to read and impossible to accidentally cross.
            check_include_category(&file, category, &raw_part)?;

            match category {
                "phases" => {
                    for (name, phase) in raw_part.phase.unwrap_or_default() {
                        if let Some(prev) = merged_phase_origin.get(&name) {
                            return Err(ParseError::DuplicatePhaseAcrossFiles {
                                phase: name,
                                first: prev.display().to_string(),
                                second: file.display().to_string(),
                            });
                        }
                        merged_phase_origin.insert(name.clone(), file.clone());
                        merged_phases.insert(name, phase);
                    }
                }
                "profiles" => {
                    for (name, value) in raw_part.profile.unwrap_or_default() {
                        if let Some(prev) = merged_profile_origin.get(&name) {
                            return Err(ParseError::DuplicateProfileAcrossFiles {
                                profile: name,
                                first: prev.display().to_string(),
                                second: file.display().to_string(),
                            });
                        }
                        merged_profile_origin.insert(name.clone(), file.clone());
                        merged_profiles.insert(name, value);
                    }
                }
                "markers" => {
                    for (literal, marker) in raw_part.marker.unwrap_or_default() {
                        if let Some(prev) = merged_marker_origin.get(&literal) {
                            return Err(ParseError::DuplicateMarkerAcrossFiles {
                                marker: literal,
                                first: prev.display().to_string(),
                                second: file.display().to_string(),
                            });
                        }
                        merged_marker_origin.insert(literal.clone(), file.clone());
                        merged_markers.insert(literal, marker);
                    }
                }
                _ => unreachable!(),
            }
            source_files.push(file);
        }
    }

    finish_parse(
        merged_phases,
        merged_profiles,
        merged_markers,
        meta,
        source_files,
    )
}

/// Reject if an included file declares a section outside its category.
fn check_include_category(
    file: &Path,
    category: &'static str,
    part: &RawManifest,
) -> Result<(), ParseError> {
    let allowed_section = match category {
        "phases" => "phase",
        "profiles" => "profile",
        "markers" => "marker",
        _ => unreachable!(),
    };
    let report = |key: &str| ParseError::IncludeFileWrongCategory {
        file: file.display().to_string(),
        category,
        key: key.to_string(),
    };
    if allowed_section != "phase" && part.phase.as_ref().is_some_and(|m| !m.is_empty()) {
        return Err(report("phase"));
    }
    if allowed_section != "profile" && part.profile.as_ref().is_some_and(|m| !m.is_empty()) {
        return Err(report("profile"));
    }
    if allowed_section != "marker" && part.marker.as_ref().is_some_and(|m| !m.is_empty()) {
        return Err(report("marker"));
    }
    Ok(())
}

/// Resolve an `[include].<category>` glob pattern against `root_dir`,
/// returning a deterministic, lexicographically-sorted file list.
/// Empty matches are an error (callers must declare globs that actually
/// match something).
fn expand_include_glob(
    root_dir: &Path,
    category: &'static str,
    pattern: &str,
) -> Result<Vec<PathBuf>, ParseError> {
    let pat_path = root_dir.join(pattern);
    let pat_str = pat_path
        .to_str()
        .ok_or_else(|| ParseError::IncludeGlobEmpty {
            category,
            pattern: pattern.to_string(),
        })?;
    let entries = glob::glob(pat_str).map_err(|e| ParseError::IncludeGlobEmpty {
        category,
        pattern: format!("{pattern} ({e})"),
    })?;
    let mut files: Vec<PathBuf> = Vec::new();
    for entry in entries {
        let path = entry.map_err(|e| ParseError::Io {
            path: pat_str.to_string(),
            detail: e.to_string(),
        })?;
        // Skip directories: a glob like `markers/*` may match dirs on some
        // filesystems; we only consume files.
        if path.is_file() {
            files.push(path);
        }
    }
    files.sort();
    if files.is_empty() {
        return Err(ParseError::IncludeGlobEmpty {
            category,
            pattern: pattern.to_string(),
        });
    }
    Ok(files)
}

impl Manifest {
    /// Convenience iterator over markers in declaration order.
    pub fn markers(&self) -> impl Iterator<Item = &Marker> {
        self.markers.iter()
    }

    /// Sub-iterator filtered by phase name.
    pub fn markers_in_phase<'a>(&'a self, phase: &'a str) -> impl Iterator<Item = &'a Marker> {
        self.markers.iter().filter(move |m| m.phase == phase)
    }

    /// Resolve the flattened env dictionary for `profile`, walking
    /// `extends` toward the root and letting child entries shadow parent
    /// entries.
    ///
    /// # Errors
    ///
    /// Returns [`ParseError::ProfileUnknownParent`] if a transitive
    /// `extends` references a missing profile (cycles already rejected
    /// at parse time).
    pub fn resolve_env_chain(&self, profile: &str) -> Result<BTreeMap<String, String>, ParseError> {
        let chain = self.profile_chain(profile)?;
        let mut out: BTreeMap<String, String> = BTreeMap::new();
        for ancestor in chain.iter().rev() {
            let p = &self.profiles[ancestor];
            for (k, v) in &p.env {
                out.insert(k.clone(), v.clone());
            }
        }
        Ok(out)
    }

    /// Return the profile names from `profile` (child) up to its root
    /// ancestor (parent-most) in inheritance order. The first entry is
    /// always `profile` itself.
    fn profile_chain(&self, profile: &str) -> Result<Vec<String>, ParseError> {
        let mut chain = Vec::new();
        let mut cur = profile.to_string();
        loop {
            if !self.profiles.contains_key(&cur) {
                return Err(ParseError::ProfileUnknownParent {
                    profile: profile.to_string(),
                    parent: cur,
                });
            }
            chain.push(cur.clone());
            match &self.profiles[&cur].extends {
                Some(parent) => cur = parent.clone(),
                None => return Ok(chain),
            }
        }
    }

    /// Markers expected to appear (in declaration order) under `profile`.
    /// Honors `emit_when` (declared profile must match) and `emit_when_not`
    /// (declared profile must NOT match) clauses; profile-unconditional
    /// markers always pass through.
    pub fn expected_markers<'a>(&'a self, profile: &'a str) -> impl Iterator<Item = &'a Marker> {
        self.markers
            .iter()
            .filter(move |m| marker_active(m, profile))
    }

    /// Markers forbidden under `profile` (`forbidden_when.profile` matches).
    pub fn forbidden_markers<'a>(&'a self, profile: &'a str) -> impl Iterator<Item = &'a Marker> {
        self.markers.iter().filter(move |m| {
            m.forbidden_when
                .as_ref()
                .is_some_and(|g| g.profile == profile)
        })
    }
}

fn marker_active(marker: &Marker, profile: &str) -> bool {
    if let Some(g) = &marker.emit_when {
        if g.profile != profile {
            return false;
        }
    }
    if let Some(g) = &marker.emit_when_not {
        if g.profile == profile {
            return false;
        }
    }
    // `forbidden_when` markers belong on the deny-list for that profile;
    // they MUST NOT appear in the expected-ladder for the same profile.
    if let Some(g) = &marker.forbidden_when {
        if g.profile == profile {
            return false;
        }
    }
    true
}

fn detect_extends_cycle(profiles: &BTreeMap<String, Profile>) -> Result<(), ParseError> {
    for start in profiles.keys() {
        let mut seen = std::collections::HashSet::new();
        let mut cur = start.clone();
        while let Some(parent) = profiles[&cur].extends.clone() {
            if !seen.insert(cur.clone()) {
                return Err(ParseError::ProfileExtendsCycle(start.clone()));
            }
            if !profiles.contains_key(&parent) {
                // already validated above; defensive guard.
                break;
            }
            if parent == *start {
                return Err(ParseError::ProfileExtendsCycle(start.clone()));
            }
            cur = parent;
        }
    }
    Ok(())
}

impl Marker {
    /// Generate a deterministic uppercase-snake-case constant key for use
    /// in `markers_generated.rs`. The key is derived from the literal by
    /// keeping ASCII alphanumerics, replacing other bytes with `_`, then
    /// uppercasing. Adjacent `_`s collapse to one. Used by the build
    /// script (P4-03+).
    pub fn const_key(&self) -> String {
        let mut out = String::with_capacity(self.literal.len() + 4);
        let mut last_was_underscore = false;
        for b in self.literal.bytes() {
            let c = if b.is_ascii_alphanumeric() {
                b.to_ascii_uppercase() as char
            } else {
                '_'
            };
            if c == '_' {
                if !last_was_underscore && !out.is_empty() {
                    out.push('_');
                    last_was_underscore = true;
                }
            } else {
                out.push(c);
                last_was_underscore = false;
            }
        }
        if out.ends_with('_') {
            out.pop();
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn check_profile_ref(
    literal: &str,
    gate: &Option<RawProfileGate>,
    clause: &'static str,
    profiles: &BTreeMap<String, Profile>,
) -> Result<(), ParseError> {
    if let Some(g) = gate {
        if !profiles.contains_key(&g.profile) {
            return Err(ParseError::MarkerUnknownProfile {
                marker: literal.to_string(),
                profile: g.profile.clone(),
                clause,
            });
        }
    }
    Ok(())
}

fn into_gate(raw: RawProfileGate) -> ProfileGate {
    ProfileGate {
        profile: raw.profile,
    }
}

// ---------------------------------------------------------------------------
// Raw TOML view (private; mirrors the on-disk shape, then validated above).
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct RawManifest {
    #[serde(default)]
    meta: Option<RawMeta>,
    /// P5-00: schema-v2 `[include]` directives. Only legal in v2 root
    /// manifests; rejected in v1 (`ParseError::IncludeInV1Source`).
    #[serde(default)]
    include: Option<RawInclude>,
    #[serde(default)]
    phase: Option<BTreeMap<String, RawPhase>>,
    #[serde(default)]
    profile: Option<BTreeMap<String, toml::Value>>,
    #[serde(default)]
    marker: Option<IndexMap<String, RawMarker>>,
    #[serde(flatten)]
    extra: BTreeMap<String, toml::Value>,
}

/// P5-00: `[include]` directive table. Each field is an optional glob
/// pattern (relative to the root manifest's directory) that matches files
/// containing **only** the named category. Unknown keys reject.
#[derive(Debug, Deserialize)]
struct RawInclude {
    #[serde(default)]
    phases: Option<String>,
    #[serde(default)]
    markers: Option<String>,
    #[serde(default)]
    profiles: Option<String>,
    #[serde(flatten)]
    extra: BTreeMap<String, toml::Value>,
}

impl RawInclude {
    fn first_unknown_key(&self) -> Option<String> {
        self.extra.keys().next().cloned()
    }
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawProfile {
    #[serde(default)]
    runner: Option<String>,
    #[serde(default)]
    extends: Option<String>,
    #[serde(default)]
    env: Option<BTreeMap<String, String>>,
    #[serde(default)]
    runtime_only: bool,
    /// P4-08: ordered subset of `[phase.*]` names that this profile
    /// enables. Only meaningful for `runtime_only = true` profiles;
    /// harness profiles (`full|smp|...`) leave this empty (= "all phases").
    #[serde(default)]
    phases: Option<Vec<String>>,
}

impl RawManifest {
    fn first_unknown_top_level_key(&self) -> Option<String> {
        self.extra.keys().next().cloned()
    }
}

#[derive(Debug, Deserialize)]
struct RawMeta {
    schema_version: Option<String>,
    default_profile: Option<String>,
    #[serde(flatten)]
    extra: BTreeMap<String, toml::Value>,
}

impl RawMeta {
    fn first_unknown_key(&self) -> Option<String> {
        self.extra.keys().next().cloned()
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPhase {
    order: u8,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMarker {
    phase: Option<String>,
    #[serde(default)]
    proves: Option<String>,
    #[serde(default)]
    introduced_in: Option<String>,
    #[serde(default)]
    emit_when: Option<RawProfileGate>,
    #[serde(default)]
    emit_when_not: Option<RawProfileGate>,
    #[serde(default)]
    forbidden_when: Option<RawProfileGate>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProfileGate {
    profile: String,
}
