// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Host-side crashdump v1 symbolization helpers
//! OWNERS: @tools-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 2 unit tests
//! ADR: docs/rfcs/RFC-0031-crashdumps-v1-minidump-host-symbolize.md

use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolizedPc {
    pub pc: u64,
    pub function: String,
    pub file: String,
    pub line: u32,
}

#[derive(Debug)]
pub enum SymbolizeError {
    DumpDecode,
    LoaderOpen,
}

#[must_use = "symbolization result must be handled by caller"]
pub fn symbolize_minidump_with_elf(
    elf_path: &Path,
    dump_bytes: &[u8],
) -> Result<Vec<SymbolizedPc>, SymbolizeError> {
    let dump = crash::MinidumpFrame::decode(dump_bytes).map_err(|_| SymbolizeError::DumpDecode)?;
    let loader = addr2line::Loader::new(elf_path).map_err(|_| SymbolizeError::LoaderOpen)?;
    let mut out = Vec::with_capacity(dump.pcs.len());
    for pc in dump.pcs {
        let mut function = String::from("<unknown>");
        let mut file = String::from("<unknown>");
        let mut line = 0u32;

        if let Ok(mut frames) = loader.find_frames(pc) {
            loop {
                match frames.next() {
                    Ok(Some(frame)) => {
                        if function == "<unknown>" {
                            if let Some(func) = frame.function {
                                if let Ok(name) = func.demangle() {
                                    function = name.into_owned();
                                }
                            }
                        }
                        if file == "<unknown>" {
                            if let Some(loc) = frame.location {
                                if let Some(path) = loc.file {
                                    file = String::from(path);
                                }
                                if let Some(ln) = loc.line {
                                    line = ln;
                                }
                            }
                        }
                        if function != "<unknown>" && file != "<unknown>" && line != 0 {
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(_) => break,
                }
            }
        }

        out.push(SymbolizedPc { pc, function, file, line });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use object::{Object, ObjectSymbol};

    #[inline(never)]
    #[no_mangle]
    pub extern "C" fn test_symbolization_fixture() -> u64 {
        42
    }

    #[test]
    fn test_symbolize_minidump_fixture_pc() {
        let _ = test_symbolization_fixture();
        let exe = std::env::current_exe().expect("current exe");
        let bytes = std::fs::read(&exe).expect("read exe");
        let obj = object::File::parse(&*bytes).expect("parse object");
        let mut fixture_pc = None;
        for sym in obj.symbols() {
            if let Ok(name) = sym.name() {
                if name.contains("test_symbolization_fixture") {
                    fixture_pc = Some(sym.address());
                    break;
                }
            }
        }
        let pc = fixture_pc.expect("fixture symbol address");

        let dump = crash::MinidumpFrame {
            timestamp_nsec: 1,
            pid: 123,
            code: 42,
            name: String::from("demo.minidump"),
            build_id: crash::deterministic_build_id("demo.minidump"),
            pcs: vec![pc],
            stack_preview: vec![0x11; 16],
            code_preview: vec![0x22; 8],
        };
        let dump_bytes = dump.encode().expect("encode dump");
        let out = symbolize_minidump_with_elf(&exe, &dump_bytes).expect("symbolize");
        assert_eq!(out.len(), 1);
        assert!(out[0].function.contains("test_symbolization_fixture"));
        assert_ne!(out[0].file, "<unknown>");
        assert_ne!(out[0].line, 0);
    }

    #[test]
    fn test_symbolize_minidump_is_deterministic() {
        let _ = test_symbolization_fixture();
        let exe = std::env::current_exe().expect("current exe");
        let bytes = std::fs::read(&exe).expect("read exe");
        let obj = object::File::parse(&*bytes).expect("parse object");
        let mut fixture_pc = None;
        for sym in obj.symbols() {
            if let Ok(name) = sym.name() {
                if name.contains("test_symbolization_fixture") {
                    fixture_pc = Some(sym.address());
                    break;
                }
            }
        }
        let pc = fixture_pc.expect("fixture symbol address");
        let dump = crash::MinidumpFrame {
            timestamp_nsec: 1,
            pid: 123,
            code: 42,
            name: String::from("demo.minidump"),
            build_id: crash::deterministic_build_id("demo.minidump"),
            pcs: vec![pc],
            stack_preview: vec![0x11; 16],
            code_preview: vec![0x22; 8],
        };
        let dump_bytes = dump.encode().expect("encode dump");
        let a = symbolize_minidump_with_elf(&exe, &dump_bytes).expect("symbolize");
        let b = symbolize_minidump_with_elf(&exe, &dump_bytes).expect("symbolize");
        assert_eq!(a, b);
    }

    #[test]
    fn test_symbolized_pc_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SymbolizedPc>();
    }
}
