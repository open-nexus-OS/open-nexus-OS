// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: `nx image` system-volume half (TASK-0321 P1, RFC-0089 §12):
//! turns a directory of `.nxb` bundle directories into the deterministic
//! pkgimg **v3** volume (`storage::pkgimg_bundles`), signs its **NXSV**
//! root descriptor (`bootfmt::nxsv`) with the OS-image key, writes it to
//! `system-a` NXSV-LAST, verifies a built volume end to end (signature,
//! pairing with boot-a, index + every bundle/entry digest) and emits the
//! `[system-volume, bundle…]` components of a bundle-set `.nxs`. Launch
//! parameters (`stack_pages` from `meta/launch.json`, `global_pointer`
//! from the payload ELF with the SAME `object` logic init-lite's build
//! script uses) land in the bundle table so init never parses ELF.
//! OWNERS: @reliability @tools-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: tests/image_cli.rs (system-volume determinism, tamper /
//!   unpaired rejects, bundle-set container decode)
//! ADR: docs/adr/0060-verified-system-volume-bundlemgrd-verifier-init-spawner.md

use std::path::{Path, PathBuf};

use serde_json::json;
use sha2::{Digest, Sha256};

use crate::commands::image::{hex, sha256, FileBlockDevice, SECTOR};
use crate::error::{ExitClass, NxError};
use storage::gpt::Partition;
use storage::pkgimg::{PkgImgCaps, PkgImgFileSpec};
use storage::pkgimg_bundles::{
    build_volume, parse_index, verify_bundle, verify_entry, BundleLaunch, VolumeIndex,
};
use storage::BlockDevice;

/// Volume payload starts at system-partition sector 8 (RFC-0089 §12.2;
/// sectors 1–7 reserved, sector 1 = the TASK-0035 stage journal).
pub(crate) const VOLUME_START_SECTOR: u64 = 8;

/// One `.nxb` bundle directory, loaded for the volume builder.
pub(crate) struct BundleDir {
    pub(crate) name: String,
    pub(crate) version: String,
    pub(crate) files: Vec<PkgImgFileSpec>,
    pub(crate) launch: Option<BundleLaunch>,
}

fn reject(msg: String) -> NxError {
    NxError::new(ExitClass::ValidationReject, msg)
}

fn read(path: &Path) -> Result<Vec<u8>, NxError> {
    std::fs::read(path).map_err(|err| {
        NxError::new(ExitClass::MissingDependency, format!("image: read {}: {err}", path.display()))
    })
}

/// Loads every `<root>/<dir>/` that carries a `manifest.nxb` (ADR-0020
/// bundle directory contract), in sorted order (deterministic input).
pub(crate) fn load_bundle_dirs(root: &Path) -> Result<Vec<BundleDir>, NxError> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(root)
        .map_err(|err| {
            NxError::new(
                ExitClass::MissingDependency,
                format!("image: bundle root {}: {err}", root.display()),
            )
        })?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    let mut out = Vec::new();
    for dir in dirs {
        let manifest_path = dir.join("manifest.nxb");
        if !manifest_path.is_file() {
            return Err(reject(format!(
                "image: {} is not a bundle directory (manifest.nxb missing)",
                dir.display()
            )));
        }
        let manifest = bundlemgr::Manifest::parse_nxb(&read(&manifest_path)?).map_err(|err| {
            reject(format!("image: {}: manifest.nxb invalid: {err}", dir.display()))
        })?;
        let name = manifest.name.clone();
        let version = manifest.version.to_string();
        let mut files = Vec::new();
        collect_files(&dir, &dir, &mut files)?;
        let mut specs = Vec::with_capacity(files.len());
        for (rel, bytes) in files {
            specs.push(PkgImgFileSpec::new(&name, &version, &rel, &bytes));
        }
        let launch = launch_params(&dir, &name, &version)?;
        if out.iter().any(|b: &BundleDir| b.name == name && b.version == version) {
            return Err(reject(format!(
                "image: duplicate bundle {name}@{version} under {}",
                root.display()
            )));
        }
        out.push(BundleDir { name, version, files: specs, launch });
    }
    if out.is_empty() {
        return Err(reject(format!("image: no bundle directories under {}", root.display())));
    }
    Ok(out)
}

