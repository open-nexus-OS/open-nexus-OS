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
//! `selftest-client` now resolves its active profile from a runtime boot-config
//! surface in QEMU `fw_cfg` when present:
//!
//! ```text
//! -fw_cfg name=opt/org.open-nexus/selftest-profile,string=bringup
//! ```
//!
//! This keeps the `make build -> make run` artifact chain valid because the
//! start mode no longer requires a rebuild. For compatibility, a compile-time
//! `SELFTEST_PROFILE` still acts as a legacy fallback when no runtime override
//! is provided.
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
use crate::os_lite::boot_cfg;
use crate::runtime_mode::{RuntimeMode, RuntimeProfile};

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
    /// Resolution order:
    /// 1. proof mode → legacy/`default` (the full ladder; keeps `verify-uart` byte-stable)
    /// 2. runtime `fw_cfg` profile override (interactive boots only)
    /// 3. runtime mode (`full` for `interactive-full`, `bringup` for `interactive-minimal`)
    /// 4. legacy build-time `SELFTEST_PROFILE`
    /// 5. caller-provided `default`
    ///
    /// Unknown values fall back to `default`; this is intentional so that a
    /// typo in boot wiring does not crash early boot.
    pub fn from_kernel_cmdline_or_default(default: Profile) -> Profile {
        let legacy = match option_env!("SELFTEST_PROFILE") {
            Some("full") | Some("Full") => Some(Profile::Full),
            Some("bringup") | Some("Bringup") => Some(Profile::Bringup),
            Some("quick") | Some("Quick") => Some(Profile::Quick),
            Some("ota") | Some("Ota") => Some(Profile::Ota),
            Some("net") | Some("Net") => Some(Profile::Net),
            Some("none") | Some("None") => Some(Profile::None),
            _ => None,
        };
        Self::resolve(
            boot_cfg::runtime_profile_with_retry(),
            boot_cfg::runtime_mode_with_retry(),
            legacy,
            default,
        )
    }

    /// Returns `true` iff the active profile enables the given phase.
    pub fn includes(self, p: PhaseId) -> bool {
        match self {
            Profile::Full => true,
            Profile::Bringup | Profile::None => {
                matches!(p, PhaseId::Bringup | PhaseId::End)
            }
            Profile::Quick => {
                matches!(p, PhaseId::Bringup | PhaseId::IpcKernel | PhaseId::Mmio | PhaseId::End)
            }
            Profile::Ota => {
                matches!(p, PhaseId::Bringup | PhaseId::IpcKernel | PhaseId::Ota | PhaseId::End)
            }
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

    fn resolve(
        runtime_profile: Option<RuntimeProfile>,
        runtime_mode: Option<RuntimeMode>,
        legacy_profile: Option<Profile>,
        default: Profile,
    ) -> Profile {
        // Proof boots ALWAYS run the full ladder. The proof harness keys its marker
        // expectation (`verify-uart list-markers --profile=<harness>`) on the HARNESS profile,
        // NOT this runtime knob, so scoping the runtime here would desync the two and fail
        // verification. The runtime `fw_cfg` profile therefore only scopes INTERACTIVE boots;
        // until the observer consumes the runtime profile (Phase 4) proof stays byte-stable.
        if matches!(runtime_mode, Some(RuntimeMode::Proof)) {
            return legacy_profile.unwrap_or(default);
        }
        // Interactive (or unknown) boots: an explicit `fw_cfg` profile wins for ad-hoc scoping…
        if let Some(profile) = runtime_profile {
            return match profile {
                RuntimeProfile::Full => Profile::Full,
                RuntimeProfile::Bringup => Profile::Bringup,
                RuntimeProfile::Quick => Profile::Quick,
                RuntimeProfile::Ota => Profile::Ota,
                RuntimeProfile::Net => Profile::Net,
                RuntimeProfile::None => Profile::None,
            };
        }
        // …otherwise the interactive mode is the single source of truth for the phase scope:
        // `interactive-full` runs the whole ladder (folded into verdicts), `interactive-minimal`
        // runs only bring-up. `just start` passes no profile, so the mode alone decides.
        match runtime_mode {
            Some(RuntimeMode::InteractiveFull) => Profile::Full,
            Some(RuntimeMode::InteractiveMinimal) => Profile::Bringup,
            _ => legacy_profile.unwrap_or(default),
        }
    }
}

/// True when this is an interactive boot (`just start`) — the human wants the aggregated
/// `group N/N OK` verdict view, not the full per-marker proof ladder. The proof harness
/// (`just test-os`) returns false, keeping the full deterministic marker stream for `verify-uart`.
pub(crate) fn runtime_is_interactive() -> bool {
    matches!(
        boot_cfg::runtime_mode_with_retry(),
        Some(RuntimeMode::InteractiveMinimal | RuntimeMode::InteractiveFull)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_mode::{RuntimeMode, RuntimeProfile};

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
        assert_eq!(Profile::resolve(None, None, None, Profile::Quick), Profile::Quick);
    }

    #[test]
    fn interactive_mode_selects_profile_by_scope() {
        // `interactive-minimal` → bring-up only (even over a legacy `full`).
        assert_eq!(
            Profile::resolve(
                None,
                Some(RuntimeMode::InteractiveMinimal),
                Some(Profile::Full),
                Profile::Quick
            ),
            Profile::Bringup
        );
        // `interactive-full` → the whole ladder (folded into verdicts at runtime).
        assert_eq!(
            Profile::resolve(None, Some(RuntimeMode::InteractiveFull), None, Profile::Quick),
            Profile::Full
        );
    }

    #[test]
    fn proof_mode_pins_to_full_ignoring_runtime_profile() {
        // The proof harness may pass a narrow `fw_cfg` profile (its default is `bringup`), but in
        // proof mode the runtime MUST still run the full ladder so the marker stream stays
        // byte-stable against the harness-keyed `verify-uart` expectation.
        assert_eq!(
            Profile::resolve(
                Some(RuntimeProfile::Bringup),
                Some(RuntimeMode::Proof),
                None,
                Profile::Full
            ),
            Profile::Full
        );
        assert_eq!(
            Profile::resolve(Some(RuntimeProfile::None), Some(RuntimeMode::Proof), None, Profile::Full),
            Profile::Full
        );
    }

    #[test]
    fn explicit_runtime_profile_wins_over_mode_and_legacy() {
        assert_eq!(
            Profile::resolve(
                Some(RuntimeProfile::Net),
                Some(RuntimeMode::InteractiveFull),
                Some(Profile::Quick),
                Profile::Full
            ),
            Profile::Net
        );
    }
}
