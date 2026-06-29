// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

fn main() {
    // Register the nexus_env cfg used by os/host selection.
    println!("cargo::rustc-check-cfg=cfg(nexus_env, values(\"os\",\"host\"))");
    // Rebuild when the per-group verdict expand set changes, so `option_env!("NEXUS_LOG_EXPAND")`
    // (consumed in bootstrap) is never stale — `NEXUS_LOG_EXPAND=netstackd just start` takes effect.
    println!("cargo::rerun-if-env-changed=NEXUS_LOG_EXPAND");
}
