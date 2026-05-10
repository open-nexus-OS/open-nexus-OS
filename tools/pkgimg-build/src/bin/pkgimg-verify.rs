//! CONTEXT: Host verifier CLI for pkgimg v2 parse/validation contract.
//! OWNERS: @runtime @storage
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Build sanity covered by `cargo test -p pkgimg-build`.

use std::fs;
use std::path::PathBuf;

use storage::pkgimg::{parse_pkgimg, PkgImgCaps};

fn usage() -> ! {
    eprintln!("usage: pkgimg-verify <image.pkgimg>");
    std::process::exit(2);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        usage();
    }
    let input = PathBuf::from(&args[1]);
    let bytes = fs::read(&input).unwrap_or_else(|err| {
        eprintln!("read {}: {err}", input.display());
        std::process::exit(1);
    });
    let parsed = parse_pkgimg(&bytes, PkgImgCaps::default()).unwrap_or_else(|err| {
        eprintln!("verify {}: {err}", input.display());
        std::process::exit(1);
    });
    println!(
        "pkgimg-verify: ok entries={} bytes={}",
        parsed.entries().len(),
        bytes.len()
    );
}
