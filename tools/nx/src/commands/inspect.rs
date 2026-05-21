// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: `nx inspect` artifact inspection commands.
//! OWNERS: @tools-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered by `nx` command tests.
//! ADR: docs/adr/0021-structured-data-formats-json-vs-capnp.md

use crate::cli::{InspectArgs, InspectNxbArgs, InspectTarget};
use crate::error::{ExecResult, ExitClass, NxError};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::Path;

pub(crate) fn handle_inspect(args: InspectArgs) -> ExecResult {
    match args.target {
        InspectTarget::Nxb(nxb) => handle_inspect_nxb(nxb),
    }
}

fn handle_inspect_nxb(args: InspectNxbArgs) -> ExecResult {
    if !args.path.exists() || !args.path.is_dir() {
        return Err(NxError::new(
            ExitClass::ValidationReject,
            "inspect nxb requires an existing directory",
        ));
    }

    let mut manifest_files = Vec::new();
    let mut meta_files = Vec::new();
    let mut payload_sha256 = None;

    let entries = fs::read_dir(&args.path).map_err(|e| {
        NxError::new(
            ExitClass::Internal,
            format!("failed to read directory: {e}"),
        )
    })?;
    for entry in entries {
        let entry = entry.map_err(|e| {
            NxError::new(
                ExitClass::Internal,
                format!("failed to iterate directory: {e}"),
            )
        })?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("manifest.") {
            manifest_files.push(name.to_string());
        }
    }
    manifest_files.sort();

    let payload_path = args.path.join("payload.elf");
    if payload_path.exists() {
        let mut file = fs::File::open(&payload_path).map_err(|e| {
            NxError::new(
                ExitClass::Internal,
                format!("failed opening payload.elf: {e}"),
            )
        })?;
        let mut hasher = Sha256::new();
        let mut buf = [0_u8; 8192];
        loop {
            let read = file.read(&mut buf).map_err(|e| {
                NxError::new(
                    ExitClass::Internal,
                    format!("failed reading payload.elf: {e}"),
                )
            })?;
            if read == 0 {
                break;
            }
            hasher.update(&buf[..read]);
        }
        payload_sha256 = Some(format!("{:x}", hasher.finalize()));
    }

    let meta_dir = args.path.join("meta");
    if meta_dir.exists() && meta_dir.is_dir() {
        collect_files(&meta_dir, &mut meta_files, &meta_dir)?;
        meta_files.sort();
    }

    let data = json!({
        "path": args.path,
        "manifest_files": manifest_files,
        "payload_present": payload_path.exists(),
        "payload_sha256": payload_sha256,
        "meta_files": meta_files,
    });
    Ok((
        ExitClass::Success,
        "inspect nxb summary generated".to_string(),
        args.json,
        Some(data),
    ))
}

fn collect_files(root: &Path, out: &mut Vec<String>, strip_prefix: &Path) -> Result<(), NxError> {
    for entry in fs::read_dir(root)
        .map_err(|e| NxError::new(ExitClass::Internal, format!("failed reading meta dir: {e}")))?
    {
        let entry = entry.map_err(|e| {
            NxError::new(
                ExitClass::Internal,
                format!("failed iterating meta dir: {e}"),
            )
        })?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, out, strip_prefix)?;
        } else {
            let rel = path.strip_prefix(strip_prefix).map_err(|e| {
                NxError::new(ExitClass::Internal, format!("failed strip prefix: {e}"))
            })?;
            out.push(rel.display().to_string());
        }
    }
    Ok(())
}
