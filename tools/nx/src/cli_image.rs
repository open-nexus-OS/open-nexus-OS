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
    /// Emit the QEMU fixture set (TASK-0179): trusted/untrusted/tampered/
    /// downgrade containers + the real-kernel os-B container, packed into
    /// an nxfs data-partition seed image under `/updates/`.
    Fixtures(ImageFixturesArgs),
    /// Arm a loader-backstop trial on a built disk (TASK-0289-B): write a
    /// boot-b image `nxboot` MUST reject (tampered payload behind a valid
    /// NXBD, or a validly-signed downgrade) and point the BSB at it.
    Backstop(ImageBackstopArgs),
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
    /// RFC-0089 §12 (TASK-0321): directory of `.nxb` bundle directories
    /// (`<dir>/<name>/{manifest.nxb,payload.elf,meta/}`) assembled into the
    /// pkgimg v3 system volume on `system-a` (signed NXSV, paired with
    /// boot-a). The volume bytes are also written next to `--out` as
    /// `<out>.system-a.pkgimg` for the image-budget gate.
    #[arg(long)]
    pub(crate) system_bundles: Option<PathBuf>,
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
    /// RFC-0089 §12: re-pair the system volume with the refreshed boot
    /// image (`--part boot-a` ⇒ `system-a`, `boot-b` ⇒ `system-b`) from
    /// this bundle directory — the keep-blk/flasher write shape, so a
    /// kept disk never carries a volume the new NXBD digest unpairs.
    #[arg(long)]
    pub(crate) system_bundles: Option<PathBuf>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Args, Debug)]
pub(crate) struct ImageOtaArgs {
    /// Flat boot image to package.
    #[arg(long)]
    pub(crate) kernel: PathBuf,
    /// RFC-0090: emit a `boot-image-delta` container (kind 3) whose
    /// payload is the deterministic `.nxdelta` stream from THIS base
    /// image to `--kernel`. The base must be the image the device is
    /// RUNNING (the stream binds to the active NXBD's digest).
    #[arg(long)]
    pub(crate) delta_from: Option<PathBuf>,
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
    /// RFC-0089 §12 (TASK-0321): bundle-set container — the components
    /// become `[boot-image(-delta), system-volume, bundle…]`, the volume
    /// built from this directory of `.nxb` bundle directories and paired
    /// with `--kernel` through the NXSV.
    #[arg(long)]
    pub(crate) bundle_set: Option<PathBuf>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Args, Debug)]
pub(crate) struct ImageFixturesArgs {
    /// Flat boot image for the REAL os-B container (the crown-proof flip
    /// boots these bytes — a build-id trailer sector makes them genuinely
    /// different from the running slot-A image).
    #[arg(long)]
    pub(crate) kernel: PathBuf,
    /// Output nxfs data-partition seed image (consumed by `image build
    /// --data`).
    #[arg(long)]
    pub(crate) data_out: PathBuf,
    /// Data partition size in MiB (must match the layout SSOT).
    #[arg(long, default_value_t = 128)]
    pub(crate) data_mib: u64,
    /// OS-image signing key seed (NXBDs).
    #[arg(long)]
    pub(crate) sign_os: PathBuf,
    /// Publisher signing key seed (manifests; the device anchor).
    #[arg(long)]
    pub(crate) sign_publisher: PathBuf,
    /// Build id of the RUNNING image (os-B derives `<id>-B`).
    #[arg(long)]
    pub(crate) build_id: String,
    /// TASK-0321 P3: bundle directories (`<dir>/<svc>/bundle.toml` +
    /// payloads) of the NEXT system volume; emits `bundle-set.nxs` =
    /// os-B + system-volume + bundles (RFC-0089 §12.4).
    #[arg(long)]
    pub(crate) system_bundles: Option<PathBuf>,
    #[arg(long)]
    pub(crate) json: bool,
}

#[derive(Args, Debug)]
pub(crate) struct ImageBackstopArgs {
    /// Built disk image to arm in place.
    #[arg(long, default_value = "build/nexus.img")]
    pub(crate) image: PathBuf,
    /// Backstop kind: `tamper` (valid NXBD, one payload byte flipped AFTER
    /// the descriptor landed → loader rejects `digest`) or `downgrade`
    /// (fully valid slot at rollback index 0, below the factory floor →
    /// loader rejects `rollback 0 < min <floor>`).
    #[arg(long)]
    pub(crate) kind: String,
    /// Flat boot image to plant in boot-b.
    #[arg(long)]
    pub(crate) kernel: PathBuf,
    /// OS-image signing key: 64 hex chars (Ed25519 seed).
    #[arg(long)]
    pub(crate) sign: PathBuf,
    /// Build id for the planted NXBD (distinct from the boot-a id so the
    /// uart names which image the loader rejected).
    #[arg(long)]
    pub(crate) build_id: String,
    #[arg(long)]
    pub(crate) json: bool,
}
