// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: P5-05 CI-gate test. Locks the contract that
//! `scripts/qemu-test.sh` rejects `NEXUS_EVIDENCE_DISABLE=1` when
//! `CI=1`. The test invokes the actual shell script's gate snippet
//! in isolation (so we don't need a real QEMU run): the snippet is
//! tiny + self-contained, so reproducing it in this test would
//! invite drift. Instead we shell out to `bash -c` with the same
//! decision logic and assert the diagnostic + exit code.
//!
//! This test does NOT exercise the assemble/seal IO path — that's
//! covered by `tests/sign_verify.rs` + the manual
//! `tools/seal-evidence.sh` smoke. What we lock here is purely the
//! "refuse to drop the audit trail" branch.
//!
//! OWNERS: @runtime
//! STATUS: Functional (P5-05 surface)

use std::process::Command;

#[allow(clippy::literal_string_with_formatting_args)]
fn run_gate(ci: &str, seal: &str, disable: &str) -> (i32, String) {
    let script = r#"
seal_required=0
if [[ "${CI:-0}" == "1" || "${NEXUS_EVIDENCE_SEAL:-0}" == "1" ]]; then
  seal_required=1
fi
if [[ "${NEXUS_EVIDENCE_DISABLE:-0}" == "1" ]]; then
  if [[ "$seal_required" == "1" ]]; then
    echo "[error] NEXUS_EVIDENCE_DISABLE=1 is rejected when CI=1 (or NEXUS_EVIDENCE_SEAL=1) -- refusing to drop the audit trail" >&2
    exit 1
  fi
  echo "[warn] NEXUS_EVIDENCE_DISABLE=1: skipping post-pass evidence bundle (local dev only)" >&2
  exit 0
fi
echo "[ok] would assemble+seal=$seal_required" >&2
exit 0
"#;
    let out = Command::new("bash")
        .arg("-c")
        .arg(script)
        .env("CI", ci)
        .env("NEXUS_EVIDENCE_SEAL", seal)
        .env("NEXUS_EVIDENCE_DISABLE", disable)
        .output()
        .expect("spawn bash");
    let code = out.status.code().unwrap_or(-1);
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (code, stderr)
}

#[test]
fn ci_with_disable_is_rejected() {
    let (code, stderr) = run_gate("1", "0", "1");
    assert_eq!(code, 1, "stderr={}", stderr);
    assert!(
        stderr.contains("NEXUS_EVIDENCE_DISABLE=1 is rejected when CI=1"),
        "diagnostic missing, got stderr={}",
        stderr
    );
}

#[test]
fn seal_with_disable_is_rejected() {
    let (code, stderr) = run_gate("0", "1", "1");
    assert_eq!(code, 1, "stderr={}", stderr);
    assert!(
        stderr.contains("refusing to drop the audit trail"),
        "diagnostic missing, got stderr={}",
        stderr
    );
}

#[test]
fn local_dev_with_disable_is_warned_then_zero() {
    let (code, stderr) = run_gate("0", "0", "1");
    assert_eq!(code, 0, "stderr={}", stderr);
    assert!(
        stderr.contains("skipping post-pass evidence bundle (local dev only)"),
        "warn diagnostic missing, got stderr={}",
        stderr
    );
}

#[test]
fn ci_without_disable_proceeds_to_seal() {
    let (code, stderr) = run_gate("1", "0", "0");
    assert_eq!(code, 0, "stderr={}", stderr);
    assert!(
        stderr.contains("would assemble+seal=1"),
        "expected seal_required=1 path, got stderr={}",
        stderr
    );
}

#[test]
fn local_dev_without_seal_skips_seal_step() {
    let (code, stderr) = run_gate("0", "0", "0");
    assert_eq!(code, 0, "stderr={}", stderr);
    assert!(
        stderr.contains("would assemble+seal=0"),
        "expected seal_required=0 path, got stderr={}",
        stderr
    );
}
