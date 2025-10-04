// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

// Build script: compile all .capnp schemas from tools/nexus-idl/schemas into OUT_DIR.
fn main() {
    #[cfg(feature = "capnp")]
    {
        let schemas = std::path::Path::new("../../tools/nexus-idl/schemas");
        if schemas.exists() {
            let mut cmd = capnpc::CompilerCommand::new();
            cmd.output_path(std::env::var("OUT_DIR").unwrap());
            cmd.file(schemas.join("samgr.capnp"));
            cmd.file(schemas.join("bundlemgr.capnp"));
            cmd.run().expect("capnp compile failed");
        }
    }
}
