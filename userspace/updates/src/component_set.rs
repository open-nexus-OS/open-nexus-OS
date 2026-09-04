// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `.nxs` v2 component-manifest verify plus apply core
//! (RFC-0089 sections 3 and 8; TASK-0179). One pass over a BORROWED
//! container slice (the staging source maps the bytes; nothing here copies
//! a payload): deterministic tar walk (manifest.nxo first,
//! manifest.sig.ed25519 second, payload entries in manifest order, unknown
//! extras reject), then the device-anchor signature (the in-archive
//! publisher id is a lookup HINT, never a trust input), stage-time
//! anti-downgrade, per-component 64-KiB streaming into a [`ComponentSink`]
//! with incremental sha256, and finally the kind checks (v1 dispatch:
//! exactly `boot-image`; the embedded NXBD must bind to the manifest
//! field-for-field). The sink owns the commit point: body bytes may land
//! during streaming, the descriptor lands ONLY in `finish` after the digest
//! matched (NXBD-last discipline, so a torn stage never yields a bootable
//! half-slot).
//! OWNERS: @runtime @security
//! STATUS: Experimental (TASK-0179)
//! PUBLIC API: verify_and_apply(), ComponentSink, ManifestV2, RejectReason
//! TEST_COVERAGE: tests/updates_host/tests/component_set.rs (accept +
//!   reject matrix + power-cut/idempotency against a block-fake sink)
//! ADR: docs/rfcs/RFC-0089-ota-v2-component-manifest-ab-boot-images-nxboot-bsb.md

#[cfg(all(feature = "os-lite", not(feature = "std")))]
use alloc::{string::String, string::ToString, vec::Vec};
#[cfg(feature = "std")]
use std::{string::String, string::ToString, vec::Vec};

use capnp::message::ReaderOptions;
use capnp::serialize;
use sha2::{Digest, Sha256};

use crate::system_set::SignatureVerifier;

/// Size bounds (RFC-0089 §3, enforced before allocation).
pub const MAX_NXS_ARCHIVE_BYTES: usize = 100 * 1024 * 1024;
pub const MAX_MANIFEST_BYTES: usize = 1024 * 1024;
pub const MAX_COMPONENTS_PER_SET: usize = 256;
/// Streaming chunk (the pipeline's bounded unit of work).
pub const APPLY_CHUNK_BYTES: usize = 64 * 1024;

/// Component kinds (RFC-0089 §3). Kinds 2/4/6 are the Phase B seam
/// (§12, TASK-0321/0035): emitted by `nx image ota --bundle-set` since P1,
/// accepted by the device engine from P3 — until then they reject as
/// `component kind unsupported` (never skipped).
pub const KIND_BOOT_IMAGE: u8 = 1;
/// One bundle's data-region window of the system volume (§12.4).
pub const KIND_BUNDLE: u8 = 2;
/// `.nxdelta` per bundle, base = the bundle window in the ACTIVE volume.
pub const KIND_BUNDLE_DELTA: u8 = 4;
/// pkgimg v3 superblock + index; `kind_data` = the signed NXSV (§12.2).
pub const KIND_SYSTEM_VOLUME: u8 = 6;
/// RFC-0090: target reconstructed from the ACTIVE slot bytes + a
/// `.nxdelta` stream. The component's `size`/`sha256` describe the DELTA
/// STREAM (signature-bound payload); target truth rides the NXBD in
/// `kind_data` and is enforced by the sink's readback gate.
pub const KIND_BOOT_IMAGE_DELTA: u8 = 3;

