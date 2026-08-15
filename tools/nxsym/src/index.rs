// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: `symbols.nxsym` index: CBOR file keyed by Build-ID with per-binary
//! address→(function, file, line) entries precomputed at index time (lookup
//! never needs the ELF again). Stable ordering everywhere: binaries sort by
//! Build-ID, entries sort by address. Reads are bounded and validated.
//! Resolution granularity is the function entry line (documented limitation).
//! OWNERS: @reliability
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Unit tests below; integration in `tests/crashdump_v2_host`
//! ADR: tasks/TASK-0048-crashdump-v2a-host-pipeline-nxsym-nx-crash.md

use crate::build_id::{build_id_for_elf, file_stem, BuildIdSource};
use crate::NxsymError;
use object::{Object, ObjectSymbol};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const INDEX_MAGIC: &str = "nxsym1";
pub const INDEX_VERSION: u16 = 1;
/// Bound for a `symbols.nxsym` file read from disk.
pub const MAX_INDEX_BYTES: usize = 64 * 1024 * 1024;
/// Bound for one ELF input during indexing.
pub const MAX_ELF_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_BINARIES: usize = 256;
pub const MAX_ENTRIES_PER_BINARY: usize = 262_144;
pub const MAX_STRING_LEN: usize = 512;
pub const MAX_BUILD_ID_LEN: usize = 128;

/// One resolved function range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolEntry {
    pub addr: u64,
    pub size: u64,
    pub function: String,
    pub file: String,
    pub line: u32,
}

/// All entries for one binary, keyed by Build-ID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BinaryIndex {
    pub build_id: String,
    pub name: String,
    /// True when the Build-ID came from the deterministic fallback.
    pub fallback_id: bool,
    pub entries: Vec<SymbolEntry>,
}

/// Root of a `symbols.nxsym` file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolIndex {
    pub magic: String,
    pub version: u16,
    pub binaries: Vec<BinaryIndex>,
}

/// Lookup result for one address.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolvedFrame {
    pub function: String,
    pub file: String,
    pub line: u32,
}

/// Index a set of ELF files into one `SymbolIndex` (deterministic output).
pub fn build_index(paths: &[std::path::PathBuf]) -> Result<SymbolIndex, NxsymError> {
    if paths.len() > MAX_BINARIES {
        return Err(NxsymError::OversizeInput);
    }
    let mut binaries = Vec::with_capacity(paths.len());
    for path in paths {
        binaries.push(index_one(path)?);
    }
    binaries.sort_by(|a, b| a.build_id.cmp(&b.build_id));
    for pair in binaries.windows(2) {
        if pair[0].build_id == pair[1].build_id {
            return Err(NxsymError::DuplicateBuildId);
        }
    }
    Ok(SymbolIndex { magic: String::from(INDEX_MAGIC), version: INDEX_VERSION, binaries })
}

fn index_one(path: &Path) -> Result<BinaryIndex, NxsymError> {
    let meta = std::fs::metadata(path).map_err(|_| NxsymError::Io)?;
    if meta.len() > MAX_ELF_BYTES {
        return Err(NxsymError::OversizeInput);
    }
    let bytes = std::fs::read(path).map_err(|_| NxsymError::Io)?;
    let name = file_stem(path);
    let (build_id, source) = build_id_for_elf(&name, &bytes)?;

    let file = object::File::parse(&*bytes).map_err(|_| NxsymError::ElfParse)?;
    let loader = addr2line::Loader::new(path).map_err(|_| NxsymError::LoaderOpen)?;

    let mut entries = Vec::new();
    for sym in file.symbols() {
        if sym.kind() != object::SymbolKind::Text {
            continue;
        }
        let raw_name = match sym.name() {
            Ok(n) if !n.is_empty() => n,
            _ => continue,
        };
        let addr = sym.address();
        let (function, file_name, line) = resolve_at(&loader, addr, raw_name);
        entries.push(SymbolEntry {
            addr,
            size: sym.size(),
            function: bounded_string(&function),
            file: bounded_string(&file_name),
            line,
        });
        if entries.len() > MAX_ENTRIES_PER_BINARY {
            return Err(NxsymError::OversizeInput);
        }
    }
    entries.sort_by(|a, b| a.addr.cmp(&b.addr).then_with(|| a.function.cmp(&b.function)));
    entries.dedup_by(|a, b| a.addr == b.addr);
    Ok(BinaryIndex {
        build_id,
        name: bounded_string(&name),
        fallback_id: source == BuildIdSource::Fallback,
        entries,
    })
}

