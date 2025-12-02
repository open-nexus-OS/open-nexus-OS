//! CONTEXT: Nexus bundle packer tool
//! INTENT: Package ELF binaries into deployable bundles with manifest
//! IDL (target): pack(input_elf, output_dir), packHello(output_dir)
//! DEPS: exec-payloads (hello ELF), std::fs (file operations)
//! READINESS: Command-line tool; no service dependencies
//! TESTS: Pack hello ELF; pack custom ELF; validate output structure
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use exec_payloads::HELLO_ELF;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let first = args.next().ok_or_else(|| usage("missing input ELF path"))?;
    let (input, output) = if first == "--hello" {
        let output = args
            .next()
            .ok_or_else(|| usage("missing output directory"))?;
        (None, output)
    } else {
        let output = args
            .next()
            .ok_or_else(|| usage("missing output directory"))?;
        (Some(first), output)
    };
    if args.next().is_some() {
        return Err(usage("too many arguments"));
    }

    let input_path = input.as_ref().map(Path::new);
    if let Some(path) = input_path {
        if !path.is_file() {
            return Err(format!("input ELF not found: {}", path.display()).into());
        }
    }

    let output_dir = PathBuf::from(output);
    fs::create_dir_all(&output_dir)?;

    let manifest_path = output_dir.join("manifest.json");
    fs::write(&manifest_path, exec_payloads::HELLO_MANIFEST)?;

    let payload_path = output_dir.join("payload.elf");
    if let Some(path) = input_path {
        fs::copy(path, &payload_path)?;
    } else {
        fs::write(&payload_path, HELLO_ELF)?;
    }

    Ok(())
}

fn usage(message: &str) -> Box<dyn std::error::Error> {
    let mut stderr = io::stderr();
    let _ = writeln!(stderr, "nxb-pack: {message}");
    let _ = writeln!(stderr, "usage: nxb-pack <input.elf> <output.nxb>");
    let _ = writeln!(stderr, "   or: nxb-pack --hello <output.nxb>");
    Box::<dyn std::error::Error>::from(message.to_string())
}
