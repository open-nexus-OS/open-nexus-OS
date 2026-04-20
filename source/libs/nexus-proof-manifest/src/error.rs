// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Stable error variants for the `nexus-proof-manifest` parser.
//! Variants are deliberately granular so that downstream tools (build.rs,
//! `qemu-test.sh`, CI) can match on a specific failure cause and emit a
//! deterministic, actionable diagnostic instead of a free-form message.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable (Phase 4 evolves variants as schema grows)
//! TEST_COVERAGE: see `tests/parse_skeleton.rs` (P4-01) +
//!                `tests/parse_markers.rs` (P4-03)
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

use core::fmt;

/// Stable parser error categories.
///
/// Each variant maps to a single deterministic schema violation. Variants
/// carry the offending key / phase / profile / marker name as a String so
/// that callers can include it in a human-readable diagnostic without
/// re-parsing the TOML AST.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum ParseError {
    /// The underlying TOML is syntactically invalid (mismatched brackets,
    /// unterminated string, etc.). The wrapped String is the upstream
    /// parser message, captured verbatim for diagnostics.
    Toml(String),

    /// `[meta]` table is entirely absent.
    MissingMeta,

    /// `[meta] schema_version = "..."` is missing or empty.
    MissingSchemaVersion,

    /// `[meta] default_profile = "..."` is missing or empty.
    MissingDefaultProfile,

    /// A top-level key (other than `meta`, `phase`, `profile`, `marker`) is
    /// declared. The wrapped String is the offending key. Phase 4 keeps
    /// the schema closed so that future additions are explicit.
    UnknownTopLevelKey(String),

    /// A key inside `[meta]` is unknown. The wrapped String is the
    /// offending key (e.g. `oops_typo`).
    UnknownMetaKey(String),

    /// Two `[phase.X]` blocks share the same name. The wrapped String is
    /// the duplicate phase name.
    DuplicatePhase(String),

    /// Two `[phase.X]` blocks share the same numeric `order` value. The
    /// wrapped tuple is `(order, first_phase_name, second_phase_name)`.
    PhaseOrderConflict {
        /// Numeric phase order that collides.
        order: u8,
        /// First phase that claimed this `order`.
        first: String,
        /// Second phase that also claimed this `order`.
        second: String,
    },

    /// A `[marker."…"]` entry is missing its `phase = "…"` field.
    /// The wrapped String is the marker literal.
    MarkerMissingPhase(String),

    /// A `[marker."…"]` entry references a phase that has no matching
    /// `[phase.X]` declaration. Wrapped fields: marker literal, unknown
    /// phase name.
    MarkerUnknownPhase {
        /// The marker literal whose `phase` field references a missing phase.
        marker: String,
        /// The phase name that was referenced but never declared.
        phase: String,
    },

    /// A `[marker."…"]` entry's `emit_when` / `emit_when_not` /
    /// `forbidden_when` clause references a profile that has no matching
    /// `[profile.X]` declaration. Wrapped fields: marker literal, missing
    /// profile name, which clause referenced it (`emit_when`,
    /// `emit_when_not`, or `forbidden_when`).
    MarkerUnknownProfile {
        /// The marker literal whose clause references a missing profile.
        marker: String,
        /// The profile name that was referenced but never declared.
        profile: String,
        /// Which clause did the bad reference: `"emit_when"`,
        /// `"emit_when_not"`, or `"forbidden_when"`.
        clause: &'static str,
    },

    /// Two `[marker."…"]` headers share the same literal (the TOML parser
    /// would normally reject this as a syntax-level redefinition; we
    /// surface a stable variant for cases where logical duplication slips
    /// through via dotted/inline forms).
    DuplicateMarker(String),

    /// A `[profile.X]` body failed structural validation (unknown keys,
    /// wrong value types, etc.). Wrapped fields: profile name, raw upstream
    /// detail.
    ProfileBodyInvalid {
        /// The profile whose body could not be parsed.
        profile: String,
        /// Verbatim upstream parser detail.
        detail: String,
    },

    /// A `[profile.X]` block sets `runtime_only = true` but also declares a
    /// `runner = …`; runtime-only profiles are dispatched by the OS-side
    /// `os_lite::profile` (P4-08) and never invoke a host harness.
    ProfileRuntimeOnlyWithRunner(String),

    /// A `[profile.X]` `extends = "…"` references a profile that has no
    /// matching `[profile.Y]` declaration. Wrapped fields: child profile,
    /// missing parent.
    ProfileUnknownParent {
        /// The profile whose `extends` chain breaks.
        profile: String,
        /// The first parent that could not be resolved.
        parent: String,
    },

    /// A `[profile.X]` `extends` chain forms a cycle (direct or transitive).
    /// Wrapped String is one of the profiles participating in the cycle.
    ProfileExtendsCycle(String),

    // -- P5-00 (schema v2: per-phase split layout) -----------------------------
    /// `[meta] schema_version` is set to a value the parser does not handle.
    /// Wrapped String is the offending value (e.g. `"3"`). Phase-5 supports
    /// `"1"` (legacy single-file) and `"2"` (per-phase split via `[include]`).
    SchemaVersionUnsupported(String),

    /// A v2 `[include]` glob pattern resolved to zero files. Wrapped fields:
    /// the include category (`phases`/`markers`/`profiles`) and the literal
    /// glob string (relative to the root manifest's directory).
    IncludeGlobEmpty {
        /// The include category whose glob did not match any file.
        category: &'static str,
        /// The literal glob pattern, verbatim from the root manifest.
        pattern: String,
    },

    /// A v2 included file is **itself** a v2 manifest (carries `[meta]` /
    /// `[include]`). Nested includes are forbidden: include depth is 1.
    /// Wrapped String is the offending file path.
    NestedInclude(String),

    /// A v2 included file contains a TOML section that does not match its
    /// declared category. Wrapped fields: file path, category the include
    /// directive declared (`phases`/`markers`/`profiles`), the offending
    /// top-level key.
    IncludeFileWrongCategory {
        /// The included file whose contents violate its category contract.
        file: String,
        /// The include category the file was loaded under.
        category: &'static str,
        /// The first top-level key in the file that is not allowed under
        /// the declared category.
        key: String,
    },

    /// A v2 root manifest declares `schema_version = "2"` but **also**
    /// inlines a phase / marker / profile section that should live in an
    /// included file. Wrapped String is the offending top-level key
    /// (`phase` / `marker` / `profile`).
    MixedSchemaInRoot(String),

    /// A v2 `[include]` resolution discovered the same `[marker."…"]`
    /// literal in two distinct files. Wrapped fields: marker literal, the
    /// two file paths participating in the duplication.
    DuplicateMarkerAcrossFiles {
        /// The marker literal that appears in two files.
        marker: String,
        /// File the marker was first seen in.
        first: String,
        /// File the duplicate was discovered in.
        second: String,
    },

    /// A v2 `[include]` resolution discovered the same `[phase.X]`
    /// declaration in two distinct files. Wrapped fields analogous to
    /// [`Self::DuplicateMarkerAcrossFiles`].
    DuplicatePhaseAcrossFiles {
        /// The phase name that appears in two files.
        phase: String,
        /// File the phase was first seen in.
        first: String,
        /// File the duplicate was discovered in.
        second: String,
    },

    /// A v2 `[include]` resolution discovered the same `[profile.X]`
    /// declaration in two distinct files. Wrapped fields analogous to
    /// [`Self::DuplicateMarkerAcrossFiles`].
    DuplicateProfileAcrossFiles {
        /// The profile name that appears in two files.
        profile: String,
        /// File the profile was first seen in.
        first: String,
        /// File the duplicate was discovered in.
        second: String,
    },

    /// A v1 inline source contains an `[include]` table; only v2 root
    /// manifests on disk may use `[include]`.
    IncludeInV1Source,

    /// I/O failure while reading the root manifest or one of its included
    /// files. Wrapped fields: file path, lossy upstream error message.
    Io {
        /// Path the parser tried to read.
        path: String,
        /// Verbatim io::Error message.
        detail: String,
    },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Toml(msg) => write!(f, "proof-manifest: TOML parse error: {msg}"),
            Self::MissingMeta => write!(f, "proof-manifest: missing [meta] table"),
            Self::MissingSchemaVersion => {
                write!(f, "proof-manifest: [meta].schema_version missing or empty")
            }
            Self::MissingDefaultProfile => {
                write!(f, "proof-manifest: [meta].default_profile missing or empty")
            }
            Self::UnknownTopLevelKey(k) => {
                write!(f, "proof-manifest: unknown top-level key `{k}`")
            }
            Self::UnknownMetaKey(k) => write!(f, "proof-manifest: unknown [meta] key `{k}`"),
            Self::DuplicatePhase(p) => write!(f, "proof-manifest: duplicate phase `{p}`"),
            Self::PhaseOrderConflict { order, first, second } => write!(
                f,
                "proof-manifest: phase order {order} claimed by both `{first}` and `{second}`"
            ),
            Self::MarkerMissingPhase(m) => {
                write!(f, "proof-manifest: marker `{m}` missing required `phase` field")
            }
            Self::MarkerUnknownPhase { marker, phase } => write!(
                f,
                "proof-manifest: marker `{marker}` references undeclared phase `{phase}`"
            ),
            Self::MarkerUnknownProfile { marker, profile, clause } => write!(
                f,
                "proof-manifest: marker `{marker}` `{clause}` references undeclared profile `{profile}`"
            ),
            Self::DuplicateMarker(m) => {
                write!(f, "proof-manifest: duplicate marker `{m}`")
            }
            Self::ProfileBodyInvalid { profile, detail } => write!(
                f,
                "proof-manifest: profile `{profile}` body invalid: {detail}"
            ),
            Self::ProfileRuntimeOnlyWithRunner(p) => write!(
                f,
                "proof-manifest: profile `{p}` is runtime_only but declares a runner"
            ),
            Self::ProfileUnknownParent { profile, parent } => write!(
                f,
                "proof-manifest: profile `{profile}` extends undeclared profile `{parent}`"
            ),
            Self::ProfileExtendsCycle(p) => write!(
                f,
                "proof-manifest: profile `{p}` extends chain forms a cycle"
            ),
            Self::SchemaVersionUnsupported(v) => write!(
                f,
                "proof-manifest: unsupported [meta].schema_version `{v}` (parser handles `1` + `2`)"
            ),
            Self::IncludeGlobEmpty { category, pattern } => write!(
                f,
                "proof-manifest: [include].{category} = `{pattern}` matched zero files"
            ),
            Self::NestedInclude(p) => write!(
                f,
                "proof-manifest: included file `{p}` itself carries [meta]/[include] (nested include forbidden)"
            ),
            Self::IncludeFileWrongCategory { file, category, key } => write!(
                f,
                "proof-manifest: included file `{file}` (category `{category}`) declares disallowed top-level key `{key}`"
            ),
            Self::MixedSchemaInRoot(k) => write!(
                f,
                "proof-manifest: schema v2 root must not declare top-level `{k}`; move it into a [include] file"
            ),
            Self::DuplicateMarkerAcrossFiles { marker, first, second } => write!(
                f,
                "proof-manifest: marker `{marker}` declared in both `{first}` and `{second}`"
            ),
            Self::DuplicatePhaseAcrossFiles { phase, first, second } => write!(
                f,
                "proof-manifest: phase `{phase}` declared in both `{first}` and `{second}`"
            ),
            Self::DuplicateProfileAcrossFiles { profile, first, second } => write!(
                f,
                "proof-manifest: profile `{profile}` declared in both `{first}` and `{second}`"
            ),
            Self::IncludeInV1Source => write!(
                f,
                "proof-manifest: [include] is only valid in v2 manifests (schema_version = \"2\")"
            ),
            Self::Io { path, detail } => write!(
                f,
                "proof-manifest: io error reading `{path}`: {detail}"
            ),
        }
    }
}

impl std::error::Error for ParseError {}
