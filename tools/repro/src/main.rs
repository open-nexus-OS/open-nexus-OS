//! CONTEXT: CLI wrapper for repro metadata capture/verify workflows
//! OWNERS: @runtime @security
//! STATUS: Functional
//! API_STABILITY: Unstable (tooling CLI)
//! TEST_COVERAGE: No direct unit tests (covered through `tools/repro/src/lib.rs` and bundle integration tests)

use std::fs;
use std::path::PathBuf;

use repro::{capture_bundle_repro_json, verify_repro_json, ReproVerifyInput};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let subcommand = args.next().ok_or_else(|| usage("missing subcommand"))?;
    match subcommand.as_str() {
        "capture" => {
            let manifest = args.next().ok_or_else(|| usage("missing <manifest.nxb path>"))?;
            let payload = args.next().ok_or_else(|| usage("missing <payload.elf path>"))?;
            let sbom = args.next().ok_or_else(|| usage("missing <meta/sbom.json path>"))?;
            let output = args.next().ok_or_else(|| usage("missing <output path>"))?;
            if args.next().is_some() {
                return Err(usage("too many arguments for `capture`"));
            }
            let manifest_bytes = fs::read(manifest)?;
            let payload_bytes = fs::read(payload)?;
            let sbom_bytes = fs::read(sbom)?;
            let repro_bytes =
                capture_bundle_repro_json(&manifest_bytes, &payload_bytes, &sbom_bytes)?;
            let output_path = PathBuf::from(output);
            if let Some(parent) = output_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(output_path, repro_bytes)?;
        }
        "verify" => {
            let repro_path = args.next().ok_or_else(|| usage("missing <repro.env.json path>"))?;
            let payload_sha = args.next().ok_or_else(|| usage("missing <payload sha256>"))?;
            let manifest_sha = args.next().ok_or_else(|| usage("missing <manifest sha256>"))?;
            let sbom_sha = args.next().ok_or_else(|| usage("missing <sbom sha256>"))?;
            if args.next().is_some() {
                return Err(usage("too many arguments for `verify`"));
            }
            let bytes = fs::read(repro_path)?;
            let expected = ReproVerifyInput {
                payload_sha256: payload_sha,
                manifest_sha256: manifest_sha,
                sbom_sha256: sbom_sha,
            };
            verify_repro_json(&bytes, &expected)?;
        }
        _ => return Err(usage("unknown subcommand")),
    }
    Ok(())
}

fn usage(message: &str) -> Box<dyn std::error::Error> {
    eprintln!("repro: {message}");
    eprintln!("usage:");
    eprintln!(
        "  repro capture <manifest.nxb> <payload.elf> <meta/sbom.json> <meta/repro.env.json>"
    );
    eprintln!(
        "  repro verify <meta/repro.env.json> <payload_sha256> <manifest_sha256> <sbom_sha256>"
    );
    Box::<dyn std::error::Error>::from(message.to_string())
}
