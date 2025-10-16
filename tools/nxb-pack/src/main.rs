use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let input = args.next().ok_or_else(|| usage("missing input ELF path"))?;
    let output = args.next().ok_or_else(|| usage("missing output directory"))?;
    if args.next().is_some() {
        return Err(usage("too many arguments"));
    }

    let input_path = Path::new(&input);
    if !input_path.is_file() {
        return Err(format!("input ELF not found: {input}").into());
    }

    let output_dir = PathBuf::from(output);
    fs::create_dir_all(&output_dir)?;

    let manifest_path = output_dir.join("manifest.json");
    let manifest = r#"{"name":"demo.hello","version":"0.0.1","required_caps":[],"publisher":"dev","sig":""}"#;
    fs::write(&manifest_path, format!("{}\n", manifest))?;

    let payload_path = output_dir.join("payload.elf");
    fs::copy(input_path, &payload_path)?;

    Ok(())
}

fn usage(message: &str) -> Box<dyn std::error::Error> {
    let mut stderr = io::stderr();
    let _ = writeln!(stderr, "nxb-pack: {message}");
    let _ = writeln!(stderr, "usage: nxb-pack <input.elf> <output.nxb>");
    Box::<dyn std::error::Error>::from(message.to_string())
}
