// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: TASK-0023B Cut P4-09 — `verify-uart` integration coverage.
//! Synthesizes minimal UART transcripts and feeds them through the CLI to
//! lock the deny-by-default semantics: forbidden markers fail, unexpected
//! markers fail, expected markers pass. Uses a self-contained TOML
//! fixture (no dependency on the on-disk manifest) so a manifest edit
//! can't accidentally make a regression test pass.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal (test-only)
//! TEST_COVERAGE: cargo test -p nexus-proof-manifest --test cli_verify_uart
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_nexus-proof-manifest");

const FIXTURE_TOML: &str = r#"[meta]
schema_version = "1"
default_profile = "full"

[phase.bringup]
order = 1
[phase.net]
order = 2
[phase.end]
order = 3

[profile.full]
runner = "scripts/qemu-test.sh"
env = {}

[profile."quic-required"]
runner = "scripts/qemu-test.sh"
extends = "full"
env = { REQUIRE_DSOFTBUS = "1" }

[marker."SELFTEST: bringup ok"]
phase = "bringup"

[marker."SELFTEST: end"]
phase = "end"

[marker."SELFTEST: quic session ok"]
phase = "net"
emit_when = { profile = "quic-required" }

[marker."dsoftbusd: transport selected tcp"]
phase = "net"
forbidden_when = { profile = "quic-required" }
"#;

struct Fixture {
    _dir: tempdir::Dir,
    manifest: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempdir::Dir::new();
        let manifest = dir.path.join("proof-manifest.toml");
        std::fs::write(&manifest, FIXTURE_TOML).expect("write fixture");
        Self {
            _dir: dir,
            manifest,
        }
    }

    fn write_uart(&self, name: &str, body: &str) -> PathBuf {
        let p = self._dir.path.join(name);
        let mut f = std::fs::File::create(&p).expect("create uart file");
        f.write_all(body.as_bytes()).expect("write uart body");
        p
    }
}

mod tempdir {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    pub struct Dir {
        pub path: PathBuf,
    }
    impl Dir {
        pub fn new() -> Self {
            let mut p = std::env::temp_dir();
            let nonce = format!(
                "nxs-verify-uart-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::SeqCst)
            );
            p.push(nonce);
            std::fs::create_dir_all(&p).expect("mkdir tempdir");
            Self { path: p }
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

fn run(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(BIN)
        .args(args)
        .output()
        .expect("invoke nexus-proof-manifest CLI");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn accept_full_profile_with_clean_uart() {
    let f = Fixture::new();
    let uart = f.write_uart(
        "ok.log",
        "[QEMU] SELFTEST: bringup ok\n\
         some unrelated noise\n\
         SELFTEST: end\n",
    );
    let (code, stdout, stderr) = run(&[
        "verify-uart",
        "--profile=full",
        &format!("--manifest={}", f.manifest.display()),
        &format!("--uart={}", uart.display()),
    ]);
    assert_eq!(code, 0, "expected ok; stdout=`{stdout}` stderr=`{stderr}`");
    assert!(stdout.contains("[verify-uart] ok"), "stdout=`{stdout}`");
}

#[test]
fn reject_unexpected_marker_under_full_profile() {
    // `SELFTEST: quic session ok` is gated to profile=quic-required;
    // its appearance under profile=full is "unexpected" → exit 1.
    let f = Fixture::new();
    let uart = f.write_uart(
        "unexpected.log",
        "SELFTEST: bringup ok\n\
         SELFTEST: quic session ok\n\
         SELFTEST: end\n",
    );
    let (code, _stdout, stderr) = run(&[
        "verify-uart",
        "--profile=full",
        &format!("--manifest={}", f.manifest.display()),
        &format!("--uart={}", uart.display()),
    ]);
    assert_ne!(code, 0, "expected failure; stderr=`{stderr}`");
    assert!(
        stderr.contains("unexpected") && stderr.contains("quic session ok"),
        "stderr=`{stderr}`"
    );
}

#[test]
fn reject_forbidden_marker_under_quic_required() {
    let f = Fixture::new();
    let uart = f.write_uart(
        "forbidden.log",
        "SELFTEST: bringup ok\n\
         dsoftbusd: transport selected tcp\n\
         SELFTEST: quic session ok\n\
         SELFTEST: end\n",
    );
    let (code, _stdout, stderr) = run(&[
        "verify-uart",
        "--profile=quic-required",
        &format!("--manifest={}", f.manifest.display()),
        &format!("--uart={}", uart.display()),
    ]);
    assert_ne!(code, 0, "expected failure; stderr=`{stderr}`");
    assert!(
        stderr.contains("forbidden") && stderr.contains("transport selected tcp"),
        "stderr=`{stderr}`"
    );
}

#[test]
fn json_format_emits_structured_violations() {
    let f = Fixture::new();
    let uart = f.write_uart(
        "json.log",
        "SELFTEST: bringup ok\n\
         dsoftbusd: transport selected tcp\n\
         SELFTEST: end\n",
    );
    let (code, stdout, _stderr) = run(&[
        "verify-uart",
        "--profile=quic-required",
        "--format=json",
        &format!("--manifest={}", f.manifest.display()),
        &format!("--uart={}", uart.display()),
    ]);
    assert_ne!(code, 0);
    let trimmed = stdout.trim();
    assert!(
        trimmed.starts_with('{') && trimmed.ends_with('}'),
        "stdout=`{trimmed}`"
    );
    assert!(trimmed.contains("\"forbidden\":["), "stdout=`{trimmed}`");
    assert!(
        trimmed.contains("transport selected tcp"),
        "stdout=`{trimmed}`"
    );
}

#[test]
fn missing_uart_file_exits_two() {
    let f = Fixture::new();
    let (code, _stdout, stderr) = run(&[
        "verify-uart",
        "--profile=full",
        &format!("--manifest={}", f.manifest.display()),
        "--uart=/does/not/exist/uart.log",
    ]);
    assert_eq!(code, 2, "expected io error exit code; stderr=`{stderr}`");
}

#[test]
fn missing_uart_arg_exits_one() {
    let f = Fixture::new();
    let (code, _stdout, stderr) = run(&[
        "verify-uart",
        "--profile=full",
        &format!("--manifest={}", f.manifest.display()),
    ]);
    assert_eq!(code, 1, "expected usage error exit code; stderr=`{stderr}`");
    assert!(stderr.contains("--uart="), "stderr=`{stderr}`");
}
