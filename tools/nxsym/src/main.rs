// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: `nxsym` CLI: `index` builds a Build-ID keyed `symbols.nxsym` from
//! ELF inputs; `addr2line` resolves an address against a previously built
//! index (Build-ID selected explicitly, or implicitly for single-binary
//! indexes).
//! OWNERS: @reliability
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Process-boundary tests in `tests/cli.rs`
//! ADR: tasks/TASK-0048-crashdump-v2a-host-pipeline-nxsym-nx-crash.md

#![forbid(unsafe_code)]

use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "nxsym")]
#[command(about = "Build-ID keyed symbol indexer and lookup (host)")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Index ELF files into a symbols.nxsym file.
    Index(IndexArgs),
    /// Resolve an address against a symbols.nxsym file.
    Addr2line(Addr2lineArgs),
}

#[derive(Args, Debug)]
struct IndexArgs {
    /// ELF files to index.
    #[arg(required = true)]
    elves: Vec<PathBuf>,
    /// Output index path.
    #[arg(short, long)]
    output: PathBuf,
}

#[derive(Args, Debug)]
struct Addr2lineArgs {
    /// Path to symbols.nxsym.
    #[arg(long)]
    sym: PathBuf,
    /// Address to resolve (hex with 0x prefix, or decimal).
    #[arg(long)]
    addr: String,
    /// Build-ID key (defaults to the only binary in single-binary indexes).
    #[arg(long)]
    build_id: Option<String>,
}

fn main() {
    let cli = Cli::parse();
    let code = match cli.command {
        Command::Index(args) => run_index(&args),
        Command::Addr2line(args) => run_addr2line(&args),
    };
    std::process::exit(code);
}

fn run_index(args: &IndexArgs) -> i32 {
    match nxsym::build_index(&args.elves) {
        Ok(index) => match nxsym::write_index(&index) {
            Ok(bytes) => match std::fs::write(&args.output, bytes) {
                Ok(()) => {
                    println!(
                        "nxsym: indexed {} binaries -> {}",
                        index.binaries.len(),
                        args.output.display()
                    );
                    for binary in &index.binaries {
                        let source = if binary.fallback_id { "fallback" } else { "gnu-note" };
                        println!(
                            "  {} {} ({} entries, {})",
                            binary.build_id,
                            binary.name,
                            binary.entries.len(),
                            source
                        );
                    }
                    0
                }
                Err(err) => fail(&format!("write {}: {err}", args.output.display())),
            },
            Err(err) => fail(&format!("encode index: {err}")),
        },
        Err(err) => fail(&format!("index: {err}")),
    }
}

fn run_addr2line(args: &Addr2lineArgs) -> i32 {
    let addr = match parse_addr(&args.addr) {
        Some(addr) => addr,
        None => return fail(&format!("invalid address: {}", args.addr)),
    };
    let bytes = match std::fs::read(&args.sym) {
        Ok(bytes) => bytes,
        Err(err) => return fail(&format!("read {}: {err}", args.sym.display())),
    };
    let index = match nxsym::read_index(&bytes) {
        Ok(index) => index,
        Err(err) => return fail(&format!("parse index: {err}")),
    };
    let build_id = match &args.build_id {
        Some(id) => id.clone(),
        None => {
            if index.binaries.len() == 1 {
                index.binaries[0].build_id.clone()
            } else {
                return fail("--build-id required for multi-binary indexes");
            }
        }
    };
    match nxsym::lookup(&index, &build_id, addr) {
        Ok(Some(frame)) => {
            println!("0x{addr:x} {} at {}:{}", frame.function, frame.file, frame.line);
            0
        }
        Ok(None) => {
            println!("0x{addr:x} ?? (build-id {build_id}, address not covered)");
            0
        }
        Err(err) => fail(&format!("lookup: {err}")),
    }
}

fn parse_addr(input: &str) -> Option<u64> {
    if let Some(hex) = input.strip_prefix("0x").or_else(|| input.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).ok()
    } else {
        input.parse::<u64>().ok()
    }
}

fn fail(message: &str) -> i32 {
    eprintln!("nxsym: error: {message}");
    1
}
