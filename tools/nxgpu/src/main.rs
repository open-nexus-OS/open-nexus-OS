// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! `nxgpu build <source.wgsl> --entry <name> [--override <key>=<value>]… --blend
//! replace|premultiplied [--unfused] --out <file>`: a fragment program compiled for the board GPU
//! with its overrides fixed (`true`/`false` or a number), its bytes written to `--out`, its facts
//! printed (groups, temporaries, pass type, bytes, FNV-1a 64 digest).

use std::process::ExitCode;

use nexus_gfx::backend::gpu::scene::fnv1a64;
use nexus_gfx::shader::rogue::encode_program;
use nxgpu::{compile_fragment, Blend};

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
    let usage = "usage: nxgpu build <source.wgsl> --entry <name> [--override <key>=<value>]... --blend replace|premultiplied [--unfused] --out <file>";
    let (Some("build"), Some(src_path)) = (args.first().map(String::as_str), args.get(1)) else {
        return Err(usage.into());
    };
    let flag =
        |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let entry = flag("--entry").unwrap_or_else(|| "main".into());
    let out = flag("--out").ok_or(usage)?;
    let blend = match flag("--blend").as_deref() {
        Some("replace") | None => Blend::Replace,
        Some("premultiplied") => Blend::OverPremultiplied,
        Some(other) => return Err(format!("unknown blend {other}")),
    };
    let fused = !args.iter().any(|a| a == "--unfused");
    let mut overrides = Vec::new();
    for (i, a) in args.iter().enumerate() {
        if a != "--override" {
            continue;
        }
        let given = args.get(i + 1).ok_or(usage)?;
        let (key, value) = given.split_once('=').ok_or_else(|| format!("override {given}"))?;
        let value = match value {
            "true" => 1.0,
            "false" => 0.0,
            v => v.parse::<f64>().map_err(|_| format!("override {given}"))?,
        };
        overrides.push((key, value));
    }
    let src = std::fs::read_to_string(src_path).map_err(|e| format!("{src_path}: {e}"))?;
    let p = compile_fragment(&src, &entry, &overrides, blend, fused)
        .map_err(|e| format!("{src_path}: {e:?}"))?;
    let bytes = encode_program(&p.groups).map_err(|e| format!("encode: {e:?}"))?;
    std::fs::write(&out, &bytes).map_err(|e| format!("{out}: {e}"))?;
    println!(
        "{out}: groups={} temps={} pass={} bytes={} fnv=0x{:016x}",
        p.groups.len(),
        p.temps,
        if p.pass_type == 0 { "opaque" } else { "translucent" },
        bytes.len(),
        fnv1a64(&bytes)
    );
    Ok(())
}
