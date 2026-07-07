// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `nx-dsl` CLI behavior proof (TASK-0078): `run` with
//! `--route/--locale/--profile` against the masterdetail app, generators
//! produce BUILDABLE scaffolds (init → build green), `i18n extract|compile`
//! round-trips the masterdetail catalogs.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 5 tests

use std::path::{Path, PathBuf};
use std::process::Command;

fn nx_dsl() -> Command {
    Command::new(env!("CARGO_BIN_EXE_nx-dsl"))
}

fn masterdetail() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../examples/dsl/masterdetail")
}

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nx-dsl-test-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

#[test]
fn run_masterdetail_desktop_exits_zero_with_scene_texts() {
    let output = nx_dsl()
        .args(["run"])
        .arg(masterdetail())
        .args(["--profile", "desktop"])
        .output()
        .expect("spawn");
    assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("mounted: hash"), "stdout: {stdout}");
    // Pseudo-locale: untranslated keys render as themselves.
    assert!(stdout.contains("text: library.title"), "stdout: {stdout}");
}

#[test]
fn run_locale_switch_changes_scene_texts() {
    let de = nx_dsl()
        .args(["run"])
        .arg(masterdetail())
        .args(["--locale", "de"])
        .output()
        .expect("spawn");
    assert!(de.status.success(), "stderr: {}", String::from_utf8_lossy(&de.stderr));
    let de_out = String::from_utf8_lossy(&de.stdout);
    assert!(de_out.contains("text: Bibliothek"), "de stdout: {de_out}");

    let en = nx_dsl()
        .args(["run"])
        .arg(masterdetail())
        .args(["--locale", "en"])
        .output()
        .expect("spawn");
    let en_out = String::from_utf8_lossy(&en.stdout);
    assert!(en_out.contains("text: Library"), "en stdout: {en_out}");
    assert_ne!(de_out, en_out, "locales must differ");
}

