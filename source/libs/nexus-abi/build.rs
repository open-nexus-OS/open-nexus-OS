// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

fn main() {
    // Allow custom cfg used across os/host builds.
    println!("cargo::rustc-check-cfg=cfg(nexus_env, values(\"os\",\"host\"))");
}
