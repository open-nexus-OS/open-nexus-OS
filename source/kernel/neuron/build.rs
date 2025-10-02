// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

fn main() {
    println!("cargo:rerun-if-env-changed=NEURON_LINKER_SCRIPT");
    if let Ok(script) = std::env::var("NEURON_LINKER_SCRIPT") {
        println!("cargo:rustc-link-arg=-T{script}");
    }
}
