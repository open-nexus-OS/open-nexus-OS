// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! `nxgpu library <manifest.toml> --out <file>`: the pipeline library of a manifest's variants
//! (sources relative to the manifest), its bytes written to `--out`, each entry's facts printed
//! (groups against budget, temporaries, pass type, program bytes) and the archive's size and
//! FNV-1a 64 digest.

use std::path::Path;
use std::process::ExitCode;

use nexus_gfx::backend::gpu::scene::fnv1a64;
use nxgpu::library::{build, parse_manifest};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("nxgpu: {e}");
            ExitCode::from(2)
        }
    }
}

fn run(args: &[String]) -> Result<(), String> {
    let usage = "usage: nxgpu library <manifest.toml> --out <file>";
    let (Some("library"), Some(path)) = (args.first().map(String::as_str), args.get(1)) else {
        return Err(usage.into());
    };
    let out = args.iter().position(|a| a == "--out").and_then(|i| args.get(i + 1)).ok_or(usage)?;
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let manifest = parse_manifest(&text).map_err(|e| format!("{path}: {e}"))?;
    let dir = Path::new(path).parent().unwrap_or(Path::new("."));
    let source = |rel: &str| {
        let p = dir.join(rel);
        std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))
    };
    let (bytes, built) = build(&manifest, source).map_err(|e| format!("{path}: {e}"))?;
    std::fs::write(out, &bytes).map_err(|e| format!("{out}: {e}"))?;
    for b in &built {
        println!(
            "  {} {}: groups={}/{} temps={} pass={} bytes={}",
            b.id,
            b.name,
            b.groups,
            b.budget,
            b.temps,
            if b.pass_type == 0 { "opaque" } else { "translucent" },
            b.bytes
        );
    }
    println!("{out}: entries={} bytes={} fnv=0x{:016x}", built.len(), bytes.len(), fnv1a64(&bytes));
    Ok(())
}
