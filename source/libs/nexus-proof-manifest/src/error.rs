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
        }
    }
}

impl std::error::Error for ParseError {}
