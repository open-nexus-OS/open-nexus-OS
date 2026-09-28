// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: the flash plan of a board image (TASK-0260 P2, RFC-0089 §2). The vendor flash
//! vehicle builds a GPT of its own from a JSON, typing every partition basic data, and its JSON
//! regions compute byte offsets in 32 bits. So the plan hands it none of that: every byte goes
//! as a raw region the vehicle is told about through `fastboot_raw_partition_<name>` — 64-bit
//! block addresses, the eMMC's hardware partition included. The regions: the user area from
//! sector 0 to the end of its last partition, in chunks no larger than one download; the
//! backup GPT in the disk's last sectors; boot0 (hardware partition 1), where the boot ROM reads
//! the header and the SPL. Each region carries its file and SHA-256; `scripts/board-flash.sh`
//! declares and writes them, and `--verify` reads each back from the stock system and compares.
//! Deterministic: the same image gives the same plan, byte for byte.
//! OWNERS: @reliability @tools-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: tests/image_flash_cli.rs

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use serde_json::json;
use sha2::{Digest, Sha256};
use storage::gpt::parse_gpt;
use storage::BlockDevice;

use crate::cli_image::ImageFlashPlanArgs;
use crate::commands::image::{hex, FileBlockDevice, SECTOR};
use crate::commands::image_board::{boot0_path, parse_bytes};
use crate::error::{ExecResult, ExitClass, NxError};

/// Hardware partitions of the eMMC: the user area and boot0.
const USER_AREA: u8 = 0;
const BOOT0: u8 = 1;
/// Copy granularity; all-zero pieces stay holes in the region files.
const PIECE: usize = 1 << 20;

fn reject(msg: String) -> NxError {
    NxError::new(ExitClass::ValidationReject, msg)
}

fn internal(msg: String) -> NxError {
    NxError::new(ExitClass::Internal, msg)
}

/// One region the vehicle writes raw.
struct Region {
    name: String,
    hwpart: u8,
    start: u64,
    sectors: u64,
}

