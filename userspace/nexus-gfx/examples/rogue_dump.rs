// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Prints a shader-core program's groups as `nexus_gfx::shader::rogue` decodes them — offset,
//! header, instructions with their phases, parts — the disassembly a backend author reads:
//!
//! ```text
//! cargo run -p nexus-gfx --example rogue_dump -- resources/gpu/programs/frag-base.usc [first] [count]
//! ```

use nexus_gfx::shader::rogue::decode_program;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let Some(path) = args.get(1) else {
        eprintln!("usage: rogue_dump <program.usc> [first group] [count]");
        std::process::exit(2);
    };
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("{path}: {e}");
            std::process::exit(1);
        }
    };
    let first: usize = args.get(2).and_then(|a| a.parse().ok()).unwrap_or(0);
    let count: usize = args.get(3).and_then(|a| a.parse().ok()).unwrap_or(usize::MAX);
    let groups = match decode_program(&bytes) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("{path}: {e:?}");
            std::process::exit(1);
        }
    };
    let mut at = 0usize;
    for (i, g) in groups.iter().enumerate() {
        let size = g.size();
        if i >= first && i - first < count {
            println!("{i:04} @{at:#06x} {:?} bytes={}", g.header, hex(&bytes[at..at + size]));
            for (phase, instr) in &g.instrs {
                println!("       {phase:?}: {instr:?}");
            }
            if let Some(s) = g.lower {
                println!("       lower: {s:?}");
            }
            if let Some(s) = g.upper {
                println!("       upper: {s:?}");
            }
            if let Some(s) = g.iss {
                println!("       iss: {s:?}");
            }
            if let Some(d) = g.dests {
                println!("       dests: {d:?}");
            }
        }
        at += size;
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
