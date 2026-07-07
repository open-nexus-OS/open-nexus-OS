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

const DSL_APP_DIR: &str = "../../../userspace/apps/counter";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo::rustc-check-cfg=cfg(nexus_env, values(\"os\",\"host\"))");
    println!("cargo:rerun-if-changed={DSL_APP_DIR}");
    let out_dir = std::env::var_os("OUT_DIR").ok_or("missing OUT_DIR")?;
    let nxir = nexus_dsl_core::compile_project_dir(Path::new(DSL_APP_DIR))
        .map_err(|e| std::io::Error::other(format!("app payload: {e}")))?;
    std::fs::write(Path::new(&out_dir).join("app_payload.nxir"), &nxir)?;
    Ok(())
}
