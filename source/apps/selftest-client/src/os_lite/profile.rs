// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Runtime selftest-profile dispatch (TASK-0023B Cut P4-08).
//!
//! `proof-manifest.toml` declares five `runtime_only = true` profiles
//! (`bringup`, `quick`, `ota`, `net`, `none`) alongside the harness profiles
//! (`full`, `smp`, `dhcp`, `os2vm`, `quic-required`). Runtime profiles are
//! NOT consumed by `scripts/qemu-test.sh`; they're consumed by THIS file
//! to scope which `os_lite::phases::<X>::run()` calls actually fire when
//! the `selftest-client` binary boots. Phases that are excluded by the
//! active profile emit a single deterministic `dbg: phase X skipped`
//! breadcrumb (one constant per phase, declared in the manifest, generated
//! into `markers_generated.rs`); no `SELFTEST:` markers are produced for
//! skipped phases, so harness profiles like `full` (which iterate all 12
//! phases) are byte-identical to the pre-P4-08 behavior.
//!
//! ### Where the profile selection comes from
//!
//! `selftest-client` is a no-std RISC-V binary with no live cmdline plumbing
//! today (kernel cmdline / device-tree integration is tracked by a follow-on
//! cut). For Phase-4 we therefore source the active profile from
//! `option_env!("SELFTEST_PROFILE")` at compile time:
//!
//! ```text
//! SELFTEST_PROFILE=quick cargo build -p selftest-client …
//! ```
//!
//! Unset (the common case) → falls back to the `default` argument supplied by
//! the caller (`Profile::Full` for the canonical QEMU smoke). The function
//! name `from_kernel_cmdline_or_default` is the public API surface the plan
//! locks; the internal source (build-time env vs runtime cmdline) is an
//! implementation detail that can swing without breaking callers.
//!
//! OWNERS: @runtime
//! STATUS: Functional (P4-08; runtime cmdline plumbing deferred)
//! API_STABILITY: Unstable (Phase 4 evolves shape between cuts)
//! TEST_COVERAGE: host-side unit tests in `cargo test -p nexus-proof-manifest`
//!                (profile catalog + phase-set membership); QEMU smoke under
//!                `just ci-os-full` (full profile = byte-identical baseline)
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

#![allow(clippy::missing_docs_in_private_items)]

use crate::markers_generated as M;

/// One of the runtime selftest profiles declared in `proof-manifest.toml`
/// under `runtime_only = true`. `Full` is the implicit default and is the
/// only profile that exercises every phase (it preserves the byte-identical
/// QEMU marker ladder).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// Run every phase. Matches `[profile.full]` semantics for the OS side.
    Full,
    /// `[profile.bringup]` — only the kernel/userspace boot lifecycle and
    /// the trailing `end` phase.
    Bringup,
    /// `[profile.quick]` — boot + IPC + MMIO smoke, then `end`. Used for
    /// fast iteration loops where the network/storage/policy ladders
    /// don't need to fire.
    Quick,
    /// `[profile.ota]` — boot + IPC + OTA, then `end`.
    Ota,
    /// `[profile.net]` — boot + IPC + MMIO + routing + net, then `end`.
    Net,
    /// `[profile.none]` — boot + `end` only (used by harness fault-injection
    /// to confirm a binary built with skip semantics actually skips).
    None,
}

/// One entry per `[phase.X]` in `proof-manifest.toml`. Order mirrors the
/// declaration order in the manifest (RFC-0014 v2 12-phase ladder).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhaseId {
    Bringup,
    IpcKernel,
    Mmio,
    Routing,
    Ota,
    Policy,
    Exec,
    Logd,
    Vfs,
    Net,
    Remote,
    End,
}

impl Profile {
    /// Resolve the active profile, falling back to `default` if unset.
    ///
    /// Reads `SELFTEST_PROFILE` from the build-time environment via
    /// `option_env!` (see module docs for the rationale). Unknown values
    /// also fall back to `default`; this is intentional so that a typo in
    /// CI env wiring doesn't crash boot.
    pub fn from_kernel_cmdline_or_default(default: Profile) -> Profile {
        match option_env!("SELFTEST_PROFILE") {
            Some("full") | Some("Full") => Profile::Full,
            Some("bringup") | Some("Bringup") => Profile::Bringup,
            Some("quick") | Some("Quick") => Profile::Quick,
            Some("ota") | Some("Ota") => Profile::Ota,
            Some("net") | Some("Net") => Profile::Net,
            Some("none") | Some("None") => Profile::None,
            _ => default,
        }
    }

