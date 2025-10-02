use std::path::PathBuf;
use std::process::Command;

fn main() {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("scripts")
        .join("qemu-run.sh");
    let status = Command::new("sh")
        .arg(script)
        .status()
        .expect("failed to run qemu");
    if !status.success() {
        eprintln!("qemu exited with status: {status}");
        std::process::exit(1);
    }
}
