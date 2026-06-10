// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

fn main() {
    // Register the nexus_env cfg used by os/host selection.
    println!("cargo::rustc-check-cfg=cfg(nexus_env, values(\"os\",\"host\"))");
}