fn resolve_at(loader: &addr2line::Loader, addr: u64, raw_name: &str) -> (String, String, u32) {
    let mut function = String::new();
    let mut file = String::new();
    let mut line = 0u32;
    if let Ok(mut frames) = loader.find_frames(addr) {
        while let Ok(Some(frame)) = frames.next() {
            if function.is_empty() {
                if let Some(func) = frame.function {
                    if let Ok(name) = func.demangle() {
                        function = name.into_owned();
                    }
                }
            }
            if file.is_empty() {
                if let Some(loc) = frame.location {
                    if let Some(path) = loc.file {
                        file = String::from(path);
                    }
                    if let Some(ln) = loc.line {
                        line = ln;
                    }
                }
            }
            if !function.is_empty() && !file.is_empty() {
                break;
            }
        }
    }
    if function.is_empty() {
        function =
            addr2line::demangle_auto(std::borrow::Cow::Borrowed(raw_name), None).into_owned();
    }
    if file.is_empty() {
        file = String::from("<unknown>");
    }
    (function, file, line)
}

/// Serialize an index to CBOR bytes.
pub fn write_index(index: &SymbolIndex) -> Result<Vec<u8>, NxsymError> {
    validate_index(index)?;
    let mut out = Vec::new();
    ciborium::ser::into_writer(index, &mut out).map_err(|_| NxsymError::IndexEncode)?;
    if out.len() > MAX_INDEX_BYTES {
        return Err(NxsymError::OversizeInput);
    }
    Ok(out)
}

/// Bounded parse + validation of an untrusted `symbols.nxsym` payload.
pub fn read_index(bytes: &[u8]) -> Result<SymbolIndex, NxsymError> {
    if bytes.len() > MAX_INDEX_BYTES {
        return Err(NxsymError::OversizeInput);
    }
    let index: SymbolIndex =
        ciborium::de::from_reader(bytes).map_err(|_| NxsymError::IndexDecode)?;
    validate_index(&index)?;
    Ok(index)
}

fn validate_index(index: &SymbolIndex) -> Result<(), NxsymError> {
    if index.magic != INDEX_MAGIC {
        return Err(NxsymError::BadIndexMagic);
    }
    if index.version != INDEX_VERSION {
        return Err(NxsymError::BadIndexMagic);
    }
    if index.binaries.len() > MAX_BINARIES {
        return Err(NxsymError::IndexInvariant);
    }
    for pair in index.binaries.windows(2) {
        if pair[0].build_id >= pair[1].build_id {
            return Err(NxsymError::IndexInvariant);
        }
    }
    for binary in &index.binaries {
        if binary.build_id.is_empty()
            || binary.build_id.len() > MAX_BUILD_ID_LEN
            || binary.name.len() > MAX_STRING_LEN
            || binary.entries.len() > MAX_ENTRIES_PER_BINARY
        {
            return Err(NxsymError::IndexInvariant);
        }
        for pair in binary.entries.windows(2) {
            if pair[0].addr >= pair[1].addr {
                return Err(NxsymError::IndexInvariant);
            }
        }
        for entry in &binary.entries {
            if entry.function.len() > MAX_STRING_LEN || entry.file.len() > MAX_STRING_LEN {
                return Err(NxsymError::IndexInvariant);
            }
        }
    }
    Ok(())
}

/// Resolve one address for a Build-ID taken from a crash artifact
/// (`MinidumpFrame.build_id` / `.nxcd` frame record).
///
/// `Ok(None)` means the id is known but the address is not covered.
pub fn lookup(
    index: &SymbolIndex,
    build_id: &str,
    addr: u64,
) -> Result<Option<ResolvedFrame>, NxsymError> {
    let binary = match index.binaries.binary_search_by(|b| b.build_id.as_str().cmp(build_id)) {
        Ok(pos) => &index.binaries[pos],
        Err(_) => return Err(NxsymError::UnknownBuildId),
    };
    let pos = match binary.entries.binary_search_by(|e| e.addr.cmp(&addr)) {
        Ok(pos) => pos,
        Err(0) => return Ok(None),
        Err(insert) => insert - 1,
    };
    let entry = &binary.entries[pos];
    let end = if entry.size > 0 {
        entry.addr.saturating_add(entry.size)
    } else {
        match binary.entries.get(pos + 1) {
            Some(next) => next.addr,
            None => entry.addr.saturating_add(1),
        }
    };
    if addr >= end {
        return Ok(None);
    }
    Ok(Some(ResolvedFrame {
        function: entry.function.clone(),
        file: entry.file.clone(),
        line: entry.line,
    }))
}

