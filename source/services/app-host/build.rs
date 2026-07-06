// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Compiles the R2 demo payload (`examples/dsl/counter/counter.nx`
//! → canonical `.nxir`) into OUT_DIR — the same seam windowd's demo mount
//! uses. R2's bundle-fetch step replaces this embed with GET_PAYLOAD bytes;
//! the mount/render code is payload-source-agnostic.
//! OWNERS: @ui @runtime
//! STATUS: Experimental (TASK-0080D R2)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: compile failure = build failure (fail-closed)

use std::path::Path;

const DSL_APP_NX: &str = "../../../examples/dsl/counter/counter.nx";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo::rustc-check-cfg=cfg(nexus_env, values(\"os\",\"host\"))");
    println!("cargo:rerun-if-changed={DSL_APP_NX}");
    let out_dir = std::env::var_os("OUT_DIR").ok_or("missing OUT_DIR")?;
    let source = std::fs::read_to_string(DSL_APP_NX)?;
    let file = nexus_dsl_core::parse_file(&source)
        .map_err(|d| std::io::Error::other(format!("app payload parse: {} {}", d.code, d.message)))?;
    let (model, diags) = nexus_dsl_core::check_file(&file);
    if nexus_dsl_core::has_errors(&diags) {
        return Err(std::io::Error::other(format!("app payload check: {diags:?}")).into());
    }
    let canonical = nexus_dsl_core::format_file(&file);
    let lowered = nexus_dsl_core::lower_file(&file, &model, &canonical)
        .map_err(|d| std::io::Error::other(format!("app payload lower: {} {}", d.code, d.message)))?;
    std::fs::write(Path::new(&out_dir).join("app_payload.nxir"), &lowered.nxir)?;
    Ok(())
}
