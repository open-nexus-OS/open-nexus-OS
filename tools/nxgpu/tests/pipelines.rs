// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The compositor's compiled pipelines (TASK-0215 5c-2 P3c): `layer.wgsl` compiled by the
//! toolchain reproduces the files in `resources/gpu/pipelines` byte for byte, and those are the
//! hand-assembled references — source, compiler, artifact and reference agree.

use nexus_gfx::backend::gpu::scene::fnv1a64;
use nexus_gfx::shader::rogue::encode_program;
use nexus_gfx::shader::rogue::reference::{self, PASS_OPAQUE, PASS_TRANSLUCENT};
use nxgpu::{compile_fragment, Blend};

const LAYER: &str = include_str!("../../../resources/gpu/shaders/layer.wgsl");
const OPAQUE: &[u8] = include_bytes!("../../../resources/gpu/pipelines/layer-opaque.usc");
const PREMULTIPLIED: &[u8] =
    include_bytes!("../../../resources/gpu/pipelines/layer-premultiplied.usc");

#[test]
fn the_layer_compiles_to_the_committed_pipelines() {
    let opaque = compile_fragment(LAYER, "main", &[("tinted", 0.0)], Blend::Replace, true)
        .expect("compiles");
    let premultiplied =
        compile_fragment(LAYER, "main", &[], Blend::OverPremultiplied, true).expect("compiles");
    assert_eq!(encode_program(&opaque.groups).unwrap(), OPAQUE, "rebuild layer-opaque.usc");
    assert_eq!(
        encode_program(&premultiplied.groups).unwrap(),
        PREMULTIPLIED,
        "rebuild layer-premultiplied.usc"
    );
    // gpud's table names these until the pipeline library carries them.
    assert_eq!((opaque.temps, opaque.pass_type, opaque.groups.len()), (1, PASS_OPAQUE, 3));
    assert_eq!(
        (premultiplied.temps, premultiplied.pass_type, premultiplied.groups.len()),
        (3, PASS_TRANSLUCENT, 11)
    );
    assert_eq!(
        (fnv1a64(OPAQUE), fnv1a64(PREMULTIPLIED)),
        (0xfc3a_43a9_c898_99e6, 0x20e4_e05b_2d8f_ff4d)
    );
}

#[test]
fn the_pipelines_are_the_references() {
    // The opaque layer is the program board cycle 14 measured at 1.30 ns a pixel.
    assert_eq!(OPAQUE, reference::opaque_itrsmp().unwrap().encode().unwrap());
    assert_eq!(PREMULTIPLIED, reference::premultiplied_itrsmp().unwrap().encode().unwrap());
}