fn bounded_string(input: &str) -> String {
    if input.len() <= MAX_STRING_LEN {
        return String::from(input);
    }
    let mut cut = MAX_STRING_LEN;
    while cut > 0 && !input.is_char_boundary(cut) {
        cut -= 1;
    }
    String::from(&input[..cut])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_index() -> SymbolIndex {
        SymbolIndex {
            magic: String::from(INDEX_MAGIC),
            version: INDEX_VERSION,
            binaries: vec![BinaryIndex {
                build_id: String::from("bdeadbeef"),
                name: String::from("demo"),
                fallback_id: true,
                entries: vec![
                    SymbolEntry {
                        addr: 0x1000,
                        size: 0x20,
                        function: String::from("alpha"),
                        file: String::from("src/a.rs"),
                        line: 10,
                    },
                    SymbolEntry {
                        addr: 0x2000,
                        size: 0,
                        function: String::from("beta"),
                        file: String::from("src/b.rs"),
                        line: 20,
                    },
                    SymbolEntry {
                        addr: 0x3000,
                        size: 0x10,
                        function: String::from("gamma"),
                        file: String::from("src/c.rs"),
                        line: 30,
                    },
                ],
            }],
        }
    }

    #[test]
    fn test_index_cbor_roundtrip_is_identical() {
        let index = sample_index();
        let bytes = write_index(&index).expect("write");
        let got = read_index(&bytes).expect("read");
        assert_eq!(got, index);
        assert_eq!(write_index(&got).expect("re-write"), bytes);
    }

    #[test]
    fn test_lookup_inside_and_between_ranges() {
        let index = sample_index();
        let hit = lookup(&index, "bdeadbeef", 0x1010).expect("lookup").expect("frame");
        assert_eq!(hit.function, "alpha");
        assert_eq!(hit.line, 10);
        // Zero-size entry extends to the next entry start.
        let hit = lookup(&index, "bdeadbeef", 0x2fff).expect("lookup").expect("frame");
        assert_eq!(hit.function, "beta");
        // Gap between alpha end and beta start is uncovered.
        assert_eq!(lookup(&index, "bdeadbeef", 0x1020).expect("lookup"), None);
        // Below the first entry is uncovered.
        assert_eq!(lookup(&index, "bdeadbeef", 0x1).expect("lookup"), None);
    }

    #[test]
    fn test_reject_lookup_unknown_build_id() {
        let index = sample_index();
        assert_eq!(lookup(&index, "missing", 0x1000), Err(NxsymError::UnknownBuildId));
    }

    #[test]
    fn test_reject_index_bad_magic() {
        let mut index = sample_index();
        index.magic = String::from("wrong");
        assert_eq!(write_index(&index), Err(NxsymError::BadIndexMagic));
        let mut ok = Vec::new();
        ciborium::ser::into_writer(&index, &mut ok).expect("cbor");
        assert_eq!(read_index(&ok), Err(NxsymError::BadIndexMagic));
    }

    #[test]
    fn test_reject_index_unsorted_entries() {
        let mut index = sample_index();
        index.binaries[0].entries.swap(0, 2);
        let mut bytes = Vec::new();
        ciborium::ser::into_writer(&index, &mut bytes).expect("cbor");
        assert_eq!(read_index(&bytes), Err(NxsymError::IndexInvariant));
    }

    #[test]
    fn test_reject_index_truncated_cbor() {
        let bytes = write_index(&sample_index()).expect("write");
        assert_eq!(read_index(&bytes[..bytes.len() / 2]), Err(NxsymError::IndexDecode));
    }

    #[test]
    fn test_reject_index_oversize_input() {
        let bytes = vec![0u8; MAX_INDEX_BYTES + 1];
        assert_eq!(read_index(&bytes), Err(NxsymError::OversizeInput));
    }

    #[test]
    fn test_bounded_string_respects_char_boundaries() {
        let long = "ä".repeat(MAX_STRING_LEN);
        let out = bounded_string(&long);
        assert!(out.len() <= MAX_STRING_LEN);
        assert!(long.starts_with(&out));
    }
}
