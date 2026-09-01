// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: `nx update check/stage/switch/status/rollback` (TASK-0140,
//! RFC-0089 §8/§9) — the host update surface over BUILT artifacts. There
//! is no host↔guest transport, so every verb is honest about its reach:
//! `status` decodes BSB + slot NXBDs from the disk (the same `bootfmt`
//! codecs nxboot/bootctld link), `check` enumerates `/updates/*.nxs` on
//! the data volume and verifies each through the REAL device engine
//! (`updates::component_set` against the baked anchor), `stage` is the
//! §9 provisioning drop (verify, then write into `/updates/`), and
//! `switch`/`rollback` are preflights that report `applied=false` — the
//! live transitions belong to `updated`/bootctld on the device. Verdicts
//! are data; nx exit classes stay the CLI contract.
//! OWNERS: @tools-team @reliability
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: tests/update_cli.rs
//! ADR: docs/rfcs/RFC-0089-ota-v2-component-manifest-ab-boot-images-nxboot-bsb.md

use std::path::Path;

use serde_json::{json, Value};

use crate::cli::{
    UpdateAction, UpdateArgs, UpdateRollbackArgs, UpdateStatusArgs, UpdateSwitchArgs,
};
use crate::commands::image::{part, FileBlockDevice, SECTOR};
use crate::error::{ExecResult, ExitClass, NxError};

use storage::gpt::{parse_gpt, Partition, GUID_NEXUS_BOOT, GUID_NEXUS_BSB};
use storage::BlockDevice;

/// Boot image payload starts at slot-partition sector 8 (RFC-0089 §5).
const IMAGE_START_SECTOR: u64 = 8;

pub(crate) fn handle_update(args: UpdateArgs) -> ExecResult {
    match args.action {
        UpdateAction::Check(a) => crate::commands::update_feed::handle_check(a),
        UpdateAction::Stage(a) => crate::commands::update_feed::handle_stage(a),
        UpdateAction::Switch(a) => handle_switch(a),
        UpdateAction::Status(a) => handle_status(a),
        UpdateAction::Rollback(a) => handle_rollback(a),
    }
}

// --------------------------------------------------------------- devices --

pub(crate) fn open_disk(path: &Path) -> Result<FileBlockDevice, NxError> {
    FileBlockDevice::open_rw(path).map_err(|err| {
        NxError::new(
            ExitClass::MissingDependency,
            format!("update: open {}: {err}", path.display()),
        )
    })
}

pub(crate) fn disk_parts(dev: &FileBlockDevice) -> Result<Vec<Partition>, NxError> {
    parse_gpt(dev).map_err(|e| {
        NxError::new(ExitClass::ValidationReject, format!("update: gpt parse failed ({e:?})"))
    })
}

// ---------------------------------------------------------------- decode --

pub(crate) fn read_bsb(
    dev: &FileBlockDevice,
    parts: &[Partition],
) -> Result<(bootfmt::bsb::Bsb, usize), NxError> {
    let bsb_part = part(parts, &GUID_NEXUS_BSB, "bsb")?;
    let mut blocks = [0u8; 2 * SECTOR];
    dev.read_blocks(bsb_part.first_lba, &mut blocks)
        .map_err(|e| NxError::new(ExitClass::Internal, format!("update: bsb read ({e:?})")))?;
    bootfmt::bsb::pick(&blocks[..SECTOR], &blocks[SECTOR..])
        .ok_or_else(|| NxError::new(ExitClass::ValidationReject, "update: no valid BSB block"))
}

fn slot_label(slot: bootfmt::bsb::Slot) -> &'static str {
    match slot {
        bootfmt::bsb::Slot::A => "a",
        bootfmt::bsb::Slot::B => "b",
    }
}

fn slot_part_name(slot: bootfmt::bsb::Slot) -> &'static str {
    match slot {
        bootfmt::bsb::Slot::A => "boot-a",
        bootfmt::bsb::Slot::B => "boot-b",
    }
}

/// One slot's NXBD view: `None` = blank descriptor sector (empty slot).
struct SlotView {
    desc: Option<bootfmt::nxbd::Nxbd>,
    /// verified | unchecked | reject:<err> | undecodable
    sig: String,
}

fn read_slot(
    dev: &FileBlockDevice,
    parts: &[Partition],
    name: &str,
    pubkey: Option<&[u8; 32]>,
) -> Result<SlotView, NxError> {
    let p = part(parts, &GUID_NEXUS_BOOT, name)?;
    let mut sector = [0u8; SECTOR];
    dev.read_blocks(p.first_lba, &mut sector)
        .map_err(|e| NxError::new(ExitClass::Internal, format!("update: nxbd read ({e:?})")))?;
    if sector.iter().all(|&b| b == 0) {
        return Ok(SlotView { desc: None, sig: "empty".into() });
    }
    match bootfmt::nxbd::decode(&sector) {
        Ok((desc, _sig)) => {
            let sig = match pubkey {
                Some(pk) => match bootfmt::nxbd::verify(&sector, pk) {
                    Ok(_) => "verified".into(),
                    Err(e) => format!("reject:{e:?}"),
                },
                None => "unchecked".into(),
            };
            Ok(SlotView { desc: Some(desc), sig })
        }
        Err(e) => Ok(SlotView { desc: None, sig: format!("undecodable:{e:?}") }),
    }
}

fn slot_json(view: &SlotView) -> Value {
    match &view.desc {
        Some(d) => json!({
            "build_id": d.build_id_str(),
            "rollback_index": d.rollback_index,
            "image_size": d.image_size,
            "sig": view.sig,
        }),
        None => json!({ "empty": true, "sig": view.sig }),
    }
}