/// Stable reject vocabulary (RFC-0089 §8 + RFC-0090) — `label()` is the
/// marker/audit string, never reworded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RejectReason {
    UntrustedPublisher,
    Sig,
    Digest,
    Bounds,
    Path,
    KindUnsupported,
    Downgrade,
    Io,
    SlotActive,
    /// RFC-0090: malformed `.nxdelta` stream (bad header/tag/bounds/END).
    DeltaFormat,
    /// RFC-0090: the stream's base binding does not match the ACTIVE
    /// slot's loader-verified NXBD (digest or size).
    DeltaBase,
    /// RFC-0089 §12.4: component ordering violated (`[boot-image|delta]?,
    /// system-volume, (bundle|bundle-delta)*`; one volume per set).
    Order,
    /// RFC-0089 §12.4: the system-volume component does not bind (NXSV
    /// malformed, index size/digest mismatch, build/rollback mismatch, or
    /// the volume is not paired with the set's boot image).
    VolumeBinding,
    /// RFC-0089 §12.4: a `bundle` component is not a window of the verified
    /// index (digest/size), or a reused bundle is missing from the ACTIVE volume.
    BundleNotInIndex,
    /// RFC-0089 §12.4: the assembled volume (or a reused window) does not
    /// hash to the NXSV/index digest at the set commit.
    VolumeDigest,
}

impl RejectReason {
    pub fn label(self) -> &'static str {
        match self {
            Self::UntrustedPublisher => "untrusted publisher",
            Self::Sig => "sig",
            Self::Digest => "digest",
            Self::Bounds => "bounds",
            Self::Path => "path",
            Self::KindUnsupported => "component kind unsupported",
            Self::Downgrade => "downgrade",
            Self::Io => "io",
            Self::SlotActive => "slot-active",
            Self::DeltaFormat => "delta-format",
            Self::DeltaBase => "delta-base",
            Self::Order => "order",
            Self::VolumeBinding => "volume-binding",
            Self::BundleNotInIndex => "bundle-not-in-index",
            Self::VolumeDigest => "volume-digest",
        }
    }
}

/// Decoded manifest header (components carry their payload windows).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestV2 {
    pub build_id: String,
    pub rollback_index: u32,
    pub component_count: usize,
    /// Kind of the FIRST component (v1 sets carry exactly one) — lets the
    /// stage marker name what was actually verified (boot-image vs
    /// boot-image-delta) instead of assuming.
    pub component_kind: u8,
}

/// One component's metadata as the sink sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentMeta {
    pub kind: u8,
    pub name: String,
    pub size: u64,
    pub sha256: [u8; 32],
    /// Kind-specific descriptor (boot-image: the signed 512-byte NXBD,
    /// written VERBATIM at the commit point — the device never re-signs).
    pub kind_data: Vec<u8>,
}

/// Destination of verified component bytes. The sink owns the commit
/// point: `finish` runs ONLY after the streamed digest matched.
pub trait ComponentSink {
    /// Called once per component before any byte. Must invalidate the
    /// destination's commit point (e.g. zero the slot's NXBD sector) and
    /// may reject (`SlotActive`, `Bounds`, `Io`).
    fn begin(&mut self, meta: &ComponentMeta) -> Result<(), RejectReason>;
    /// Sequential payload bytes (`offset` is the running byte offset).
    fn chunk(&mut self, offset: u64, bytes: &[u8]) -> Result<(), RejectReason>;
    /// Digest verified — perform readback verification and write the
    /// descriptor LAST.
    fn finish(&mut self, meta: &ComponentMeta) -> Result<(), RejectReason>;
    /// RFC-0089 §12.4 set commit: called ONCE after every component
    /// finished. A volume sink copies the index bundles the set did not
    /// ship from the ACTIVE volume (digest-checked), readback-verifies the
    /// whole volume and lands the NXSV LAST. Default: nothing to commit
    /// (boot slots commit per component).
    fn commit_set(&mut self) -> Result<(), RejectReason> {
        Ok(())
    }
}

struct TarView<'a> {
    name: &'a str,
    data: &'a [u8],
}

/// Forensic snapshot of a container's signing inputs: manifest length,
/// manifest digest prefix, signature digest prefix, publisher hint. When a
/// `sig` reject happens the ONLY useful question is "which of the inputs
/// differs from the one that signed it" — this makes that answerable from
/// a uart line instead of a debugger.
pub type SigningInputs = (usize, [u8; 4], [u8; 4], [u8; 8]);

