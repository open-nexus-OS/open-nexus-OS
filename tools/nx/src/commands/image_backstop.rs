// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `nx image backstop` (TASK-0289-B) — arms a built disk with a
//! boot-b trial the LOADER must reject, proving the boot-time trust floor
//! independently of the userspace apply engine. Kind `tamper` plants a
//! fully VALID signed NXBD whose digest covers the untampered payload,
//! then flips ONE payload byte after the descriptor landed — stage-time
//! verification never saw these bytes, only the loader's streamed digest
//! can catch them. Kind `downgrade` plants a fully valid slot at rollback
//! index 0, below the factory floor the BSB ships — only the loader's
//! floor check stands between it and execution. Both kinds arm the BSB
//! with `next=b tries=2` via the alternate-block writer rule (ADR-0058:
//! the factory wrote block 0, this writes block 1 with seq+1 — a torn
//! write leaves the factory block authoritative).
//! OWNERS: @tools-team @security
//! STATUS: Experimental (TASK-0289-B)
//! TEST_COVERAGE: tests/image_cli.rs (alternate-block arm, floor kept);
//!   the loader-side rejects are host-proven in nxboot tests/loader_flow.rs
//!   and the QEMU `ota-tamper` / `ota-downgrade` lanes are the live proof.
//! ADR: docs/adr/0058-boot-selection-block-dual-actor-discipline.md

use serde_json::json;

use crate::cli::ImageBackstopArgs;
use crate::commands::image::{make_nxbd, part, read_kernel, read_seed, write_boot_slot};
use crate::error::{ExecResult, ExitClass, NxError};

use storage::gpt::{parse_gpt, Partition, GUID_NEXUS_BOOT, GUID_NEXUS_BSB};
use storage::BlockDevice;

use crate::commands::image::{FileBlockDevice, SECTOR};

/// Payload sector (relative to the image start) that `tamper` flips a byte
/// in — well inside any real kernel image, far from the NXBD sector.
const TAMPER_SECTOR_OFFSET: u64 = 8;
const TAMPER_BYTE_OFFSET: usize = 100;
/// Image payload starts at this slot-relative sector (parity with
/// `image.rs::IMAGE_START_SECTOR` — the NXBD owns sector 0).
const IMAGE_START_SECTOR: u64 = 8;

pub(crate) fn handle_backstop(args: ImageBackstopArgs) -> ExecResult {
    let (rollback_index, corrupt) = match args.kind.as_str() {
        "tamper" => (2u32, true),
        "downgrade" => (0u32, false),
        _ => {
            return Err(NxError::new(ExitClass::Usage, "image: --kind must be tamper or downgrade"))
        }
    };
    let os_seed = read_seed(&args.sign)?;
    let kernel = read_kernel(&args.kernel)?;
    let mut dev = FileBlockDevice::open_rw(&args.image).map_err(|err| {
        NxError::new(
            ExitClass::MissingDependency,
            format!("image: open {}: {err}", args.image.display()),
        )
    })?;
    let parts = parse_gpt(&dev).map_err(|e| {
        NxError::new(ExitClass::ValidationReject, format!("image: gpt parse failed ({e:?})"))
    })?;

    // Plant the trial image with a fully valid, signed descriptor (the
    // NXBD is honest — for `tamper` the DISK lies afterwards).
    let boot_b = part(&parts, &GUID_NEXUS_BOOT, "boot-b")?;
    let nxbd = make_nxbd(&kernel, &args.build_id, rollback_index, &os_seed)?;
    write_boot_slot(&mut dev, &boot_b, &kernel, &nxbd)?;
    if corrupt {
        flip_payload_byte(&mut dev, &boot_b)?;
    }

    // Arm the BSB: read the current pair, write the ALTERNATE block with
    // seq+1 and next=b (writer rule — a torn write keeps the old block).
    let bsb_part = part(&parts, &GUID_NEXUS_BSB, "bsb")?;
    let io = |e| NxError::new(ExitClass::Internal, format!("image: bsb io ({e:?})"));
    let mut blocks = [0u8; 2 * SECTOR];
    dev.read_blocks(bsb_part.first_lba, &mut blocks[..SECTOR]).map_err(io)?;
    dev.read_blocks(bsb_part.first_lba + 1, &mut blocks[SECTOR..]).map_err(io)?;
    let (cur, cur_idx) =
        bootfmt::bsb::pick(&blocks[..SECTOR], &blocks[SECTOR..]).ok_or_else(|| {
            NxError::new(ExitClass::ValidationReject, "image: no valid bsb block to arm from")
        })?;
    let armed = bootfmt::bsb::Bsb {
        seq: cur.seq + 1,
        next_slot: Some(bootfmt::bsb::Slot::B),
        tries_left: 2,
        health_committed: false,
        ..cur
    };
    let alternate_lba = bsb_part.first_lba + (1 - cur_idx as u64);
    dev.write_blocks(alternate_lba, &bootfmt::bsb::encode(&armed)).map_err(io)?;
    dev.sync().map_err(|e| NxError::new(ExitClass::Internal, format!("image: sync ({e:?})")))?;

    Ok((
        ExitClass::Success,
        format!(
            "image: backstop armed (kind={} build={} floor={})",
            args.kind, args.build_id, cur.rollback_min_index
        ),
        args.json,
        Some(json!({
            "image": args.image.display().to_string(),
            "kind": args.kind,
            "build_id": args.build_id,
            "nxbd_rollback_index": rollback_index,
            "bsb_floor": cur.rollback_min_index,
            "bsb_seq": armed.seq,
        })),
    ))
}

/// XORs one byte in the planted payload — after this, the (valid) NXBD's
/// digest no longer matches the slot bytes.
fn flip_payload_byte(dev: &mut FileBlockDevice, slot: &Partition) -> Result<(), NxError> {
    let lba = slot.first_lba + IMAGE_START_SECTOR + TAMPER_SECTOR_OFFSET;
    let io = |e| NxError::new(ExitClass::Internal, format!("image: tamper io ({e:?})"));
    let mut sector = [0u8; SECTOR];
    dev.read_blocks(lba, &mut sector).map_err(io)?;
    sector[TAMPER_BYTE_OFFSET] ^= 0xff;
    dev.write_blocks(lba, &sector).map_err(io)?;
    Ok(())
}
