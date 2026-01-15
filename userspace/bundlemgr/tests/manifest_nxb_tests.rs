// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
// CONTEXT: Legacy test filename kept for stability; smoke-check only.
// OWNERS: @runtime
//
// NOTE:
// The canonical manifest.nxb test suite lives in `manifest_tests.rs`.
// This file remains as a small smoke test so we don't duplicate full coverage.

use bundlemgr::manifest::Manifest;
use capnp::message::Builder;
use nexus_idl_runtime::manifest_capnp::bundle_manifest;

#[test]
fn smoke_parse_manifest_nxb_ok() {
    let mut builder = Builder::new_default();
    {
        let mut msg = builder.init_root::<bundle_manifest::Builder<'_>>();
        msg.set_schema_version(1);
        msg.set_name("demo.smoke");
        msg.set_semver("1.0.0");
        msg.set_min_sdk("0.1.0");
        msg.set_publisher(&[0u8; 16]);
        msg.set_signature(&[0u8; 64]);
        let mut abilities = msg.reborrow().init_abilities(1);
        abilities.set(0, "demo");
        let _caps = msg.reborrow().init_capabilities(0);
    }
    let mut bytes = Vec::new();
    capnp::serialize::write_message(&mut bytes, &builder).unwrap();
    let m = Manifest::parse_nxb(&bytes).expect("parse ok");
    assert_eq!(m.name, "demo.smoke");
}