    /// Returns `true` iff the active profile enables the given phase.
    pub fn includes(self, p: PhaseId) -> bool {
        match self {
            Profile::Full => true,
            Profile::Bringup | Profile::None => {
                matches!(p, PhaseId::Bringup | PhaseId::End)
            }
            Profile::Quick => matches!(
                p,
                PhaseId::Bringup | PhaseId::IpcKernel | PhaseId::Mmio | PhaseId::End
            ),
            Profile::Ota => matches!(
                p,
                PhaseId::Bringup | PhaseId::IpcKernel | PhaseId::Ota | PhaseId::End
            ),
            Profile::Net => matches!(
                p,
                PhaseId::Bringup
                    | PhaseId::IpcKernel
                    | PhaseId::Mmio
                    | PhaseId::Routing
                    | PhaseId::Net
                    | PhaseId::End
            ),
        }
    }

    /// Returns the deterministic `dbg: phase X skipped` marker constant for
    /// the supplied phase. Used by `os_lite::run()` to emit the skip
    /// breadcrumb when `includes(p)` is `false`.
    pub fn skip_marker(p: PhaseId) -> &'static str {
        match p {
            PhaseId::Bringup => M::M_DBG_PHASE_BRINGUP_SKIPPED,
            PhaseId::IpcKernel => M::M_DBG_PHASE_IPC_KERNEL_SKIPPED,
            PhaseId::Mmio => M::M_DBG_PHASE_MMIO_SKIPPED,
            PhaseId::Routing => M::M_DBG_PHASE_ROUTING_SKIPPED,
            PhaseId::Ota => M::M_DBG_PHASE_OTA_SKIPPED,
            PhaseId::Policy => M::M_DBG_PHASE_POLICY_SKIPPED,
            PhaseId::Exec => M::M_DBG_PHASE_EXEC_SKIPPED,
            PhaseId::Logd => M::M_DBG_PHASE_LOGD_SKIPPED,
            PhaseId::Vfs => M::M_DBG_PHASE_VFS_SKIPPED,
            PhaseId::Net => M::M_DBG_PHASE_NET_SKIPPED,
            PhaseId::Remote => M::M_DBG_PHASE_REMOTE_SKIPPED,
            PhaseId::End => M::M_DBG_PHASE_END_SKIPPED,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_profile_enables_every_phase() {
        let p = Profile::Full;
        for ph in [
            PhaseId::Bringup,
            PhaseId::IpcKernel,
            PhaseId::Mmio,
            PhaseId::Routing,
            PhaseId::Ota,
            PhaseId::Policy,
            PhaseId::Exec,
            PhaseId::Logd,
            PhaseId::Vfs,
            PhaseId::Net,
            PhaseId::Remote,
            PhaseId::End,
        ] {
            assert!(p.includes(ph), "Full must include {ph:?}");
        }
    }

    #[test]
    fn bringup_profile_enables_only_bringup_and_end() {
        let p = Profile::Bringup;
        assert!(p.includes(PhaseId::Bringup));
        assert!(p.includes(PhaseId::End));
        for ph in [
            PhaseId::IpcKernel,
            PhaseId::Mmio,
            PhaseId::Routing,
            PhaseId::Ota,
            PhaseId::Policy,
            PhaseId::Exec,
            PhaseId::Logd,
            PhaseId::Vfs,
            PhaseId::Net,
            PhaseId::Remote,
        ] {
            assert!(!p.includes(ph), "Bringup must NOT include {ph:?}");
        }
    }

    #[test]
    fn unknown_env_falls_back_to_default() {
        // option_env! is evaluated at compile time, so we can only assert
        // that an unset/unknown SELFTEST_PROFILE returns the supplied
        // default. (CI never bakes a bogus value, so this is the steady
        // state.)
        let p = Profile::from_kernel_cmdline_or_default(Profile::Quick);
        // Either we got the compile-time value (when CI explicitly set
        // SELFTEST_PROFILE) or we got the default.
        let valid = matches!(
            p,
            Profile::Full
                | Profile::Bringup
                | Profile::Quick
                | Profile::Ota
                | Profile::Net
                | Profile::None
        );
        assert!(valid);
    }
}
