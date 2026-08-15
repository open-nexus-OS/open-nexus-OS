// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: `nx crash` host commands over crash-dump directories:
//! `ls/show/export/purge/grep` for `.nxcd`, `.nxcd.zst` (canonical) and
//! legacy `.nmd` (minidump v1, converted on load). Dump files are untrusted:
//! reads are size-bounded and every parse failure is a deterministic reject.
//! OWNERS: @reliability
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Process-boundary tests in `tests/crash_cli.rs`
//! ADR: tasks/TASK-0048-crashdump-v2a-host-pipeline-nxsym-nx-crash.md

use crate::cli::{
    CrashAction, CrashArgs, CrashExportArgs, CrashGrepArgs, CrashLsArgs, CrashPurgeArgs,
    CrashShowArgs,
};
use crate::error::{ExecResult, ExitClass, NxError};
use nxcd::{CrashHeader, FramesSection, MapsSection, NxcdContainer, SectionKind};
use serde_json::json;
use std::path::{Path, PathBuf};

/// Bound for directory listings (defensive; a crash dir is budget-managed).
const MAX_DUMPS_PER_DIR: usize = 4096;
/// Bound for reading any dump file from disk (matches the container bound;
/// compressed inputs can only be smaller).
const MAX_DUMP_FILE_BYTES: u64 = nxcd::MAX_TOTAL_NXCD as u64;

pub(crate) fn handle_crash(args: CrashArgs) -> ExecResult {
    match args.action {
        CrashAction::Ls(args) => handle_ls(args),
        CrashAction::Show(args) => handle_show(args),
        CrashAction::Export(args) => handle_export(args),
        CrashAction::Purge(args) => handle_purge(args),
        CrashAction::Grep(args) => handle_grep(args),
    }
}

/// Recognized dump kinds by file name.
fn dump_kind(name: &str) -> Option<&'static str> {
    if name.ends_with(".nxcd.zst") {
        Some("nxcd.zst")
    } else if name.ends_with(".nxcd") {
        Some("nxcd")
    } else if name.ends_with(".nmd") {
        Some("nmd")
    } else {
        None
    }
}

/// Sorted (deterministic) list of dump files in a directory.
fn list_dump_files(dir: &Path) -> Result<Vec<(String, PathBuf)>, NxError> {
    if !dir.is_dir() {
        return Err(NxError::new(
            ExitClass::ValidationReject,
            format!("crash dir does not exist: {}", dir.display()),
        ));
    }
    let entries = std::fs::read_dir(dir)
        .map_err(|e| NxError::new(ExitClass::Internal, format!("read dir failed: {e}")))?;
    let mut out = Vec::new();
    for entry in entries {
        let entry = entry
            .map_err(|e| NxError::new(ExitClass::Internal, format!("dir iteration failed: {e}")))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if dump_kind(&name).is_some() {
            out.push((name, entry.path()));
        }
        if out.len() > MAX_DUMPS_PER_DIR {
            return Err(NxError::new(
                ExitClass::ValidationReject,
                format!("crash dir exceeds {MAX_DUMPS_PER_DIR} dumps; refusing unbounded scan"),
            ));
        }
    }
    out.sort();
    Ok(out)
}

/// Bounded read + decode of one untrusted dump file into a container.
fn load_dump(path: &Path) -> Result<(NxcdContainer, &'static str), NxError> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let kind = dump_kind(&name).ok_or_else(|| {
        NxError::new(
            ExitClass::ValidationReject,
            format!("unsupported dump extension (expect .nxcd/.nxcd.zst/.nmd): {name}"),
        )
    })?;
    let meta = std::fs::metadata(path).map_err(|e| {
        NxError::new(ExitClass::ValidationReject, format!("cannot stat {}: {e}", path.display()))
    })?;
    if meta.len() > MAX_DUMP_FILE_BYTES {
        return Err(NxError::new(
            ExitClass::ValidationReject,
            format!("dump exceeds {MAX_DUMP_FILE_BYTES} bytes: {}", path.display()),
        ));
    }
    let bytes = std::fs::read(path).map_err(|e| {
        NxError::new(ExitClass::Internal, format!("read {} failed: {e}", path.display()))
    })?;
    let reject = |what: &str| {
        NxError::new(ExitClass::ValidationReject, format!("{what}: {}", path.display()))
    };
    let container = match kind {
        "nxcd.zst" => {
            let raw =
                nxcd::decompress_nxcd(&bytes).map_err(|e| reject(&format!("bad zst ({e})")))?;
            NxcdContainer::decode(&raw).map_err(|e| reject(&format!("bad nxcd ({e})")))?
        }
        "nxcd" => NxcdContainer::decode(&bytes).map_err(|e| reject(&format!("bad nxcd ({e})")))?,
        _ => {
            let frame = crash::MinidumpFrame::decode(&bytes)
                .map_err(|e| reject(&format!("bad minidump ({e:?})")))?;
            nxcd::from_minidump(&frame).map_err(|e| reject(&format!("convert failed ({e})")))?
        }
    };
    Ok((container, kind))
}