fn slot_line(name: &str, view: &SlotView) -> String {
    match &view.desc {
        Some(d) => format!(
            "update: slot-{name} build={} rbidx={} size={} sig={}",
            d.build_id_str(),
            d.rollback_index,
            d.image_size,
            view.sig
        ),
        None => format!("update: slot-{name} {}", view.sig),
    }
}

// ---------------------------------------------------------------- status --

fn handle_status(args: UpdateStatusArgs) -> ExecResult {
    let pubkey = match (&args.pubkey, &args.key) {
        (Some(hexkey), None) => Some(decode_hex32(hexkey).ok_or_else(|| {
            NxError::new(ExitClass::ValidationReject, "update: --pubkey must be 64 hex chars")
        })?),
        (None, Some(seed_path)) => Some(
            bootfmt::nxbd::pubkey_id_for_seed(&crate::commands::image::read_seed(seed_path)?).0,
        ),
        _ => None,
    };
    let dev = open_disk(&args.image)?;
    let parts = disk_parts(&dev)?;
    let (bsb, block) = read_bsb(&dev, &parts)?;
    let slot_a = read_slot(&dev, &parts, "boot-a", pubkey.as_ref())?;
    let slot_b = read_slot(&dev, &parts, "boot-b", pubkey.as_ref())?;

    let message = [
        format!("update: status (image={})", args.image.display()),
        format!(
            "update: bsb active={} next={} tries={} committed={} floor={} seq={} block={block}",
            slot_label(bsb.active_slot),
            bsb.next_slot.map_or("-", slot_label),
            bsb.tries_left,
            if bsb.health_committed { "yes" } else { "no" },
            bsb.rollback_min_index,
            bsb.seq,
        ),
        slot_line("a", &slot_a),
        slot_line("b", &slot_b),
    ]
    .join("\n");
    let data = json!({
        "image": args.image.display().to_string(),
        "bsb": {
            "block": block,
            "seq": bsb.seq,
            "active_slot": slot_label(bsb.active_slot),
            "next_slot": bsb.next_slot.map(slot_label),
            "tries_left": bsb.tries_left,
            "health_committed": bsb.health_committed,
            "rollback_min_index": bsb.rollback_min_index,
        },
        "slot_a": slot_json(&slot_a),
        "slot_b": slot_json(&slot_b),
    });
    Ok((ExitClass::Success, message, args.json, Some(data)))
}

// ----------------------------------------- switch / rollback (preflight) --

fn handle_switch(args: UpdateSwitchArgs) -> ExecResult {
    if args.tries == 0 {
        return Err(NxError::new(ExitClass::Usage, "update: --tries must be non-zero"));
    }
    let dev = open_disk(&args.image)?;
    let parts = disk_parts(&dev)?;
    let (bsb, _) = read_bsb(&dev, &parts)?;
    let target = bsb.active_slot.other();
    let view = read_slot(&dev, &parts, slot_part_name(target), None)?;
    let desc = view.desc.ok_or_else(|| {
        NxError::new(
            ExitClass::ValidationReject,
            format!("update: switch preflight reject (slot-{} {})", slot_label(target), view.sig),
        )
    })?;
    if desc.rollback_index < bsb.rollback_min_index {
        return Err(NxError::new(
            ExitClass::ValidationReject,
            format!(
                "update: switch preflight reject (downgrade {} < min {})",
                desc.rollback_index, bsb.rollback_min_index
            ),
        ));
    }
    let slot_part = part(&parts, &GUID_NEXUS_BOOT, slot_part_name(target))?;
    let budget =
        (slot_part.last_lba - slot_part.first_lba + 1 - IMAGE_START_SECTOR) * SECTOR as u64;
    if desc.image_size > budget {
        return Err(NxError::new(
            ExitClass::ValidationReject,
            "update: switch preflight reject (image exceeds slot budget)",
        ));
    }
    let message = format!(
        "update: switch preflight ok (target={} tries={} build={} rbidx={} floor={}) applied=false",
        slot_label(target),
        args.tries,
        desc.build_id_str(),
        desc.rollback_index,
        bsb.rollback_min_index,
    );
    let data = json!({
        "preflight_only": true,
        "applied": false,
        "target": slot_label(target),
        "tries": args.tries,
        "build_id": desc.build_id_str(),
        "rollback_index": desc.rollback_index,
        "floor": bsb.rollback_min_index,
    });
    Ok((ExitClass::Success, message, args.json, Some(data)))
}

fn handle_rollback(args: UpdateRollbackArgs) -> ExecResult {
    let dev = open_disk(&args.image)?;
    let parts = disk_parts(&dev)?;
    let (bsb, _) = read_bsb(&dev, &parts)?;
    let Some(trial) = bsb.next_slot else {
        return Err(NxError::new(
            ExitClass::ValidationReject,
            "update: rollback preflight reject (no pending trial)",
        ));
    };
    let message = format!(
        "update: rollback preflight ok (standing={} clears trial slot={} tries={}) applied=false",
        slot_label(bsb.active_slot),
        slot_label(trial),
        bsb.tries_left,
    );
    let data = json!({
        "preflight_only": true,
        "applied": false,
        "standing": slot_label(bsb.active_slot),
        "trial": slot_label(trial),
        "tries_left": bsb.tries_left,
    });
    Ok((ExitClass::Success, message, args.json, Some(data)))
}

// --------------------------------------------------------------- helpers --

fn decode_hex32(hex: &str) -> Option<[u8; 32]> {
    if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}