#[test]
fn run_profile_matrix_is_stable_and_distinct_where_overridden() {
    // masterdetail has a phone override for DetailPage; the list page is
    // shared — both profiles must run green and deterministically.
    let mut outputs = Vec::new();
    for profile in ["desktop", "phone"] {
        let output = nx_dsl()
            .args(["run"])
            .arg(masterdetail())
            .args(["--profile", profile, "--route", "/detail"])
            .output()
            .expect("spawn");
        assert!(
            output.status.success(),
            "{profile} stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let repeat = nx_dsl()
            .args(["run"])
            .arg(masterdetail())
            .args(["--profile", profile, "--route", "/detail"])
            .output()
            .expect("spawn");
        assert_eq!(output.stdout, repeat.stdout, "{profile} run must be deterministic");
        outputs.push(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    assert_ne!(outputs[0], outputs[1], "the phone override renders differently");
}

#[test]
fn init_scaffold_builds_green() {
    let dir = temp_dir("init");
    let app = dir.join("app");
    let status = nx_dsl().arg("init").arg(&app).status().expect("spawn");
    assert!(status.success());

    let build = nx_dsl()
        .arg("build")
        .args(["-o"])
        .arg(dir.join("out"))
        .arg(&app)
        .output()
        .expect("spawn");
    assert!(
        build.status.success(),
        "scaffold must build: {}",
        String::from_utf8_lossy(&build.stderr)
    );

    // add page/component/store scaffolds stay buildable too.
    for args in [["page", "Extra"], ["component", "Chip"], ["store", "Extra"]] {
        let status = nx_dsl()
            .arg("add")
            .args(args)
            .arg(&app)
            .status()
            .expect("spawn");
        assert!(status.success(), "add {args:?}");
    }
    let build = nx_dsl()
        .arg("build")
        .args(["-o"])
        .arg(dir.join("out2"))
        .arg(&app)
        .output()
        .expect("spawn");
    assert!(
        build.status.success(),
        "extended scaffold must build: {}",
        String::from_utf8_lossy(&build.stderr)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn i18n_extract_and_compile_round_trip() {
    let dir = temp_dir("i18n");
    let extracted = dir.join("en.json");
    let status = nx_dsl()
        .args(["i18n", "extract"])
        .arg(masterdetail())
        .arg("-o")
        .arg(&extracted)
        .status()
        .expect("spawn");
    assert!(status.success());
    let text = std::fs::read_to_string(&extracted).expect("extracted file");
    assert!(text.contains("\"library.title\""), "extracted: {text}");
    assert!(text.contains("\"common.loading\""), "extracted: {text}");

    // Compile the shipped en catalog; the binary loads via the runtime.
    let compiled = dir.join("en.nxc");
    let status = nx_dsl()
        .args(["i18n", "compile"])
        .arg(masterdetail().join("i18n/en.json"))
        .arg("-o")
        .arg(&compiled)
        .status()
        .expect("spawn");
    assert!(status.success());
    let bytes = std::fs::read(&compiled).expect("compiled catalog");
    let keys = ["library.title", "common.loading"];
    let catalog =
        nexus_dsl_runtime::Catalog::from_binary(&keys, &bytes).expect("binary loads");
    let names: Vec<String> = keys.iter().map(|k| String::from(*k)).collect();
    let catalogs = [&catalog];
    let chain = nexus_dsl_runtime::LocaleChain::new(&catalogs, &names);
    let formatted = nexus_dsl_runtime::LocaleSource::format(&chain, 0, &[]);
    assert_eq!(formatted, "Library");
    let _ = std::fs::remove_dir_all(&dir);
}

/// TASK-0081 C1: `add native` scaffolds the companion (surface.toml +
/// Cargo.toml + Surface-trait skeleton) exactly once; the scaffolded
/// surface makes `svc.<app>.ping` checkable via the project build.
#[test]
fn add_native_scaffolds_companion_and_enables_svc_surface() {
    let root = std::env::temp_dir().join(format!("nx-add-native-{}", std::process::id()));
    let app = root.join("demoapp");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(app.join("ui/pages")).expect("mkdir");
    std::fs::write(app.join("manifest.toml"), "name = \"demoapp\"\n").expect("manifest");
    std::fs::write(
        app.join("ui/pages/Main.nx"),
        "Store S { v: Str = \"\", }\nEvent E { Go, Got(Str), Bad(Int), }\nreduce E {\n    Go => state.v = state.v,\n    Got(t) => state.v = t,\n    Bad(c) => state.v = state.v,\n}\n@effect on Go {\n    match svc.demoapp.ping(state.v, timeoutMs: 250) {\n        Ok(t) => dispatch(Got(t)),\n        Err(e) => dispatch(Bad(e)),\n    }\n}\nPage Main { Stack { Text($state.v) } }\n",
    )
    .expect("page");

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_nx-dsl"))
        .args(["add", "native", app.to_str().expect("utf8 path")])
        .output()
        .expect("spawn");
    assert!(out.status.success(), "add native failed: {}", String::from_utf8_lossy(&out.stderr));
    for rel in ["native/surface.toml", "native/Cargo.toml", "native/src/lib.rs"] {
        assert!(app.join(rel).is_file(), "{rel} missing");
    }
    let skeleton = std::fs::read_to_string(app.join("native/src/lib.rs")).expect("lib");
    assert!(skeleton.contains("pub trait Surface"), "trait skeleton: {skeleton}");
    assert!(skeleton.contains("svc.demoapp.ping"), "app-specific doc: {skeleton}");

    // The scaffolded surface makes the project compile (checker knows the svc).
    nexus_dsl_core::compile_project_dir(&app).expect("project compiles with scaffolded surface");

    // Re-run refuses (never overwrites developer code).
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_nx-dsl"))
        .args(["add", "native", app.to_str().expect("utf8 path")])
        .output()
        .expect("spawn");
    assert!(!out.status.success(), "re-run must refuse");
    let _ = std::fs::remove_dir_all(&root);
}