fn handle_ls(args: CrashLsArgs) -> ExecResult {
    let files = list_dump_files(&args.dir)?;
    let mut rows = Vec::new();
    for (name, path) in &files {
        let bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        match load_dump(path).and_then(|(c, kind)| {
            CrashHeader::from_section(&c)
                .map(|h| (h, kind))
                .map_err(|e| NxError::new(ExitClass::ValidationReject, format!("bad header ({e})")))
        }) {
            Ok((header, kind)) => rows.push(json!({
                "id": name,
                "format": kind,
                "bytes": bytes,
                "valid": true,
                "timestamp_nsec": header.timestamp_nsec,
                "pid": header.pid,
                "code": header.code,
                "name": header.name,
                "build_id": header.build_id,
            })),
            Err(err) => rows.push(json!({
                "id": name,
                "bytes": bytes,
                "valid": false,
                "error": err.message,
            })),
        }
    }
    let message = format!("{} dump(s) in {}", rows.len(), args.dir.display());
    Ok((ExitClass::Success, message, args.json, Some(json!({ "dumps": rows }))))
}

fn handle_show(args: CrashShowArgs) -> ExecResult {
    let (container, kind) = load_dump(&args.path)?;
    let header = CrashHeader::from_section(&container)
        .map_err(|e| NxError::new(ExitClass::ValidationReject, format!("bad header ({e})")))?;
    let mut frames = FramesSection::from_section(&container)
        .map_err(|e| NxError::new(ExitClass::ValidationReject, format!("bad frames ({e})")))?;
    let maps = MapsSection::from_section(&container)
        .map_err(|e| NxError::new(ExitClass::ValidationReject, format!("bad maps ({e})")))?;

    let mut symbolized = false;
    if let Some(sym_path) = &args.sym {
        let bytes = std::fs::read(sym_path).map_err(|e| {
            NxError::new(ExitClass::ValidationReject, format!("cannot read symbols: {e}"))
        })?;
        let index = nxsym::read_index(&bytes)
            .map_err(|e| NxError::new(ExitClass::ValidationReject, format!("bad symbols ({e})")))?;
        for record in &mut frames.frames {
            // Key lookups on the build_id carried in the dump itself
            // (MinidumpFrame.build_id provenance) — never re-derived here.
            if let Ok(Some(hit)) = nxsym::lookup(&index, &record.build_id, record.pc) {
                record.function = Some(hit.function);
                record.file = Some(hit.file);
                record.line = Some(hit.line);
                symbolized = true;
            }
        }
    }

    let sections: Vec<&'static str> = container.kinds().iter().map(|k| k.name()).collect();
    let data = json!({
        "path": args.path,
        "format": kind,
        "sections": sections,
        "header": header,
        "frames": frames.frames,
        "modules": maps.modules,
        "symbolized": symbolized,
    });
    let message = format!(
        "crash {} pid={} code={} build_id={} frames={}",
        header.name,
        header.pid,
        header.code,
        header.build_id,
        frames.frames.len()
    );
    Ok((ExitClass::Success, message, args.json, Some(data)))
}

