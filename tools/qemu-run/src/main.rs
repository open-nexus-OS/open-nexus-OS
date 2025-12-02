//! CONTEXT: QEMU runner tool
//! INTENT: Launch QEMU with OS image for testing
//! IDL (target): run()
//! DEPS: scripts/qemu-run.sh (shell script)
//! READINESS: Command-line tool; no service dependencies
//! TESTS: QEMU launches successfully
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("scripts")
        .join("qemu-run.sh");
    let status = match Command::new("sh").arg(script).status() {
        Ok(s) => s,
        Err(e) => panic!("failed to run qemu: {e}"),
    };
    if !status.success() {
        eprintln!("qemu exited with status: {status}");
        std::process::exit(1);
    }
}
