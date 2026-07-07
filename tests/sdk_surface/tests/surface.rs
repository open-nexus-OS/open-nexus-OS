// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! SDK-surface pins (TASK-0081 C0): the curated list stays real and the
//! trust-boundary crates stay off it.

use sdk_surface::{repo_root, sdk_entries};

#[test]
fn every_sdk_entry_is_a_real_crate() {
    let entries = sdk_entries();
    assert!(entries.len() >= 8, "curated list unexpectedly small: {entries:?}");
    for (name, path) in &entries {
        assert!(!path.is_empty(), "{name}: missing path");
        let cargo = repo_root().join(path).join("Cargo.toml");
        assert!(cargo.is_file(), "{name}: {path}/Cargo.toml does not exist");
        let text = std::fs::read_to_string(&cargo).expect("readable");
        assert!(
            text.contains(&format!("name = \"{name}\"")),
            "{name}: crate name mismatch under {path}"
        );
    }
}

#[test]
fn trust_boundary_crates_are_never_sdk_public() {
    let names: Vec<String> = sdk_entries().into_iter().map(|(n, _)| n).collect();
    for forbidden in ["nexus-abi", "nexus-ipc", "nexus-service-entry", "neuron", "nexus-driverkit"]
    {
        assert!(
            !names.iter().any(|n| n == forbidden),
            "{forbidden} is OS-internal (trust boundary) and must not be SDK-public"
        );
    }
}