fn handle_export(args: CrashExportArgs) -> ExecResult {
    let out_name = args.output.file_name().map(|n| n.to_string_lossy().into_owned());
    let compress = match out_name.as_deref() {
        Some(name) if name.ends_with(".nxcd.zst") => true,
        Some(name) if name.ends_with(".nxcd") => false,
        _ => {
            return Err(NxError::new(
                ExitClass::ValidationReject,
                "export output must end with .nxcd or .nxcd.zst",
            ))
        }
    };
    let (container, _) = load_dump(&args.path)?;
    let encoded = container
        .encode()
        .map_err(|e| NxError::new(ExitClass::Internal, format!("encode failed ({e})")))?;
    let payload = if compress {
        nxcd::compress_nxcd(&encoded)
            .map_err(|e| NxError::new(ExitClass::Internal, format!("compress failed ({e})")))?
    } else {
        encoded
    };
    std::fs::write(&args.output, &payload).map_err(|e| {
        NxError::new(ExitClass::Internal, format!("write {} failed: {e}", args.output.display()))
    })?;
    let message = format!("exported {} ({} bytes)", args.output.display(), payload.len());
    let data = json!({
        "output": args.output,
        "bytes": payload.len(),
        "compressed": compress,
    });
    Ok((ExitClass::Success, message, args.json, Some(data)))
}

fn handle_purge(args: CrashPurgeArgs) -> ExecResult {
    let files = list_dump_files(&args.dir)?;
    let budget = nxcd::GcBudget {
        max_total_bytes: args.max_bytes.unwrap_or(u64::MAX),
        max_count: args.max_count.unwrap_or(usize::MAX),
    };
    let mut entries = Vec::new();
    let mut invalid = Vec::new();
    for (name, path) in &files {
        let bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        match load_dump(path).and_then(|(c, _)| {
            CrashHeader::from_section(&c)
                .map_err(|e| NxError::new(ExitClass::ValidationReject, format!("bad header ({e})")))
        }) {
            Ok(header) => entries.push(nxcd::GcEntry {
                id: name.clone(),
                bytes,
                timestamp_nsec: header.timestamp_nsec,
            }),
            // Invalid dumps are never deleted by budget logic; they are
            // reported so an operator can decide.
            Err(_) => invalid.push(name.clone()),
        }
    }
    let plan = nxcd::plan_purge(&entries, &budget);
    let mut deleted = Vec::new();
    if !args.dry_run {
        for id in &plan {
            let path = args.dir.join(id);
            std::fs::remove_file(&path).map_err(|e| {
                NxError::new(ExitClass::Internal, format!("delete {} failed: {e}", path.display()))
            })?;
            deleted.push(id.clone());
        }
    }
    let message = if args.dry_run {
        format!("purge plan: {} of {} dump(s) would be deleted", plan.len(), entries.len())
    } else {
        format!("purged {} of {} dump(s)", deleted.len(), entries.len())
    };
    let data = json!({
        "dir": args.dir,
        "planned": plan,
        "deleted": deleted,
        "kept": entries.len() - plan.len(),
        "invalid": invalid,
        "dry_run": args.dry_run,
    });
    Ok((ExitClass::Success, message, args.json, Some(data)))
}

fn handle_grep(args: CrashGrepArgs) -> ExecResult {
    if args.pattern.is_empty() {
        return Err(NxError::new(ExitClass::ValidationReject, "grep pattern must not be empty"));
    }
    let files = list_dump_files(&args.dir)?;
    let mut matches = Vec::new();
    for (name, path) in &files {
        let Ok((container, _)) = load_dump(path) else { continue };
        if container_matches(&container, &args.pattern) {
            matches.push(name.clone());
        }
    }
    let message = format!("{} dump(s) match \"{}\"", matches.len(), args.pattern);
    Ok((ExitClass::Success, message, args.json, Some(json!({ "matches": matches }))))
}

/// Substring search over the textual sections of a dump.
fn container_matches(container: &NxcdContainer, pattern: &str) -> bool {
    let mut haystacks: Vec<String> = Vec::new();
    if let Ok(header) = CrashHeader::from_section(container) {
        haystacks.push(header.name);
        haystacks.push(header.build_id);
        haystacks.push(header.pid.to_string());
        haystacks.push(header.code.to_string());
    }
    if let Ok(frames) = FramesSection::from_section(container) {
        for frame in frames.frames {
            haystacks.push(format!("0x{:x}", frame.pc));
            if let Some(function) = frame.function {
                haystacks.push(function);
            }
            if let Some(file) = frame.file {
                haystacks.push(file);
            }
        }
    }
    if let Ok(maps) = MapsSection::from_section(container) {
        for module in maps.modules {
            haystacks.push(module.name);
            haystacks.push(module.build_id);
        }
    }
    for kind in [SectionKind::Logs, SectionKind::Spans] {
        if let Some(bytes) = container.get(kind) {
            haystacks.push(String::from_utf8_lossy(bytes).into_owned());
        }
    }
    haystacks.iter().any(|h| h.contains(pattern))
}
