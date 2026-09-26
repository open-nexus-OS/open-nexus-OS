// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: the disk an image is built for (TASK-0260 P1, RFC-0089 §2). QEMU's: the image file
//! is the disk — the layout's 384 MiB, or the size a lane asks for. A board's, from its profile
//! (`config/board/<board>/image.toml`): the disk's sector count, so the GPT's backup sits at
//! that disk's last sector and the image is the disk byte for byte (sparse), and the boot-ROM
//! head's contents — the boot-ROM header in the first 80 bytes of sector 0, next to the
//! protective MBR (checked: length, magic, CRC, as the boot ROM checks it), the SPL and OpenSBI
//! in `fsbl` and `opensbi`, the loader's FIT in `uboot` (TASK-0260B; zero until given), `env`
//! zero (no U-Boot reads it). The profile's paths are relative to the profile, so it is read
//! the same from anywhere.
//! OWNERS: @reliability @tools-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: tests/image_cli.rs (a board image against a fixture profile, its refusals)

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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    disk: DiskProfile,
    head: HeadProfile,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DiskProfile {
    /// The board's boot disk in 512-byte sectors.
    sectors: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HeadProfile {
    boot_header: PathBuf,
    fsbl: PathBuf,
    opensbi: PathBuf,
}

/// The disk an image is built for.
pub(crate) struct Disk {
    pub(crate) bytes: u64,
    head: Option<Head>,
}

/// A board's boot-ROM head, read and checked.
struct Head {
    boot_header: Vec<u8>,
    /// (partition, bytes) — `fsbl`, `opensbi` and, when given, `uboot`.
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

/// `4294967296`, `4096M`, `4G` (binary units) — a whole number of sectors.
fn parse_bytes(text: &str) -> Option<u64> {
    let (digits, unit) = match text.char_indices().last()? {
        (i, 'K' | 'k') => (&text[..i], 1u64 << 10),
        (i, 'M' | 'm') => (&text[..i], 1u64 << 20),
        (i, 'G' | 'g') => (&text[..i], 1u64 << 30),
        _ => (text, 1),
    };
    let bytes = digits.parse::<u64>().ok()?.checked_mul(unit)?;
    (bytes % SECTOR as u64 == 0).then_some(bytes)
}

/// The disk `args` asks for, its head read and checked.
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
    let boot_header = read(&base.join(&profile.head.boot_header))?;
    check_boot_header(&boot_header)?;
    let mut parts = vec![
        ("fsbl", read(&base.join(&profile.head.fsbl))?),
        ("opensbi", read(&base.join(&profile.head.opensbi))?),
    ];
    if let Some(fit) = &args.fit {
        parts.push(("uboot", read(fit)?));
    }
    Ok(Disk { bytes, head: Some(Head { boot_header, parts }) })
}

/// The boot ROM's own checks: the length, the magic, the CRC of the first 64 bytes.
fn check_boot_header(header: &[u8]) -> Result<(), NxError> {
    let word = |at: usize| {
        u32::from_le_bytes([header[at], header[at + 1], header[at + 2], header[at + 3]])
    };
    if header.len() != BOOT_HEADER_LEN
        || word(0) != BOOT_HEADER_MAGIC
        || word(BOOT_HEADER_CRC_SPAN) != crc32_ieee(&header[..BOOT_HEADER_CRC_SPAN])
    {
        return Err(reject(
            "image: the boot-ROM header is not 80 bytes with its magic and CRC".into(),
        ));
    }
    Ok(())
}

impl Disk {
    /// Writes the head into a built disk: the header into sector 0's first 80 bytes (the
    /// protective MBR stays whole), each piece into its partition. Returns what went where.
    pub(crate) fn write_head(
        &self,
        dev: &mut FileBlockDevice,
        parts: &[Partition],
    ) -> Result<serde_json::Value, NxError> {
        let Some(head) = &self.head else { return Ok(serde_json::Value::Null) };
        let io = |what: &str| {
            let what = what.to_string();
            move |e| NxError::new(ExitClass::Internal, format!("image: {what} ({e:?})"))
        };
        let mut sector0 = vec![0u8; SECTOR];
        dev.read_block(0, &mut sector0).map_err(io("sector 0 read"))?;
        sector0[..BOOT_HEADER_LEN].copy_from_slice(&head.boot_header);
        dev.write_block(0, &sector0).map_err(io("sector 0 write"))?;
        let mut written = vec![json!({ "at": "sector 0", "bytes": BOOT_HEADER_LEN,
            "sha256": hex(&sha256(&head.boot_header)) })];
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
            dev.write_blocks(p.first_lba, &padded).map_err(io("head write"))?;
            written
                .push(json!({ "at": name, "bytes": bytes.len(), "sha256": hex(&sha256(bytes)) }));
        }
        Ok(json!({ "written": written, "fit": head.parts.iter().any(|(n, _)| *n == "uboot") }))
    }
}
