// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

fn main() {
    println!("cargo::rustc-check-cfg=cfg(nexus_env, values(\"os\",\"host\"))");
}
