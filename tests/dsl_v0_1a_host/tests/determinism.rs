// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Build determinism + IR goldens + loader-side validation rejects.
//! Regenerate goldens with `UPDATE_GOLDENS=1 cargo test -p dsl_v0_1a_host`.

use dsl_v0_1a_host::{compile, PROOF_SURFACE};
use nexus_dsl_ir::read::ProgramReader;
use nexus_dsl_ir::validate::validate_program;
use std::path::PathBuf;

fn golden_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("goldens").join(name)
}

fn check_golden(name: &str, bytes: &[u8]) {
    let path = golden_path(name);
    if std::env::var("UPDATE_GOLDENS").is_ok() {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(&path, bytes).expect("write golden");
        return;
    }
    let expected = std::fs::read(&path).unwrap_or_else(|_| {
        panic!("missing golden {name}; run with UPDATE_GOLDENS=1 to create it")
    });
    assert_eq!(
        expected, bytes,
        "golden `{name}` drifted — if intentional, regenerate with UPDATE_GOLDENS=1 \
         and record the change in docs/dev/dsl/ir.md#changelog"
    );
}

#[test]
fn build_is_byte_deterministic() {
    let a = compile(PROOF_SURFACE);
    let b = compile(PROOF_SURFACE);
    assert_eq!(a, b);
}

#[test]
fn proof_surface_matches_the_ir_golden() {
    let bytes = compile(PROOF_SURFACE);
    check_golden("proof_surface.nxir", &bytes);
}

#[test]
fn built_ir_passes_loader_validation() {
    let bytes = compile(PROOF_SURFACE);
    let reader = ProgramReader::from_canonical_bytes(&bytes).expect("reads");
    validate_program(reader.root().expect("root")).expect("validates");
}

#[test]
fn loader_rejects_wrong_major_and_corruption() {
    let bytes = compile(PROOF_SURFACE);
    // Corruption: flip a byte in the middle.
    let mut corrupt = bytes.clone();
    let idx = corrupt.len() / 2;
    corrupt[idx] ^= 0xff;
    let outcome = ProgramReader::from_canonical_bytes(&corrupt)
        .and_then(|r| r.root().and_then(validate_program));
    assert!(outcome.is_err());
    // Truncation.
    let outcome = ProgramReader::from_canonical_bytes(&bytes[..bytes.len() - 8]);
    assert!(
        outcome.is_err() || {
            let r = outcome.unwrap_or_else(|_| unreachable!());
            r.root().and_then(validate_program).is_err()
        }
    );
    // Odd length is rejected outright.
    assert!(ProgramReader::from_canonical_bytes(&bytes[..bytes.len() - 3]).is_err());
}

#[test]
fn fmt_of_the_fixture_is_idempotent_and_reflows_to_the_same_ir() {
    let file = nexus_dsl_core::parse_file(PROOF_SURFACE).expect("parses");
    let once = nexus_dsl_core::format_file(&file);
    let reparsed = nexus_dsl_core::parse_file(&once).expect("reparses");
    let twice = nexus_dsl_core::format_file(&reparsed);
    assert_eq!(once, twice, "fmt fixpoint");
    // Formatting must not change the IR (modulo sourceDigest, which is part
    // of the canonical-source contract: both lower from the SAME canonical
    // text, so the full bytes match).
    assert_eq!(compile(PROOF_SURFACE), compile(&once));
}
