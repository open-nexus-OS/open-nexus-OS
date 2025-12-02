use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let linker_script = manifest_dir.join("kernel.ld");
    println!("cargo:rerun-if-changed={}", linker_script.display());
    // Use canonicalize to ensure only a single absolute path reaches the linker
    let abs_script = linker_script.canonicalize().expect("kernel.ld must exist");
    println!("cargo:rustc-link-arg=-T{}", abs_script.display());
}
