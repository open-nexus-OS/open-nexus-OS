//! CONTEXT: CLI wrapper for deterministic SBOM generation
//! OWNERS: @runtime @security
//! STATUS: Functional
//! API_STABILITY: Unstable (tooling CLI)
//! TEST_COVERAGE: No direct unit tests (covered through `tools/sbom/src/lib.rs` and integration via `nxb-pack`)

use std::fs;
use std::path::PathBuf;

use sbom::{generate_bundle_sbom_json, source_date_epoch_from_env, BundleSbomInput};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let bundle_name = args.next().ok_or_else(|| usage("missing <bundle_name>"))?;
    let bundle_version = args.next().ok_or_else(|| usage("missing <bundle_version>"))?;
    let publisher_hex = args.next().ok_or_else(|| usage("missing <publisher_hex>"))?;
    let payload_sha256 = args.next().ok_or_else(|| usage("missing <payload_sha256>"))?;
    let payload_size = args
        .next()
        .ok_or_else(|| usage("missing <payload_size>"))?
        .parse::<u64>()
        .map_err(|_| usage("invalid <payload_size>"))?;
    let manifest_sha256 = args.next().ok_or_else(|| usage("missing <manifest_sha256>"))?;
    let output = args.next().ok_or_else(|| usage("missing <output_path>"))?;

    if args.next().is_some() {
        return Err(usage("too many arguments"));
    }

    let input = BundleSbomInput {
        bundle_name,
        bundle_version,
        publisher_hex,
        payload_sha256,
        payload_size,
        manifest_sha256,
        source_date_epoch: source_date_epoch_from_env()?,
        components: Vec::new(),
    };
    let bytes = generate_bundle_sbom_json(&input)?;
    let output_path = PathBuf::from(output);
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(output_path, bytes)?;
    Ok(())
}

fn usage(message: &str) -> Box<dyn std::error::Error> {
    eprintln!("sbom: {message}");
    eprintln!(
        "usage: sbom <bundle_name> <bundle_version> <publisher_hex> <payload_sha256> <payload_size> <manifest_sha256> <output_path>"
    );
    Box::<dyn std::error::Error>::from(message.to_string())
}