pub fn describe(container: &[u8]) -> Option<SigningInputs> {
    let entries = walk_tar(container).ok()?;
    if entries.len() < 2 || entries[0].name != "manifest.nxo" {
        return None;
    }
    let manifest = entries[0].data;
    let sig = entries[1].data;
    // DIGESTS, not prefixes: a 4-byte prefix matched on both sides while
    // the messages still differed — the prefix proved nothing.
    let mut manifest_prefix = [0u8; 4];
    manifest_prefix.copy_from_slice(&sha256(manifest)[..4]);
    let mut sig_prefix = [0u8; 4];
    sig_prefix.copy_from_slice(&sha256(sig)[..4]);
    let (header, _components) = decode_manifest(manifest).ok()?;
    Some((manifest.len(), manifest_prefix, sig_prefix, header.publisher_hint))
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let mut out = [0u8; 32];
    out.copy_from_slice(&hasher.finalize());
    out
}

/// Verifies the container and applies every component through `sink`.
/// `floor` is the persisted anti-downgrade minimum (RFC-0089 §10 stage
/// rung). `yield_hook` runs between chunks (cooperative scheduling).
pub fn verify_and_apply(
    container: &[u8],
    verifier: &dyn SignatureVerifier,
    trust: &[[u8; 32]],
    floor: u32,
    sink: &mut dyn ComponentSink,
    yield_hook: &mut dyn FnMut(),
) -> Result<ManifestV2, RejectReason> {
    if container.len() > MAX_NXS_ARCHIVE_BYTES {
        return Err(RejectReason::Bounds);
    }
    let entries = walk_tar(container)?;
    if entries.len() < 2 {
        return Err(RejectReason::Path);
    }
    if entries[0].name != "manifest.nxo" || entries[1].name != "manifest.sig.ed25519" {
        return Err(RejectReason::Path);
    }
    let manifest_bytes = entries[0].data;
    if manifest_bytes.len() > MAX_MANIFEST_BYTES {
        return Err(RejectReason::Bounds);
    }
    // A wrong-length signature entry is a STRUCTURAL fault, not a failed
    // crypto check — keeping them distinct is what makes the reject marker
    // diagnostic instead of a guess.
    if entries[1].data.len() != 64 {
        return Err(RejectReason::Bounds);
    }
    let mut signature = [0u8; 64];
    signature.copy_from_slice(entries[1].data);

    let (manifest, components) = decode_manifest(manifest_bytes)?;

    // Trust: the manifest's 8-byte publisher id only SELECTS an anchor key;
    // the signature must verify against the selected full device key.
    let anchor = trust
        .iter()
        .find(|key| key[..8] == manifest.publisher_hint)
        .ok_or(RejectReason::UntrustedPublisher)?;
    // `sig` means the CRYPTO verdict was negative. Backend/transport
    // trouble is `io`, a bad key is `untrusted publisher` — collapsing all
    // three into `sig` blamed the publisher for a broken verifier hop and
    // cost several QEMU round trips of chasing bytes that were never wrong.
    verifier.verify_ed25519(anchor, manifest_bytes, &signature).map_err(|err| match err {
        crate::system_set::VerifyError::InvalidSignature => RejectReason::Sig,
        crate::system_set::VerifyError::InvalidKey => RejectReason::UntrustedPublisher,
        crate::system_set::VerifyError::Backend(_) => RejectReason::Io,
    })?;

    // Stage-time anti-downgrade (§10) — before any byte moves.
    if manifest.rollback_index < floor {
        return Err(RejectReason::Downgrade);
    }

    // Payload entries: exactly the manifest's components, in order, no
    // extras (deterministic container rule).
    if entries.len() - 2 != components.len() {
        return Err(RejectReason::Path);
    }
    for (entry, comp) in entries[2..].iter().zip(components.iter()) {
        if entry.name != comp.payload_path {
            return Err(RejectReason::Path);
        }
    }

    // §12.4 ordering (checked over the WHOLE set before any byte moves).
    check_order(components.iter().map(|c| c.meta.kind))?;
    // The set's boot-image digest (from its NXBD) pairs the system volume.
    let mut boot_digest: Option<[u8; 32]> = None;
    for (entry, comp) in entries[2..].iter().zip(components.iter()) {
        if entry.data.len() as u64 != comp.meta.size || comp.meta.size == 0 {
            return Err(RejectReason::Bounds);
        }
        // Per-kind binding checks (§3 step 4) BEFORE any byte moves.
        match comp.meta.kind {
            KIND_BOOT_IMAGE => {
                check_boot_image_binding(&manifest, &comp.meta)?;
                boot_digest = Some(comp.meta.sha256);
            }
            KIND_BOOT_IMAGE_DELTA => {
                check_boot_image_delta_binding(&manifest, &comp.meta)?;
                boot_digest = nxbd_image_digest(&comp.meta);
            }
            KIND_SYSTEM_VOLUME => check_system_volume_binding(&manifest, &comp.meta, boot_digest)?,
            KIND_BUNDLE => check_bundle_binding(&comp.meta)?,
            _ => return Err(RejectReason::KindUnsupported),
        }

        sink.begin(&comp.meta)?;
        let mut hasher = Sha256::new();
        let mut offset = 0u64;
        for chunk in entry.data.chunks(APPLY_CHUNK_BYTES) {
            hasher.update(chunk);
            sink.chunk(offset, chunk)?;
            offset += chunk.len() as u64;
            yield_hook();
        }
        let mut digest = [0u8; 32];
        digest.copy_from_slice(&hasher.finalize());
        if digest != comp.meta.sha256 {
            return Err(RejectReason::Digest);
        }
        sink.finish(&comp.meta)?;
    }
    // §12.4 set commit (volume sinks: reuse + readback + NXSV last).
    sink.commit_set()?;

    Ok(ManifestV2 {
        build_id: manifest.build_id,
        rollback_index: manifest.rollback_index,
        component_count: components.len(),
        component_kind: components.first().map_or(0, |c| c.meta.kind),
    })
}

