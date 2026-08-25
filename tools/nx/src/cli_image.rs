// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: `clap` arg structs for `nx image build/verify/patch/ota`
//! (TASK-0260, RFC-0089) — split from `cli.rs` under the structure
//! ratchet; re-exported there so `crate::cli::Image*` paths stay stable.
//! OWNERS: @tools-team @reliability
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: tests/image_cli.rs (process boundary)
//! ADR: docs/adr/0058-boot-selection-block-dual-actor-discipline.md

use clap::{Args, Subcommand};
use std::path::PathBuf;

/// `nx image …` — the deterministic RFC-0089 disk/OTA artifact authority
/// (TASK-0260): GPT assembler + NXBD signer + factory BSB + `.nxs` v2
/// container emission.
#[derive(Args, Debug)]
pub(crate) struct ImageArgs {
    #[command(subcommand)]
    pub(crate) action: ImageAction,
}

#[derive(Subcommand, Debug)]
pub(crate) enum ImageAction {
    /// Assemble the full GPT disk (`build/nexus.img` by default).
    Build(ImageBuildArgs),
    /// Re-parse + verify a built disk (GPT, BSB, NXBD signature, digest).
    Verify(ImageVerifyArgs),
    /// Refresh ONE boot partition without touching bsb/state/data.
    Patch(ImagePatchArgs),
    /// Emit the `.nxs` v2 OTA container for a boot image.
    Ota(ImageOtaArgs),
}

#[derive(Args, Debug)]
pub(crate) struct ImageBuildArgs {
    /// Flat boot image (neuron-boot.bin).
    #[arg(long)]
    pub(crate) kernel: PathBuf,
    /// Output disk image.
    #[arg(short, long, default_value = "build/nexus.img")]
    pub(crate) out: PathBuf,
    /// OS-image signing key: 64 hex chars (Ed25519 seed).
    #[arg(long)]
    pub(crate) sign: PathBuf,
    /// Deterministic build id (ASCII, ≤ 32 chars; printed at boot).
    #[arg(long)]
    pub(crate) build_id: String,
    /// Anti-downgrade index for this build (RFC-0089 §10).
    #[arg(long, default_value_t = 0)]
    pub(crate) rollback_index: u32,
    /// Optional statefs journal seed image (copied into `state`).
    #[arg(long)]
    pub(crate) state: Option<PathBuf>,
    /// Optional nxfs seed image (copied into `data`).
    #[arg(long)]
    pub(crate) data: Option<PathBuf>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Args, Debug)]
pub(crate) struct ImageVerifyArgs {
    /// Disk image to verify.
    #[arg(long, default_value = "build/nexus.img")]
    pub(crate) image: PathBuf,
    /// Expected OS-image verifying key (64 hex chars pubkey) OR a seed
    /// file via --key.
    #[arg(long, conflicts_with = "key")]
    pub(crate) pubkey: Option<String>,
    /// Seed file (64 hex chars) to derive the verifying key from.
    #[arg(long)]
    pub(crate) key: Option<PathBuf>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Args, Debug)]
pub(crate) struct ImagePatchArgs {
    /// Disk image to patch in place.
    #[arg(long, default_value = "build/nexus.img")]
    pub(crate) image: PathBuf,
    /// Target boot partition: boot-a | boot-b.
    #[arg(long)]
    pub(crate) part: String,
    /// Flat boot image to write.
    #[arg(long)]
    pub(crate) kernel: PathBuf,
    /// OS-image signing key: 64 hex chars (Ed25519 seed).
    #[arg(long)]
    pub(crate) sign: PathBuf,
    /// Build id for the refreshed NXBD.
    #[arg(long)]
    pub(crate) build_id: String,
    /// Anti-downgrade index for the refreshed NXBD.
    #[arg(long, default_value_t = 0)]
    pub(crate) rollback_index: u32,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Args, Debug)]
pub(crate) struct ImageOtaArgs {
    /// Flat boot image to package.
    #[arg(long)]
    pub(crate) kernel: PathBuf,
    /// Output `.nxs` v2 container path.
    #[arg(short, long)]
    pub(crate) out: PathBuf,
    /// Publisher signing key (64 hex chars seed) — must be in the DEVICE
    /// anchor (policies/update-trust.toml) for the container to stage.
    #[arg(long)]
    pub(crate) sign_publisher: PathBuf,
    /// OS-image signing key (64 hex chars seed) for the embedded NXBD.
    #[arg(long)]
    pub(crate) sign_os: PathBuf,
    /// Deterministic build id (ASCII, ≤ 32 chars).
    #[arg(long)]
    pub(crate) build_id: String,
    /// Anti-downgrade index carried by manifest AND NXBD (must match).
    #[arg(long, default_value_t = 0)]
    pub(crate) rollback_index: u32,
    #[arg(long)]
    pub(crate) json: bool,
}
