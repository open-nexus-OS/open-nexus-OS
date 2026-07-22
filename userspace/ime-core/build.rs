// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

// This crate doubles as a BUILD-dependency (the font bake) where the
// workspace RUSTFLAGS do not apply — declare the nexus cfg axis so the
// host compile stays warning-clean under the hard warn gate.
fn main() {
    println!("cargo::rustc-check-cfg=cfg(nexus_env, values(\"os\",\"host\"))");
}