fn collect_files(base: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) -> Result<(), NxError> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|err| {
            NxError::new(ExitClass::MissingDependency, format!("image: {}: {err}", dir.display()))
        })?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            collect_files(base, &path, out)?;
        } else if path.is_file() {
            let rel = path
                .strip_prefix(base)
                .map_err(|_| reject("image: bundle file outside its directory".into()))?
                .to_string_lossy()
                .replace('\\', "/");
            out.push((rel, read(&path)?));
        }
    }
    Ok(())
}

/// `meta/launch.json` `{ "stack_pages": N }` marks a spawnable service
/// bundle; `global_pointer` comes from the payload ELF (`__global_pointer$`
/// or `.sdata + 0x800`, the RISC-V psABI fallback init-lite uses).
fn launch_params(dir: &Path, name: &str, version: &str) -> Result<Option<BundleLaunch>, NxError> {
    let launch_path = dir.join("meta").join("launch.json");
    if !launch_path.is_file() {
        return Ok(None);
    }
    let v: serde_json::Value = serde_json::from_slice(&read(&launch_path)?)
        .map_err(|err| reject(format!("image: {}: {err}", launch_path.display())))?;
    let stack_pages =
        v["stack_pages"].as_u64().filter(|n| (1..=64).contains(n)).ok_or_else(|| {
            reject(format!("image: {}: stack_pages 1..=64", launch_path.display()))
        })? as u32;
    let elf = read(&dir.join("payload.elf"))?;
    let global_pointer = global_pointer_of(&elf).ok_or_else(|| {
        reject(format!(
            "image: bundle {name}@{version}: payload.elf has no __global_pointer$/.sdata"
        ))
    })?;
    Ok(Some(BundleLaunch {
        bundle: name.to_string(),
        version: version.to_string(),
        stack_pages,
        global_pointer,
    }))
}

pub(crate) fn global_pointer_of(elf: &[u8]) -> Option<u64> {
    use object::{Object, ObjectSection, ObjectSymbol};
    let file = object::File::parse(elf).ok()?;
    if let Some(addr) = file
        .symbols()
        .find_map(|s| s.name().ok().filter(|n| *n == "__global_pointer$").map(|_| s.address()))
    {
        return Some(addr);
    }
    file.section_by_name(".sdata").map(|s| s.address().saturating_add(0x800))
}

/// Builds the v3 volume bytes from bundle directories.
pub(crate) fn build_volume_bytes(bundles: &[BundleDir]) -> Result<Vec<u8>, NxError> {
    let specs: Vec<PkgImgFileSpec> = bundles.iter().flat_map(|b| b.files.iter().cloned()).collect();
    let launch: Vec<BundleLaunch> = bundles.iter().filter_map(|b| b.launch.clone()).collect();
    build_volume(&specs, &launch, &PkgImgCaps::default())
        .map_err(|err| reject(format!("image: system volume build: {err}")))
}

/// Signs the NXSV for `volume` paired with the boot image digest.
pub(crate) fn make_nxsv(
    volume: &[u8],
    index: &VolumeIndex,
    boot_image_sha256: [u8; 32],
    build_id: &str,
    rollback_index: u32,
    os_seed: &[u8; 32],
) -> Result<[u8; SECTOR], NxError> {
    let index_len = index.superblock.index_end();
    let (_pk, pubkey_id) = bootfmt::nxbd::pubkey_id_for_seed(os_seed);
    let desc = bootfmt::nxsv::Nxsv {
        rollback_index,
        volume_size: volume.len() as u64,
        volume_sha256: sha256(volume),
        build_id: bootfmt::nxbd::Nxbd::build_id_from(build_id),
        pubkey_id,
        boot_image_sha256,
        index_len: index_len as u32,
        index_sha256: sha256(&volume[..index_len]),
    };
    Ok(bootfmt::nxsv::sign(&desc, os_seed))
}