struct ManifestHeader {
    publisher_hint: [u8; 8],
    build_id: String,
    rollback_index: u32,
}

struct DecodedComponent {
    meta: ComponentMeta,
    payload_path: String,
}

fn decode_manifest(bytes: &[u8]) -> Result<(ManifestHeader, Vec<DecodedComponent>), RejectReason> {
    let mut slice = bytes;
    let message =
        serialize::read_message_from_flat_slice_no_alloc(&mut slice, ReaderOptions::new())
            .map_err(|_| RejectReason::Bounds)?;
    let root = message
        .get_root::<crate::system_set_capnp::component_manifest::Reader<'_>>()
        .map_err(|_| RejectReason::Bounds)?;
    if root.get_schema_version() != 2 {
        return Err(RejectReason::Bounds);
    }
    let hint_raw = root.get_publisher_key_id().map_err(|_| RejectReason::Bounds)?;
    if hint_raw.len() != 8 {
        return Err(RejectReason::Bounds);
    }
    let mut publisher_hint = [0u8; 8];
    publisher_hint.copy_from_slice(hint_raw);
    let build_id = root
        .get_build_id()
        .and_then(|t| t.to_str().map_err(capnp::Error::from))
        .map_err(|_| RejectReason::Bounds)?
        .to_string();
    if build_id.is_empty() || build_id.len() > 32 {
        return Err(RejectReason::Bounds);
    }
    let rollback_index = root.get_rollback_index();

    let list = root.get_components().map_err(|_| RejectReason::Bounds)?;
    if list.len() as usize > MAX_COMPONENTS_PER_SET || list.is_empty() {
        return Err(RejectReason::Bounds);
    }
    let mut components = Vec::with_capacity(list.len() as usize);
    for comp in list.iter() {
        let sha_raw = comp.get_sha256().map_err(|_| RejectReason::Bounds)?;
        if sha_raw.len() != 32 {
            return Err(RejectReason::Bounds);
        }
        let mut sha256 = [0u8; 32];
        sha256.copy_from_slice(sha_raw);
        let name = comp
            .get_name()
            .and_then(|t| t.to_str().map_err(capnp::Error::from))
            .map_err(|_| RejectReason::Bounds)?
            .to_string();
        let payload_path = comp
            .get_payload_path()
            .and_then(|t| t.to_str().map_err(capnp::Error::from))
            .map_err(|_| RejectReason::Bounds)?
            .to_string();
        if payload_path.is_empty() || payload_path.len() > 100 {
            return Err(RejectReason::Path);
        }
        let kind_data = comp.get_kind_data().map_err(|_| RejectReason::Bounds)?.to_vec();
        components.push(DecodedComponent {
            meta: ComponentMeta {
                kind: comp.get_kind(),
                name,
                size: comp.get_size(),
                sha256,
                kind_data,
            },
            payload_path,
        });
    }
    Ok((ManifestHeader { publisher_hint, build_id, rollback_index }, components))
}

