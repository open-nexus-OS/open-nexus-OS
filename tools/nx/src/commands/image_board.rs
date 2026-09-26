// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: the disk an image is built for (TASK-0260 P1/P2, RFC-0089 §2). QEMU's: the image
//! file is the disk — the layout's 384 MiB, or the size a lane asks for. A board's, from its
//! profile (`config/board/<board>/image.toml`), as the vendor's flash source shows the eMMC
//! boots (docs/board/measurements/2026-09-26-boot-medium): the user area — exactly the disk's
//! sector count, so the GPT's backup sits at its last sector, with the head the SPL loads by
//! name (`opensbi`, and `uboot` holding the loader's FIT once given) — and, beside it, the
//! eMMC's boot0 hardware partition (`<out>.boot0`): the boot-ROM header at 0 and the SPL at the
//! offset the header names. The header is checked as the boot ROM checks it (length, magic,
//! CRC-32 of the first 64 bytes) and must be the eMMC's (media tag, the SPL's offset and size
//! limit). The profile's paths are relative to the profile, so it is read the same from
//! anywhere.
//! OWNERS: @reliability @tools-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: tests/image_board_cli.rs (a board image against a fixture profile, its refusals)

use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::json;
use storage::gpt::{crc32_ieee, Partition};
use storage::layout::NEXUS_DISK_BYTES;
use storage::BlockDevice;

use crate::cli::ImageBuildArgs;
use crate::commands::image::{hex, part_named, sha256, FileBlockDevice, SECTOR};
use crate::error::{ExitClass, NxError};