pub(crate) fn volume_budget_sectors(p: &Partition) -> u64 {
    (p.last_lba - p.first_lba + 1).saturating_sub(VOLUME_START_SECTOR)
}

/// Writes a system slot NXSV-LAST: zero the descriptor, write the padded
/// volume body from sector 8, THEN the signed descriptor.
pub(crate) fn write_volume_slot(
    dev: &mut FileBlockDevice,
    slot: &Partition,
    volume: &[u8],
    nxsv: &[u8; SECTOR],
) -> Result<(), NxError> {
    let budget = volume_budget_sectors(slot) * SECTOR as u64;
    if volume.len() as u64 > budget {
        return Err(reject(format!(
            "image: system volume ({} B) exceeds the `{}` budget ({budget} B)",
            volume.len(),
            slot.name
        )));
    }
    let io = |e: storage::BlockError| {
        NxError::new(ExitClass::Internal, format!("image: volume write failed ({e:?})"))
    };
    dev.write_blocks(slot.first_lba, &[0u8; SECTOR]).map_err(io)?;
    let mut padded = volume.to_vec();
    padded.resize(volume.len().div_ceil(SECTOR) * SECTOR, 0);
    dev.write_blocks(slot.first_lba + VOLUME_START_SECTOR, &padded).map_err(io)?;
    dev.write_blocks(slot.first_lba, nxsv).map_err(io)?;
    Ok(())
}

/// Reads the whole volume body named by a verified NXSV.
fn read_volume(dev: &FileBlockDevice, slot: &Partition, size: u64) -> Result<Vec<u8>, NxError> {
    if size > volume_budget_sectors(slot) * SECTOR as u64 {
        return Err(reject("image: nxsv volume_size exceeds the slot".into()));
    }
    let padded = (size as usize).div_ceil(SECTOR) * SECTOR;
    let mut buf = vec![0u8; padded];
    dev.read_blocks(slot.first_lba + VOLUME_START_SECTOR, &mut buf)
        .map_err(|e| NxError::new(ExitClass::Internal, format!("image: volume read ({e:?})")))?;
    buf.truncate(size as usize);
    Ok(buf)
}

/// Verifies a system slot: `Ok(None)` for a factory-empty slot (zeroed
/// descriptor), else every check bundlemgrd performs at boot PLUS the lazy
/// per-bundle/per-entry digests (the host has all the bytes).
pub(crate) fn verify_volume_slot(
    dev: &FileBlockDevice,
    slot: &Partition,
    pubkey: &[u8; 32],
    boot_image_sha256: [u8; 32],
) -> Result<Option<serde_json::Value>, NxError> {
    let mut sector = [0u8; SECTOR];
    dev.read_blocks(slot.first_lba, &mut sector)
        .map_err(|e| NxError::new(ExitClass::Internal, format!("image: nxsv read ({e:?})")))?;
    if sector.iter().all(|&b| b == 0) {
        return Ok(None);
    }
    let desc = bootfmt::nxsv::verify(&sector, pubkey)
        .map_err(|e| reject(format!("image: `{}` nxsv verify failed ({e:?})", slot.name)))?;
    if desc.boot_image_sha256 != boot_image_sha256 {
        return Err(reject(format!("image: `{}` is not paired with boot-a (pair)", slot.name)));
    }
    let volume = read_volume(dev, slot, desc.volume_size)?;
    if sha256(&volume) != desc.volume_sha256 {
        return Err(reject(format!("image: `{}` volume digest mismatch", slot.name)));
    }
    let index_len = desc.index_len as usize;
    if index_len > volume.len() || sha256(&volume[..index_len]) != desc.index_sha256 {
        return Err(reject(format!("image: `{}` index digest mismatch", slot.name)));
    }
    let index = parse_index(&volume[..index_len], &PkgImgCaps::default())
        .map_err(|err| reject(format!("image: `{}` index: {err}", slot.name)))?;
    if index.superblock.index_end() != index_len {
        return Err(reject(format!(
            "image: `{}` nxsv index_len disagrees with the superblock",
            slot.name
        )));
    }
    for b in &index.bundles {
        verify_bundle(&volume, &index.superblock, b).map_err(|err| {
            reject(format!("image: `{}` bundle {}@{}: {err}", slot.name, b.bundle, b.version))
        })?;
    }
    for e in &index.entries {
        verify_entry(&volume, &index.superblock, e).map_err(|err| {
            reject(format!("image: `{}` {}@{}/{}: {err}", slot.name, e.bundle, e.version, e.path))
        })?;
    }
    Ok(Some(json!({
        "build_id": desc.build_id_str(),
        "rollback_index": desc.rollback_index,
        "volume_size": desc.volume_size,
        "volume_sha256": hex(&desc.volume_sha256),
        "index_len": desc.index_len,
        "paired": true,
        "bundles": index.bundles.iter().map(|b| json!({
            "bundle": b.bundle, "version": b.version, "bytes": b.data_len,
            "sha256": hex(&b.sha256), "stack_pages": b.stack_pages,
            "global_pointer": format!("0x{:x}", b.global_pointer),
        })).collect::<Vec<_>>(),
    })))
}

