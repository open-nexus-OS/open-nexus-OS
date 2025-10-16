use std::{env, fs, path::PathBuf, process};

fn main() {
    if let Err(err) = run() {
        eprintln!("nxb-pack: {err}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let input = args.next().ok_or_else(|| "missing input ELF path".to_string())?;
    let output = args.next().ok_or_else(|| "missing output directory".to_string())?;
    if args.next().is_some() {
        return Err("unexpected extra arguments".to_string());
    }

    let input_path = PathBuf::from(&input);
    if !input_path.is_file() {
        return Err(format!("input ELF not found: {input}"));
    }

    let output_path = PathBuf::from(&output);
    fs::create_dir_all(&output_path).map_err(|e| format!("create output dir: {e}"))?;

    let manifest_path = output_path.join("manifest.json");
    let payload_path = output_path.join("payload.elf");

    let manifest = r#"{ "name":"demo.hello", "version":"0.0.1", "required_caps":[], "publisher":"dev", "sig":"" }\n"#;
    fs::write(&manifest_path, manifest)
        .map_err(|e| format!("write manifest {manifest_path:?}: {e}"))?;

    fs::copy(&input_path, &payload_path)
        .map_err(|e| format!("copy payload to {payload_path:?}: {e}"))?;

    Ok(())
}