/// The boot-ROM header: 80 bytes, the magic first, a CRC-32 (IEEE) of its first 64 bytes at 64.
const BOOT_HEADER_LEN: usize = 80;
const BOOT_HEADER_MAGIC: u32 = 0xB007_14F0;
const BOOT_HEADER_CRC_SPAN: usize = 64;
/// Fields of the header the eMMC image depends on: the media tag, the SPL's offset (in boot0)
/// and the largest SPL the boot ROM loads.
const BOOT_HEADER_MEDIA: std::ops::Range<usize> = 8..12;
const BOOT_HEADER_SPL0_OFFSET: usize = 0x20;
const BOOT_HEADER_SPL_LIMIT: usize = 0x28;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    disk: DiskProfile,
    boot0: Boot0Profile,
    head: HeadProfile,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DiskProfile {
    /// The board's boot disk (the eMMC's user area) in 512-byte sectors.
    sectors: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Boot0Profile {
    /// The eMMC's boot0 hardware partition in 512-byte sectors.
    sectors: u64,
    header: PathBuf,
    spl: PathBuf,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HeadProfile {
    opensbi: PathBuf,
}

/// The disk an image is built for.
pub(crate) struct Disk {
    pub(crate) bytes: u64,
    head: Option<Head>,
}

/// A board's boot chain, read and checked.
struct Head {
    /// The whole boot0 image: the header at 0, the SPL at its offset, zeros elsewhere.
    boot0: Vec<u8>,
    spl0_offset: usize,
    /// (partition, bytes) in the user area — `opensbi` and, when given, `uboot`.
    parts: Vec<(&'static str, Vec<u8>)>,
}

fn reject(msg: String) -> NxError {
    NxError::new(ExitClass::ValidationReject, msg)
}

fn read(path: &Path) -> Result<Vec<u8>, NxError> {
    std::fs::read(path).map_err(|err| {
        NxError::new(ExitClass::MissingDependency, format!("image: read {}: {err}", path.display()))
    })
}

fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

/// `4294967296`, `4096M`, `4G` (binary units) — a whole number of sectors.
pub(crate) fn parse_bytes(text: &str) -> Option<u64> {
    let (digits, unit) = match text.char_indices().last()? {
        (i, 'K' | 'k') => (&text[..i], 1u64 << 10),
        (i, 'M' | 'm') => (&text[..i], 1u64 << 20),
        (i, 'G' | 'g') => (&text[..i], 1u64 << 30),
        _ => (text, 1),
    };
    let bytes = digits.parse::<u64>().ok()?.checked_mul(unit)?;
    (bytes % SECTOR as u64 == 0).then_some(bytes)
}

/// The disk `args` asks for, its boot chain read and checked.
pub(crate) fn disk_for(args: &ImageBuildArgs) -> Result<Disk, NxError> {
    if args.target == "qemu" {
        if args.board_profile.is_some() || args.fit.is_some() {
            return Err(reject("image: --board-profile and --fit belong to a board".into()));
        }
        let bytes = match &args.disk_bytes {
            None => NEXUS_DISK_BYTES,
            Some(text) => {
                parse_bytes(text).filter(|b| *b >= NEXUS_DISK_BYTES).ok_or_else(|| {
                    reject(format!("image: --disk-bytes {text}: sectors of at least 384 MiB"))
                })?
            }
        };
        return Ok(Disk { bytes, head: None });
    }
    if args.disk_bytes.is_some() {
        return Err(reject("image: a board's disk is its profile's, not --disk-bytes".into()));
    }
    if args.target.is_empty()
        || !args.target.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err(reject(format!("image: --target {:?} names no board", args.target)));
    }
    let path = args
        .board_profile
        .clone()
        .unwrap_or_else(|| PathBuf::from(format!("config/board/{}/image.toml", args.target)));
    let text = String::from_utf8(read(&path)?)
        .map_err(|_| reject(format!("image: {} is not UTF-8", path.display())))?;
    let profile: Profile = toml::from_str(&text)
        .map_err(|err| reject(format!("image: profile {}: {err}", path.display())))?;
    let bytes = profile.disk.sectors.checked_mul(SECTOR as u64).filter(|b| *b >= NEXUS_DISK_BYTES);
    let bytes =
        bytes.ok_or_else(|| reject("image: the board's disk is smaller than 384 MiB".into()))?;
    let base = path.parent().unwrap_or(Path::new("."));
    let header = read(&base.join(&profile.boot0.header))?;
    let spl = read(&base.join(&profile.boot0.spl))?;
    let (boot0, spl0_offset) = boot0_image(&header, &spl, profile.boot0.sectors)?;
    let mut parts = vec![("opensbi", read(&base.join(&profile.head.opensbi))?)];
    if let Some(fit) = &args.fit {
        parts.push(("uboot", read(fit)?));
    }
    Ok(Disk { bytes, head: Some(Head { boot0, spl0_offset, parts }) })
}

/// boot0 as the boot ROM reads it: the header — checked as the boot ROM checks it, and the
/// eMMC's — at 0, the SPL at the offset the header names and within the size it allows.
fn boot0_image(header: &[u8], spl: &[u8], sectors: u64) -> Result<(Vec<u8>, usize), NxError> {
    if header.len() != BOOT_HEADER_LEN
        || word(header, 0) != BOOT_HEADER_MAGIC
        || word(header, BOOT_HEADER_CRC_SPAN) != crc32_ieee(&header[..BOOT_HEADER_CRC_SPAN])
    {
        return Err(reject(
            "image: the boot-ROM header is not 80 bytes with its magic and CRC".into(),
        ));
    }
    if &header[BOOT_HEADER_MEDIA] != b"eMMC" {
        return Err(reject("image: the boot-ROM header is not the eMMC's".into()));
    }
    let size = usize::try_from(sectors.saturating_mul(SECTOR as u64)).unwrap_or(usize::MAX);
    let offset = word(header, BOOT_HEADER_SPL0_OFFSET) as usize;
    let limit = word(header, BOOT_HEADER_SPL_LIMIT) as usize;
    if offset < BOOT_HEADER_LEN || offset % SECTOR != 0 || spl.is_empty() || spl.len() > limit {
        return Err(reject(format!(
            "image: the SPL ({} B) does not fit the header's offset {offset:#x} and limit {limit:#x}",
            spl.len()
        )));
    }
    if size > 64 << 20 || offset + spl.len() > size {
        return Err(reject(format!("image: boot0 ({size} B) cannot hold the header and the SPL")));
    }
    let mut boot0 = vec![0u8; size];
    boot0[..BOOT_HEADER_LEN].copy_from_slice(header);
    boot0[offset..offset + spl.len()].copy_from_slice(spl);
    Ok((boot0, offset))
}

impl Disk {
    /// Writes the board's boot chain: the head's pieces into their partitions of the user area,
    /// boot0 next to `out` (`<out>.boot0`). Returns what went where.
    pub(crate) fn write_head(
        &self,
        dev: &mut FileBlockDevice,
        parts: &[Partition],
        out: &Path,
    ) -> Result<serde_json::Value, NxError> {
        let Some(head) = &self.head else { return Ok(serde_json::Value::Null) };
        let mut written = Vec::new();
        for (name, bytes) in &head.parts {
            let p = part_named(parts, name)?;
            let room = (p.last_lba - p.first_lba + 1) * SECTOR as u64;
            if bytes.is_empty() || bytes.len() as u64 > room {
                return Err(reject(format!(
                    "image: {name} ({} B) does not fit {room} B",
                    bytes.len()
                )));
            }
            let mut padded = bytes.clone();
            padded.resize(bytes.len().div_ceil(SECTOR) * SECTOR, 0);
            dev.write_blocks(p.first_lba, &padded).map_err(|e| {
                NxError::new(ExitClass::Internal, format!("image: head write ({e:?})"))
            })?;
            written
                .push(json!({ "at": name, "bytes": bytes.len(), "sha256": hex(&sha256(bytes)) }));
        }
        let side = boot0_path(out);
        std::fs::write(&side, &head.boot0).map_err(|err| {
            NxError::new(ExitClass::Internal, format!("image: write {}: {err}", side.display()))
        })?;
        Ok(json!({
            "written": written,
            "fit": head.parts.iter().any(|(n, _)| *n == "uboot"),
            "boot0": { "file": side.display().to_string(), "bytes": head.boot0.len(),
                "spl0_offset": head.spl0_offset, "sha256": hex(&sha256(&head.boot0)) },
        }))
    }
}

/// Where a board image's boot0 lives: next to the image.
pub(crate) fn boot0_path(out: &Path) -> PathBuf {
    let mut name = out.as_os_str().to_owned();
    name.push(".boot0");
    PathBuf::from(name)
}