/// One `.nxs` component (kind, name, payload path, payload, kindData).
pub(crate) struct Component {
    pub(crate) kind: u8,
    pub(crate) name: String,
    pub(crate) payload_path: String,
    pub(crate) payload: Vec<u8>,
    pub(crate) kind_data: Vec<u8>,
}

/// The bundle-set components (RFC-0089 §12.4 ordering: `system-volume`
/// first — its payload is the superblock + index, `kindData` the NXSV —
/// then one `bundle` per bundle-table row whose payload is exactly the
/// bundle's window, so the component sha256 IS the index bundle sha256).
pub(crate) fn bundle_set_components(
    volume: &[u8],
    index: &VolumeIndex,
    nxsv: &[u8; SECTOR],
) -> Vec<Component> {
    bundle_set_components_reusing(volume, index, nxsv, None).0
}

/// The same set, shipping ONLY the bundles the device cannot reuse: a
/// bundle whose window digest is already on `base` (the volume the device
/// runs) is left out — the device's `commit_set` copies it from the active
/// slot against the new index (RFC-0089 §12.5 `updated: bundle reused`).
/// Returns the components and the names of the reused bundles.
pub(crate) fn bundle_set_components_reusing(
    volume: &[u8],
    index: &VolumeIndex,
    nxsv: &[u8; SECTOR],
    base: Option<&VolumeIndex>,
) -> (Vec<Component>, Vec<String>) {
    let index_len = index.superblock.index_end();
    let mut reused = Vec::new();
    let mut out = vec![Component {
        kind: updates::component_set::KIND_SYSTEM_VOLUME,
        name: "system-volume".to_string(),
        payload_path: "system.idx".to_string(),
        payload: volume[..index_len].to_vec(),
        kind_data: nxsv.to_vec(),
    }];
    for b in &index.bundles {
        if base.is_some_and(|bi| bi.bundles.iter().any(|x| x.sha256 == b.sha256)) {
            reused.push(format!("{}@{}", b.bundle, b.version));
            continue;
        }
        let begin = index.superblock.data_offset + b.data_offset as usize;
        let window = &volume[begin..begin + b.data_len as usize];
        debug_assert_eq!(Sha256::digest(window).as_slice(), b.sha256);
        out.push(Component {
            kind: updates::component_set::KIND_BUNDLE,
            name: format!("{}@{}", b.bundle, b.version),
            payload_path: format!("bundles/{}@{}.bin", b.bundle, b.version),
            payload: window.to_vec(),
            kind_data: Vec::new(),
        });
    }
    (out, reused)
}
