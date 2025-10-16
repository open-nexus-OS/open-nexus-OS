use std::path::PathBuf;

fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
    if target == "riscv64imac-unknown-none-elf" {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let script = PathBuf::from(manifest_dir).join("kernel.ld");
        println!("cargo:rerun-if-changed={}", script.display());
        // Linker script is already provided via .cargo/config.toml; avoid duplicating -T
    }
}