/// boot-image binding (§3 kind checks + §5): kind_data is a well-FORMED
/// 512-byte NXBD whose fields match the manifest. Its SIGNATURE is the
/// loader's to verify (different anchor); a structural or binding
/// mismatch here is a dishonest container.
fn check_boot_image_binding(
    manifest: &ManifestHeader,
    meta: &ComponentMeta,
) -> Result<(), RejectReason> {
    let (nxbd, _sig) = bootfmt::nxbd::decode(&meta.kind_data).map_err(|_| RejectReason::Bounds)?;
    if nxbd.image_sha256 != meta.sha256 {
        return Err(RejectReason::Digest);
    }
    if nxbd.image_size != meta.size
        || nxbd.build_id_str() != manifest.build_id
        || nxbd.rollback_index != manifest.rollback_index
    {
        return Err(RejectReason::Bounds);
    }
    Ok(())
}

/// boot-image-delta binding (RFC-0090): the NXBD still binds
/// build/rollback to the manifest, but `meta.size`/`meta.sha256` describe
/// the DELTA STREAM — the kind-1 `image_sha256 == sha256` rule does NOT
/// apply (target and stream digests legitimately differ). Target truth is
/// enforced downstream by the sink's readback gate against this NXBD.
fn check_boot_image_delta_binding(
    manifest: &ManifestHeader,
    meta: &ComponentMeta,
) -> Result<(), RejectReason> {
    let (nxbd, _sig) = bootfmt::nxbd::decode(&meta.kind_data).map_err(|_| RejectReason::Bounds)?;
    if nxbd.image_size == 0
        || nxbd.build_id_str() != manifest.build_id
        || nxbd.rollback_index != manifest.rollback_index
    {
        return Err(RejectReason::Bounds);
    }
    Ok(())
}

/// §12.4 ordering: `[boot-image | boot-image-delta]?` then at most ONE
/// `system-volume`, then `(bundle | bundle-delta)*`; a bundle without a
/// preceding volume, a second volume, or a boot image after the volume
/// is `order`. Unknown kinds fall through to the per-kind dispatch.
fn check_order(kinds: impl Iterator<Item = u8>) -> Result<(), RejectReason> {
    let mut seen_boot = false;
    let mut seen_volume = false;
    let mut seen_bundle = false;
    for kind in kinds {
        match kind {
            KIND_BOOT_IMAGE | KIND_BOOT_IMAGE_DELTA => {
                if seen_boot || seen_volume || seen_bundle {
                    return Err(RejectReason::Order);
                }
                seen_boot = true;
            }
            KIND_SYSTEM_VOLUME => {
                if seen_volume || seen_bundle {
                    return Err(RejectReason::Order);
                }
                seen_volume = true;
            }
            KIND_BUNDLE | KIND_BUNDLE_DELTA => {
                if !seen_volume {
                    return Err(RejectReason::Order);
                }
                seen_bundle = true;
            }
            _ => {}
        }
    }
    Ok(())
}

fn nxbd_image_digest(meta: &ComponentMeta) -> Option<[u8; 32]> {
    bootfmt::nxbd::decode(&meta.kind_data).ok().map(|(d, _)| d.image_sha256)
}

