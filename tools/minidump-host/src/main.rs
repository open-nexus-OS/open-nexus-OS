// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: CLI for host-side minidump symbolization output
//! OWNERS: @tools-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: No tests
//! ADR: docs/rfcs/RFC-0031-crashdumps-v1-minidump-host-symbolize.md

use std::path::PathBuf;

fn main() {
    let mut args = std::env::args_os();
    let _bin = args.next();
    let Some(elf) = args.next() else {
        eprintln!("usage: minidump-host <elf-path> <dump-path>");
        std::process::exit(2);
    };
    let Some(dump) = args.next() else {
        eprintln!("usage: minidump-host <elf-path> <dump-path>");
        std::process::exit(2);
    };

    let elf_path = PathBuf::from(elf);
    let dump_path = PathBuf::from(dump);
    let dump_bytes = match std::fs::read(&dump_path) {
        Ok(v) => v,
        Err(err) => {
            eprintln!("read dump failed: {err}");
            std::process::exit(1);
        }
    };

    let rows = match minidump_host::symbolize_minidump_with_elf(&elf_path, &dump_bytes) {
        Ok(v) => v,
        Err(_) => {
            eprintln!("symbolize failed");
            std::process::exit(1);
        }
    };

    for row in rows {
        println!("pc=0x{:016x} fn={} {}:{}", row.pc, row.function, row.file, row.line);
    }
}
