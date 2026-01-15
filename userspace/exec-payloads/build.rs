// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
// CONTEXT: Build-time generation of deterministic manifest.nxb fixtures
// OWNERS: @runtime
//
// This crate is `no_std` and cannot depend on Cap'n Proto at runtime, but we still
// want repository-wide single-truth fixtures (`manifest.nxb`) without committing
// binary artifacts. We therefore generate the bytes at build time and include
// them via `include_bytes!` from the library code.

use std::{env, fs, io, path::PathBuf};

use capnp::message::Builder;

fn main() -> Result<(), io::Error> {
    // Ensure rebuild if schema changes (single source of truth).
    println!("cargo:rerun-if-changed=../../tools/nexus-idl/schemas/manifest.capnp");
    println!("cargo:rerun-if-changed=src/hello_elf.rs");

    let out_dir = PathBuf::from(env::var("OUT_DIR").map_err(io::Error::other)?);
    fs::write(out_dir.join("hello.manifest.nxb"), build_manifest("demo.hello", "0.0.1"))?;
    fs::write(out_dir.join("exit0.manifest.nxb"), build_manifest("demo.exit0", "1.0.0"))?;

    Ok(())
}

fn build_manifest(name: &str, semver: &str) -> Vec<u8> {
    // Use the generated schema from nexus-idl-runtime (host build, std OK).
    use nexus_idl_runtime::manifest_capnp::bundle_manifest;

    let mut builder = Builder::new_default();
    let mut msg = builder.init_root::<bundle_manifest::Builder>();

    msg.set_schema_version(1);
    msg.set_name(name);
    msg.set_semver(semver);

    {
        let mut abilities = msg.reborrow().init_abilities(1);
        abilities.set(0, "demo");
    }
    {
        let _caps = msg.reborrow().init_capabilities(0);
    }

    msg.set_min_sdk("0.1.0");
    msg.set_publisher(&[0u8; 16]);
    msg.set_signature(&[0u8; 64]);

    // v1.1 fields intentionally left at defaults in v1.0.
    // payloadDigest: empty
    // payloadSize: 0

    let mut out: Vec<u8> = Vec::new();
    if let Err(err) = capnp::serialize::write_message(&mut out, &builder) {
        panic!("exec-payloads: capnp serialize failed: {err}");
    }
    out
}
