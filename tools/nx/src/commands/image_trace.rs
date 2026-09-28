// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: the boot trace's reader (RFC-0107, TASK-0327B): each kept boot's console text from a
//! disk image — its `trace` partition found by the layout's name and type — or from a dump of
//! that partition alone, which is what `just board-log` pulls from the board's stock system. The
//! latest boot by default, every kept boot oldest first with `--all`, one run's boots with
//! `--since <seq>`, one boot with `--seq <n>`; the loader's text only with `--loader`, the OS
//! text only with `--os`. Records are read through `storage::trace`, which counts a slot only
//! with its magic, version, CRC and in-bounds lengths. The text goes out byte for byte (`--out`),
//! so the proof manifest judges it like a `uart.log`.
//! OWNERS: @devx @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: tests/image_trace_cli.rs

use serde_json::json;
use storage::gpt::{parse_gpt, Partition, GUID_NEXUS_TRACE};
use storage::trace::{
    self, Record, TraceError, LOADER_COMPLETE, LOADER_OVERFLOW, OS_COMPLETE, OS_OVERFLOW,
};
use storage::BlockDevice;

use crate::cli_image::ImageTraceArgs;
use crate::commands::image::FileBlockDevice;
use crate::error::{ExecResult, ExitClass, NxError};

pub(crate) fn handle_trace(args: ImageTraceArgs) -> ExecResult {
    let dev = FileBlockDevice::open_ro(&args.image).map_err(|err| {
        NxError::new(
            ExitClass::MissingDependency,
            format!("image: open {}: {err}", args.image.display()),
        )
    })?;
    // A disk's `trace` partition, or — no GPT naming one — the file as that partition's dump.
    let part =
        parse_gpt(&dev).ok().and_then(|parts| trace::partition(&parts)).unwrap_or(Partition {
            type_guid: GUID_NEXUS_TRACE,
            first_lba: 0,
            last_lba: dev.block_count().saturating_sub(1),
            name: trace::PARTITION.into(),
        });
    let records = trace::records(&dev, &part).map_err(|e| match e {
        TraceError::TooSmall | TraceError::NoPartition | TraceError::NotThisBoot => NxError::new(
            ExitClass::ValidationReject,
            "image: neither a disk with a trace partition nor a dump of one",
        ),
        TraceError::Io => NxError::new(ExitClass::Internal, "image: reading the trace failed"),
    })?;
    let Some(latest) = records.last() else {
        return Err(NxError::new(ExitClass::ValidationReject, "image: the trace keeps no boot"));
    };
    let chosen: Vec<&Record> = match (args.seq, args.since) {
        (Some(seq), _) => records.iter().filter(|r| r.header.seq == seq).collect(),
        (None, Some(from)) => records.iter().filter(|r| r.header.seq >= from).collect(),
        (None, None) if args.all => records.iter().collect(),
        (None, None) => vec![latest],
    };
    if chosen.is_empty() {
        let which = match args.seq {
            Some(seq) => format!("boot {seq}"),
            None => format!("boot from seq {} on", args.since.unwrap_or_default()),
        };
        return Err(NxError::new(
            ExitClass::ValidationReject,
            format!("image: the trace keeps no {which} (the latest is {})", latest.header.seq),
        ));
    }
    let mut text = Vec::new();
    for r in &chosen {
        if !args.os {
            text.extend_from_slice(&r.loader);
        }
        if !args.loader {
            text.extend_from_slice(&r.os);
        }
    }
    let flag = |r: &Record, bit: u32| r.header.flags & bit != 0;
    let boots: Vec<serde_json::Value> = chosen
        .iter()
        .map(|r| {
            json!({
                "seq": r.header.seq, "slot": r.header.slot,
                "loader_bytes": r.loader.len(), "loader_complete": flag(r, LOADER_COMPLETE),
                "loader_overflow": flag(r, LOADER_OVERFLOW),
                "os_bytes": r.os.len(), "os_complete": flag(r, OS_COMPLETE),
                "os_overflow": flag(r, OS_OVERFLOW),
            })
        })
        .collect();
    let meta = json!({ "kept": records.len(), "boots": boots });
    if let Some(out) = &args.out {
        std::fs::write(out, &text).map_err(|err| {
            NxError::new(ExitClass::Internal, format!("image: write {}: {err}", out.display()))
        })?;
        let msg = format!("image: trace of {} boot(s) -> {}", chosen.len(), out.display());
        return Ok((ExitClass::Success, msg, args.json, Some(meta)));
    }
    if args.json {
        return Ok((ExitClass::Success, "image: trace".into(), true, Some(meta)));
    }
    let shown = String::from_utf8_lossy(&text).trim_end_matches('\n').to_string();
    Ok((ExitClass::Success, shown, false, None))
}
