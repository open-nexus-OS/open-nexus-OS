// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: P5-02 acceptance tests for `Bundle::assemble`,
//! `extract_trace`, `gather_config`, and the unsigned-bundle
//! tar.gz round-trip. Five tests; one per stop-condition bullet
//! in the Phase-5 plan §"Cut P5-02".
//!
//! Tests use a small synthetic v2 manifest tree (4 markers in 2
//! phases) instead of the real on-disk manifest to keep fixtures
//! self-contained and avoid coupling to manifest evolution.
//!
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 5 tests

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use nexus_evidence::{
    canonical_hash, read_unsigned, AssembleOpts, Bundle, EvidenceError, GatherOpts,
};

// ---------------------------------------------------------------------------
// Test fixtures
// ---------------------------------------------------------------------------

/// Holds a tempdir + the manifest root path. Dropped on test exit.
struct Fixture {
    _root: tempdir::TempDir,
    manifest_path: PathBuf,
    uart_path: PathBuf,
    out_path: PathBuf,
}

impl Fixture {
    fn new(name: &str, uart_text: &str) -> Self {
        let root = tempdir::TempDir::with_label(name);
        let pm_dir = root.path.join("proof-manifest");
        fs::create_dir_all(pm_dir.join("markers")).unwrap();
        fs::create_dir_all(pm_dir.join("profiles")).unwrap();

        // [meta] + [include]
        fs::write(
            pm_dir.join("manifest.toml"),
            r#"[meta]
schema_version = "2"
default_profile = "full"

[include]
phases = "phases.toml"
markers = "markers/*.toml"
profiles = "profiles/*.toml"
"#,
        )
        .unwrap();

        // 2 phases
        fs::write(
            pm_dir.join("phases.toml"),
            r#"[phase.bringup]
order = 1

[phase.vfs]
order = 2
"#,
        )
        .unwrap();

        // 4 markers (2 per phase)
        fs::write(
            pm_dir.join("markers").join("bringup.toml"),
            r#"[marker."SELFTEST: bringup alpha ok"]
phase = "bringup"

[marker."SELFTEST: bringup beta ok"]
phase = "bringup"
"#,
        )
        .unwrap();
        fs::write(
            pm_dir.join("markers").join("vfs.toml"),
            r#"[marker."SELFTEST: vfs gamma ok"]
phase = "vfs"

[marker."SELFTEST: vfs delta ok"]
phase = "vfs"
"#,
        )
        .unwrap();

        // 1 harness profile
        fs::write(
            pm_dir.join("profiles").join("harness.toml"),
            r#"[profile.full]
runner = "scripts/qemu-test.sh"
"#,
        )
        .unwrap();

        let manifest_path = pm_dir.join("manifest.toml");
        let uart_path = root.path.join("uart.log");
        fs::write(&uart_path, uart_text).unwrap();
        let out_path = root.path.join("bundle.tar.gz");

        Self {
            _root: root,
            manifest_path,
            uart_path,
            out_path,
        }
    }

    fn opts(&self, profile: &str, wall_clock: &str) -> AssembleOpts {
        AssembleOpts {
            uart_path: self.uart_path.clone(),
            manifest_path: self.manifest_path.clone(),
            gather_opts: GatherOpts {
                profile: profile.into(),
                env: env_fixture(),
                kernel_cmdline: "console=ttyS0".into(),
                qemu_args: vec!["-machine".into(), "virt".into(), "-smp".into(), "1".into()],
                host_info: "Linux test 6.12.0".into(),
                build_sha: "abc1234".into(),
                rustc_version: "rustc 1.90.0".into(),
                qemu_version: "QEMU 9.0.0".into(),
                wall_clock_utc: wall_clock.into(),
            },
        }
    }
}

fn env_fixture() -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    env.insert("PROFILE".into(), "full".into());
    env.insert("REQUIRE_DSOFTBUS".into(), "0".into());
    env
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Test 1 (round-trip): assemble → write_unsigned → read_unsigned;
/// resulting bundle's trace + config + meta + manifest match.
#[test]
fn assemble_write_read_round_trip() {
    let uart = "[ts=10ms] SELFTEST: bringup alpha ok\n\
                [ts=20ms] SELFTEST: bringup beta ok\n\
                SELFTEST: vfs gamma ok\n\
                SELFTEST: vfs delta ok\n";
    let fx = Fixture::new("p5_02_round_trip", uart);
    let bundle = Bundle::assemble(fx.opts("full", "2026-04-17T10:00:00Z")).unwrap();
    bundle.write_unsigned(&fx.out_path).unwrap();

    let reread = read_unsigned(&fx.out_path).unwrap();
    assert_eq!(reread.meta, bundle.meta);
    assert_eq!(reread.manifest, bundle.manifest);
    assert_eq!(reread.uart, bundle.uart);
    assert_eq!(reread.trace, bundle.trace);
    assert_eq!(reread.config, bundle.config);
    assert!(reread.signature.is_none());
    assert_eq!(reread.trace.entries.len(), 4);
}

