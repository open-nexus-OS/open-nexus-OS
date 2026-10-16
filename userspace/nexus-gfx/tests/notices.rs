// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The third-party notices stay true to the tree: `THIRD_PARTY_NOTICES` carries the MIT notices of
//! the two open GPU drivers the GPU stack derives from — each with its permission text and its
//! warranty disclaimer — and every directory it names as derived or licensed exists, so a module
//! that moves takes its notice along instead of leaving it behind.

use std::path::Path;

const NOTICES: &str = include_str!("../../../THIRD_PARTY_NOTICES");

fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

#[test]
fn both_drivers_mit_notices_are_reproduced_whole() {
    // The kernel driver's text and the userspace driver's variant ("including the next
    // paragraph"), each with the disclaimer the licence requires.
    let permission = "Permission is hereby granted, free of charge, to any person obtaining a copy";
    let disclaimer = "THE SOFTWARE IS PROVIDED \"AS IS\", WITHOUT WARRANTY OF ANY KIND";
    assert_eq!(NOTICES.matches(permission).count(), 2);
    assert_eq!(NOTICES.matches(disclaimer).count(), 2);
    assert!(NOTICES.contains("The above copyright notice and this permission notice shall be"));
    assert!(NOTICES.contains("The above copyright notice and this permission notice (including"));
    assert!(NOTICES.contains("used here under the MIT option"));
}

#[test]
fn every_directory_the_notices_name_exists() {
    let named: Vec<&str> = NOTICES
        .lines()
        .map(str::trim_start)
        .filter_map(|l| l.split_whitespace().next())
        .filter(|w| w.ends_with('/') && !w.contains("://"))
        .filter(|w| {
            ["userspace/", "source/", "resources/", "vendor/", "tools/"]
                .iter()
                .any(|p| w.starts_with(p))
        })
        .collect();
    // The GPU stack's derived modules and the assets with licences of their own.
    for must in [
        "userspace/nexus-gfx/src/backend/gpu/",
        "userspace/nexus-gfx/src/shader/rogue/",
        "source/drivers/gpud/src/backend/gpu/",
        "resources/firmware/gpu/",
    ] {
        assert!(named.contains(&must), "{must} named in the notices");
    }
    for dir in &named {
        assert!(root().join(dir).is_dir(), "{dir} named in THIRD_PARTY_NOTICES does not exist");
    }
}
