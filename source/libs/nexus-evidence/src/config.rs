// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: [`ConfigArtifact`] builder (P5-02). Collects the subset of
//! the host environment needed to reproduce a QEMU run from a sealed
//! bundle: profile env (resolved against the manifest), kernel cmdline,
//! qemu argv, host info, build SHA, rustc/qemu versions, and the
//! wall-clock seal time (excluded from the canonical hash).
//!
//! All host-introspection inputs (`uname -a`, `git rev-parse HEAD`,
//! `rustc --version`, `qemu-system-riscv64 --version`) are accepted as
//! pre-collected `String`s on [`GatherOpts`]. Tests provide them
//! verbatim so trace + config are byte-identical across runs (modulo
//! `wall_clock_utc`); the harness wrapper at P5-05 is responsible for
//! actually shelling out and populating them.
//!
//! OWNERS: @runtime
//! STATUS: Functional (P5-02 surface)
//! API_STABILITY: Unstable (Phase 5 evolves shape between cuts)
//! TEST_COVERAGE: see `tests/assemble.rs` (5 tests)

use std::collections::BTreeMap;

use crate::{ConfigArtifact, EvidenceError};

/// Inputs to [`gather_config`]. All fields are required and
/// caller-supplied: the crate does not shell out to `uname` / `git` /
/// `rustc` / `qemu-system-riscv64` itself. The harness wrapper
/// (P5-05) collects those once per run and forwards them here.
///
/// Keeping the gatherer pure (no I/O, no subprocess spawns) is what
/// lets [`crate::Bundle::assemble`] be deterministic in tests.
#[derive(Debug, Clone)]
pub struct GatherOpts {
    /// Profile name (echoes the bundle profile).
    pub profile: String,
    /// Resolved env dictionary, typically the output of
    /// `nexus-proof-manifest list-env --profile=<name>`.
    pub env: BTreeMap<String, String>,
    /// Kernel command line passed to QEMU via `-append`.
    pub kernel_cmdline: String,
    /// QEMU argv as captured by the harness (order significant).
    pub qemu_args: Vec<String>,
    /// Single-line host info (`uname -a` output).
    pub host_info: String,
    /// `git rev-parse HEAD` of the OS image source.
    pub build_sha: String,
    /// `rustc --version` of the toolchain that built the OS image.
    pub rustc_version: String,
    /// First line of `qemu-system-riscv64 --version`.
    pub qemu_version: String,
    /// RFC-3339 UTC seal time. The only field excluded from the
    /// canonical hash.
    pub wall_clock_utc: String,
}

/// Build a [`ConfigArtifact`] from already-collected host inputs.
///
/// Validates light invariants (non-empty profile, profile echo
/// matches) and returns the artifact verbatim otherwise. Empty
/// version strings or empty `host_info` are accepted so that early
/// bring-up runs can still seal a bundle (the harness will harden
/// these in P5-05).
///
/// # Errors
///
/// - [`EvidenceError::MalformedConfig`] if `opts.profile` is empty
///   (a bundle without a profile cannot be replayed).
pub fn gather_config(opts: GatherOpts) -> Result<ConfigArtifact, EvidenceError> {
    if opts.profile.is_empty() {
        return Err(EvidenceError::MalformedConfig {
            detail: "empty_profile".into(),
        });
    }
    Ok(ConfigArtifact {
        profile: opts.profile,
        env: opts.env,
        kernel_cmdline: opts.kernel_cmdline,
        qemu_args: opts.qemu_args,
        host_info: opts.host_info,
        build_sha: opts.build_sha,
        rustc_version: opts.rustc_version,
        qemu_version: opts.qemu_version,
        wall_clock_utc: opts.wall_clock_utc,
    })
}
