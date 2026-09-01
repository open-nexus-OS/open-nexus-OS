// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: `clap` arg structs for `nx update` (TASK-0140, RFC-0089 §8/§9)
//! — split from `cli.rs` under the structure ratchet; re-exported there so
//! `crate::cli::Update*` paths stay stable. The verbs mirror the device
//! surface (`updated` v2 + bootctld status) but run OFFLINE against built
//! artifacts: there is no host↔guest transport, so `status`/`check` decode
//! the disk truth through the same SSOT crates the OS links, `stage` is
//! the RFC-0089 §9 provisioning drop into the data volume's `/updates/`,
//! and `switch`/`rollback` are preflights (`applied=false` in data — the
//! live transitions belong to `updated`/bootctld on the device).
//! OWNERS: @tools-team @reliability
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: tests/update_cli.rs (process boundary)
//! ADR: docs/rfcs/RFC-0089-ota-v2-component-manifest-ab-boot-images-nxboot-bsb.md

use clap::{Args, Subcommand};
use std::path::PathBuf;

/// `nx update …` — offline surfaces over the real OTA artifacts
/// (TASK-0140): disk-truth status, feed enumeration, provisioning drop,
/// and switch/rollback preflights. No update logic of its own.
#[derive(Args, Debug)]
pub(crate) struct UpdateArgs {
    #[command(subcommand)]
    pub(crate) action: UpdateAction,
}

#[derive(Subcommand, Debug)]
pub(crate) enum UpdateAction {
    /// Enumerate `/updates/*.nxs` on the data volume and verify each
    /// candidate through the device engine (feed truth: RFC-0089 §9).
    Check(UpdateCheckArgs),
    /// Verify one `.nxs` container and drop it into the data volume's
    /// `/updates/` (the offline acquisition step the device stages from).
    Stage(UpdateStageArgs),
    /// Preflight a slot switch against the disk truth (applied=false —
    /// the live switch is `updated` OP_SWITCH on the device).
    Switch(UpdateSwitchArgs),
    /// Decode slot/trial/floor truth from a built disk (BSB + NXBDs).
    Status(UpdateStatusArgs),
    /// Preflight a rollback request against the BSB projection
    /// (applied=false — the live rollback is a bootctld op).
    Rollback(UpdateRollbackArgs),
}

#[derive(Args, Debug)]
pub(crate) struct UpdateStatusArgs {
    /// Built GPT disk image.
    #[arg(long, default_value = "build/nexus.img")]
    pub(crate) image: PathBuf,
    /// Optional OS-image verifying key (64 hex chars) to check NXBD
    /// signatures; without it slot descriptors report `sig=unchecked`.
    #[arg(long, conflicts_with = "key")]
    pub(crate) pubkey: Option<String>,
    /// Seed file (64 hex chars) to derive the verifying key from.
    #[arg(long)]
    pub(crate) key: Option<PathBuf>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Args, Debug)]
pub(crate) struct UpdateCheckArgs {
    /// Built GPT disk image (feed = its `data` partition).
    #[arg(long, default_value = "build/nexus.img", conflicts_with = "data")]
    pub(crate) image: PathBuf,
    /// Standalone nxfs data image instead of a full disk (no floor gate).
    #[arg(long)]
    pub(crate) data: Option<PathBuf>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Args, Debug)]
pub(crate) struct UpdateStageArgs {
    /// `.nxs` v2 container to verify and drop into `/updates/`.
    pub(crate) container: PathBuf,
    /// Built GPT disk image (target = its `data` partition).
    #[arg(long, default_value = "build/nexus.img", conflicts_with = "data")]
    pub(crate) image: PathBuf,
    /// Standalone nxfs data image instead of a full disk (no floor gate).
    #[arg(long)]
    pub(crate) data: Option<PathBuf>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Args, Debug)]
pub(crate) struct UpdateSwitchArgs {
    /// Built GPT disk image.
    #[arg(long, default_value = "build/nexus.img")]
    pub(crate) image: PathBuf,
    /// Trial budget the live switch would arm.
    #[arg(long, default_value_t = 2)]
    pub(crate) tries: u8,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Args, Debug)]
pub(crate) struct UpdateRollbackArgs {
    /// Built GPT disk image.
    #[arg(long, default_value = "build/nexus.img")]
    pub(crate) image: PathBuf,
    #[arg(long)]
    pub(crate) json: bool,
}