/// system-volume binding (§12.4): `kind_data` is a well-FORMED NXSV whose
/// `index_len`/`index_sha256` describe THIS component (the superblock +
/// index payload), whose build/rollback match the manifest, and whose
/// `boot_image_sha256` pairs with the boot image the SAME set carries.
/// A volume-only set (no boot image in the set) leaves pairing to the
/// sink, which checks it against the ACTIVE NXBD. The NXSV signature is
/// bundlemgrd's to verify at boot (OS-key anchor).
fn check_system_volume_binding(
    manifest: &ManifestHeader,
    meta: &ComponentMeta,
    boot_digest: Option<[u8; 32]>,
) -> Result<(), RejectReason> {
    let (nxsv, _sig) =
        bootfmt::nxsv::decode(&meta.kind_data).map_err(|_| RejectReason::VolumeBinding)?;
    if u64::from(nxsv.index_len) != meta.size || nxsv.index_sha256 != meta.sha256 {
        return Err(RejectReason::VolumeBinding);
    }
    if nxsv.volume_size < u64::from(nxsv.index_len)
        || nxsv.build_id_str() != manifest.build_id
        || nxsv.rollback_index != manifest.rollback_index
    {
        return Err(RejectReason::VolumeBinding);
    }
    if let Some(boot) = boot_digest {
        if nxsv.boot_image_sha256 != boot {
            return Err(RejectReason::VolumeBinding);
        }
    }
    Ok(())
}

/// bundle binding (§12.4): the payload IS a bundle window (its digest is
/// the index bundle sha256 — membership is the sink's check against the
/// verified index); no kind data, a bounded non-empty name.
fn check_bundle_binding(meta: &ComponentMeta) -> Result<(), RejectReason> {
    if !meta.kind_data.is_empty() || meta.name.is_empty() || meta.name.len() > 96 {
        return Err(RejectReason::Bounds);
    }
    Ok(())
}

/// Borrowed tar walk (zero-copy windows; ustar subset, same rules as the
/// v1 walker: bounded sizes, safe relative paths, dirs skipped).
fn walk_tar(bytes: &[u8]) -> Result<Vec<TarView<'_>>, RejectReason> {
    let mut entries = Vec::new();
    let mut offset = 0usize;
    while offset + 512 <= bytes.len() {
        let header = &bytes[offset..offset + 512];
        if header.iter().all(|b| *b == 0) {
            break;
        }
        let name_bytes = &header[0..100];
        let name_len = name_bytes.iter().position(|b| *b == 0).unwrap_or(100);
        let name =
            core::str::from_utf8(&name_bytes[..name_len]).map_err(|_| RejectReason::Path)?.trim();
        if name.is_empty() || !is_safe_path(name) {
            return Err(RejectReason::Path);
        }
        let size = parse_octal(&header[124..136])?;
        let typeflag = header[156];
        let data_start = offset + 512;
        let data_end = data_start.checked_add(size).ok_or(RejectReason::Bounds)?;
        if data_end > bytes.len() {
            return Err(RejectReason::Bounds);
        }
        if typeflag != b'5' {
            entries.push(TarView { name, data: &bytes[data_start..data_end] });
        }
        let padded = size.div_ceil(512) * 512;
        offset = data_start + padded;
    }
    Ok(entries)
}

fn is_safe_path(name: &str) -> bool {
    !name.starts_with('/')
        && !name.contains('\0')
        && !name.split('/').any(|seg| seg == ".." || seg.is_empty())
}

fn parse_octal(bytes: &[u8]) -> Result<usize, RejectReason> {
    let mut out: usize = 0;
    let mut saw_digit = false;
    for b in bytes.iter() {
        if *b == 0 || *b == b' ' {
            continue;
        }
        if *b < b'0' || *b > b'7' {
            return Err(RejectReason::Bounds);
        }
        saw_digit = true;
        out = out
            .checked_mul(8)
            .and_then(|v| v.checked_add((b - b'0') as usize))
            .ok_or(RejectReason::Bounds)?;
    }
    if saw_digit {
        Ok(out)
    } else {
        Err(RejectReason::Bounds)
    }
}
