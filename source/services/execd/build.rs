// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

fn main() {
    println!("cargo::rustc-check-cfg=cfg(nexus_env, values(\"os\",\"host\"))");

    // TASK-0080D R1: embed the app-host runtime ELF when the build
    // orchestration provides it (`scripts/build.sh` builds app-host first and
    // exports EXECD_APPHOST_ELF). Absent (plain cargo builds/checks), the
    // image table entry stays empty and IMG_APPHOST answers UNSUPPORTED —
    // never a silent fake payload.
    println!("cargo:rerun-if-env-changed=EXECD_APPHOST_ELF");
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let dest = out.join("apphost_payload.rs");
    match std::env::var("EXECD_APPHOST_ELF") {
        Ok(path) if !path.trim().is_empty() => {
            println!("cargo:rerun-if-changed={path}");
            std::fs::write(
                &dest,
                format!(
                    "/// app-host runtime ELF (built by scripts/build.sh).\n\
                     pub static APPHOST_ELF: &[u8] = include_bytes!({path:?});\n"
                ),
            )
            .expect("write apphost payload shim");
        }
        _ => {
            std::fs::write(
                &dest,
                "/// No app-host ELF provided at build time (plain cargo build).\n\
                 pub static APPHOST_ELF: &[u8] = &[];\n",
            )
            .expect("write apphost payload shim");
        }
    }

    // P0.2 recv-wake regression gate: embed the probe child ELF when the
    // build orchestration provides it (same pattern as EXECD_APPHOST_ELF).
    // Absent, the probe run is skipped with a visible marker — never faked.
    println!("cargo:rerun-if-env-changed=EXECD_RECVWAKE_ELF");
    let dest = out.join("recvwake_payload.rs");
    match std::env::var("EXECD_RECVWAKE_ELF") {
        Ok(path) if !path.trim().is_empty() => {
            println!("cargo:rerun-if-changed={path}");
            std::fs::write(
                &dest,
                format!(
                    "/// recv-wake probe ELF (built by scripts/build.sh).\n\
                     pub static RECVWAKE_ELF: &[u8] = include_bytes!({path:?});\n"
                ),
            )
            .expect("write recvwake payload shim");
        }
        _ => {
            std::fs::write(
                &dest,
                "/// No recv-wake probe ELF provided at build time (plain cargo build).\n\
                 pub static RECVWAKE_ELF: &[u8] = &[];\n",
            )
            .expect("write recvwake payload shim");
        }
    }
}