pub(crate) fn handle_flash_plan(args: ImageFlashPlanArgs) -> ExecResult {
    let chunk = parse_bytes(&args.chunk_bytes).filter(|b| *b >= PIECE as u64).ok_or_else(|| {
        reject(format!("image: --chunk-bytes {}: at least 1 MiB", args.chunk_bytes))
    })?;
    let chunk_sectors = chunk / SECTOR as u64;
    let dev = FileBlockDevice::open_ro(&args.image).map_err(|err| {
        NxError::new(
            ExitClass::MissingDependency,
            format!("image: open {}: {err}", args.image.display()),
        )
    })?;
    let parts = parse_gpt(&dev).map_err(|e| reject(format!("image: no GPT ({e:?})")))?;
    let disk = dev.block_count();
    // The image must be the disk it names: its backup GPT at its own last sector.
    let primary = sector(&dev, 1)?;
    let alternate = u64_at(&primary, 32);
    if alternate + 1 != disk {
        return Err(reject("image: not a disk image (its GPT's backup is not at its end)".into()));
    }
    let backup = sector(&dev, alternate)?;
    let backup_entries = u64_at(&backup, 72);
    if &backup[..8] != b"EFI PART"
        || u64_at(&backup, 24) != alternate
        || backup_entries >= alternate
    {
        return Err(reject("image: the backup GPT is not at the disk's end".into()));
    }
    let span = parts.iter().map(|p| p.last_lba + 1).max().unwrap_or(0);
    let boot0 = std::fs::read(boot0_path(&args.image)).map_err(|err| {
        NxError::new(ExitClass::MissingDependency, format!("image: the board image's boot0: {err}"))
    })?;
    if boot0.is_empty() || boot0.len() % SECTOR != 0 {
        return Err(reject("image: boot0 is not whole sectors".into()));
    }

    let mut regions: Vec<Region> = (0..span.div_ceil(chunk_sectors))
        .map(|i| Region {
            name: format!("nxdisk{i}"),
            hwpart: USER_AREA,
            start: i * chunk_sectors,
            sectors: chunk_sectors.min(span - i * chunk_sectors),
        })
        .collect();
    regions.push(Region {
        name: "nxgpt".into(),
        hwpart: USER_AREA,
        start: backup_entries,
        sectors: alternate + 1 - backup_entries,
    });
    regions.push(Region {
        name: "nxboot0".into(),
        hwpart: BOOT0,
        start: 0,
        sectors: (boot0.len() / SECTOR) as u64,
    });

    std::fs::create_dir_all(&args.out_dir)
        .map_err(|err| internal(format!("image: create {}: {err}", args.out_dir.display())))?;
    let mut listed = Vec::new();
    for r in &regions {
        let file = args.out_dir.join(format!("{}.img", r.name));
        let digest = if r.hwpart == BOOT0 {
            write_region_from(&mut boot0.as_slice(), r.sectors, &file)?
        } else {
            let mut src =
                File::open(&args.image).map_err(|err| internal(format!("image: {err}")))?;
            src.seek(SeekFrom::Start(r.start * SECTOR as u64))
                .map_err(|err| internal(format!("image: {err}")))?;
            write_region_from(&mut src, r.sectors, &file)?
        };
        listed.push(json!({ "name": r.name, "hwpart": r.hwpart, "start_lba": r.start,
            "sectors": r.sectors, "file": format!("{}.img", r.name), "sha256": hex(&digest) }));
    }
    // RFC-0107: the trace partition is written by every boot — `board-flash.sh --verify` reads
    // it as zero, as the plan's regions carry it.
    let trace = storage::trace::partition(&parts)
        .map(|p| json!({ "start_lba": p.first_lba, "sectors": p.last_lba + 1 - p.first_lba }));
    let plan =
        json!({ "disk_sectors": disk, "chunk_bytes": chunk, "regions": listed, "trace": trace });
    let text = serde_json::to_string_pretty(&plan).map_err(|e| internal(format!("image: {e}")))?;
    let plan_path = args.out_dir.join("plan.json");
    std::fs::write(&plan_path, format!("{text}\n"))
        .map_err(|err| internal(format!("image: write {}: {err}", plan_path.display())))?;
    Ok((
        ExitClass::Success,
        format!("image: flash plan {} ({} regions)", plan_path.display(), regions.len()),
        args.json,
        Some(plan),
    ))
}

fn sector(dev: &FileBlockDevice, lba: u64) -> Result<Vec<u8>, NxError> {
    let mut buf = vec![0u8; SECTOR];
    dev.read_block(lba, &mut buf)
        .map_err(|e| internal(format!("image: read sector {lba} ({e:?})")))?;
    Ok(buf)
}

fn u64_at(buf: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(buf[at..at + 8].try_into().unwrap_or([0; 8]))
}

/// Copies `sectors` sectors from `src` into `file` — all-zero pieces left as holes — and
/// returns the SHA-256 of the region's bytes.
fn write_region_from(src: &mut dyn Read, sectors: u64, file: &Path) -> Result<[u8; 32], NxError> {
    let io = |err: std::io::Error| internal(format!("image: region {}: {err}", file.display()));
    let mut out = File::create(file).map_err(io)?;
    let mut hasher = Sha256::new();
    let mut left = sectors * SECTOR as u64;
    let mut buf = vec![0u8; PIECE];
    while left > 0 {
        let n = PIECE.min(left as usize);
        src.read_exact(&mut buf[..n]).map_err(io)?;
        hasher.update(&buf[..n]);
        if buf[..n].iter().all(|b| *b == 0) {
            out.seek(SeekFrom::Current(n as i64)).map_err(io)?;
        } else {
            out.write_all(&buf[..n]).map_err(io)?;
        }
        left -= n as u64;
    }
    out.set_len(sectors * SECTOR as u64).map_err(io)?;
    Ok(hasher.finalize().into())
}