/// Test 2 (reproducibility): two assemblies of the same UART produce
/// identical canonical hashes (modulo `wall_clock_utc`, which is
/// excluded from the hash). Outer-tar bytes also match because the
/// whole bundle round-trip is deterministic.
#[test]
fn two_assemblies_canonical_hash_identical_modulo_wall_clock() {
    let uart = "SELFTEST: bringup alpha ok\nSELFTEST: vfs gamma ok\n";
    let fx = Fixture::new("p5_02_repro", uart);

    let b1 = Bundle::assemble(fx.opts("full", "2026-04-17T10:00:00Z")).unwrap();
    let b2 = Bundle::assemble(fx.opts("full", "2026-04-17T11:30:00Z")).unwrap();

    assert_ne!(b1.config.wall_clock_utc, b2.config.wall_clock_utc);
    assert_eq!(
        canonical_hash(&b1),
        canonical_hash(&b2),
        "canonical hash must be stable across reseals"
    );
}

/// Test 3 (phase fidelity): trace extractor honors the manifest's
/// declared phase. Even if the UART emits markers "out of order"
/// (vfs marker before bringup marker), the trace entry's `phase`
/// field MUST come from the manifest, not from UART position.
#[test]
fn trace_extractor_honors_declared_phase() {
    let uart = "SELFTEST: vfs gamma ok\nSELFTEST: bringup alpha ok\n";
    let fx = Fixture::new("p5_02_phase_fidelity", uart);

    let bundle = Bundle::assemble(fx.opts("full", "2026-04-17T10:00:00Z")).unwrap();
    let entries = &bundle.trace.entries;
    assert_eq!(entries.len(), 2);

    // First UART line was a vfs marker; phase MUST be "vfs"
    // regardless of position.
    assert_eq!(entries[0].marker, "SELFTEST: vfs gamma ok");
    assert_eq!(entries[0].phase, "vfs");
    assert_eq!(entries[1].marker, "SELFTEST: bringup alpha ok");
    assert_eq!(entries[1].phase, "bringup");
}

/// Test 4 (reject malformed `[ts=…ms]`): a UART line with a malformed
/// timestamp prefix surfaces as `EvidenceError::MalformedTrace`.
#[test]
fn reject_malformed_ts_prefix() {
    let uart = "[ts=fooms] SELFTEST: bringup alpha ok\n";
    let fx = Fixture::new("p5_02_bad_ts", uart);

    let err = Bundle::assemble(fx.opts("full", "2026-04-17T10:00:00Z")).unwrap_err();
    match err {
        EvidenceError::MalformedTrace { detail } => {
            assert!(
                detail.contains("malformed_ts_value") || detail.contains("malformed_ts_prefix"),
                "expected malformed_ts diagnostic, got: {}",
                detail
            );
        }
        other => panic!("expected MalformedTrace, got {:?}", other),
    }
}

/// Test 5 (deny-by-default unknown marker): if the UART carries a
/// `SELFTEST:` line whose literal is not declared in the manifest,
/// assembly fails with `MalformedTrace { unknown_marker }`. Mirrors
/// the P4-09 `verify-uart` deny-by-default posture for the bundle
/// pipeline.
#[test]
fn reject_unknown_marker_in_uart() {
    let uart = "SELFTEST: bringup alpha ok\nSELFTEST: orphan marker ok\n";
    let fx = Fixture::new("p5_02_unknown_marker", uart);

    let err = Bundle::assemble(fx.opts("full", "2026-04-17T10:00:00Z")).unwrap_err();
    match err {
        EvidenceError::MalformedTrace { detail } => {
            assert!(
                detail.contains("unknown_marker"),
                "expected unknown_marker diagnostic, got: {}",
                detail
            );
            assert!(
                detail.contains("orphan marker"),
                "expected the offending literal in diagnostic, got: {}",
                detail
            );
        }
        other => panic!("expected MalformedTrace, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// Minimal in-test tempdir (no extra crate; keeps `nexus-evidence`
// dev-deps lean).
//
// Provides a unique workspace under `target/tmp/<label>-<pid>/` and
// removes it on drop. Failures during teardown are swallowed (test
// already passed; cleanup is best-effort).
// ---------------------------------------------------------------------------
mod tempdir {
    use std::fs;
    use std::path::PathBuf;
    use std::process;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static SEQ: AtomicUsize = AtomicUsize::new(0);

    pub struct TempDir {
        pub path: PathBuf,
    }

    impl TempDir {
        pub fn with_label(label: &str) -> Self {
            let n = SEQ.fetch_add(1, Ordering::SeqCst);
            let path = std::env::temp_dir().join(format!(
                "nexus-evidence-test-{}-{}-{}",
                label,
                process::id(),
                n
            ));
            // Best-effort cleanup if a stale dir survived an earlier crash.
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self { path }
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[allow(dead_code)]
    fn _ensure_unused_imports_are_silent() {
        let _: super::PathBuf = PathBuf::new();
    }
}

#[allow(dead_code)]
fn _silence_unused(_p: &Path) {}
