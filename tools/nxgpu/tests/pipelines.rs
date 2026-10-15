// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The compositor's pipeline library (TASK-0215 5c-2 P4c): `compositor.toml` builds the committed
//! `compositor.nxgp` byte for byte; its entries are the hand-assembled references with the setup
//! programs the device decodes strictly; a variant above its budget and a manifest the tool does
//! not know are refused.

use nexus_gfx::backend::gpu::scene::fnv1a64;
use nexus_gfx::backend::gpu::CoreId;
use nexus_gfx::shader::library::{feature, parse};
use nexus_gfx::shader::rogue::reference::{self, PASS_OPAQUE, PASS_TRANSLUCENT};
use nexus_gfx::shader::rogue::setup::Setup;
use nxgpu::library::{build, parse_manifest};

const MANIFEST: &str = include_str!("../../../resources/gpu/pipelines/compositor.toml");
const LIBRARY: &[u8] = include_bytes!("../../../resources/gpu/pipelines/compositor.nxgp");
const LAYER: &str = include_str!("../../../resources/gpu/shaders/layer.wgsl");

fn source(rel: &str) -> Result<String, String> {
    match rel {
        "../shaders/layer.wgsl" => Ok(LAYER.into()),
        other => Err(format!("no source {other}")),
    }
}

#[test]
fn the_manifest_builds_the_committed_library() {
    let manifest = parse_manifest(MANIFEST).expect("the manifest parses");
    let (bytes, built) = build(&manifest, source).expect("the library builds");
    assert_eq!(bytes, LIBRARY, "rebuild compositor.nxgp (README.md)");
    assert_eq!(fnv1a64(LIBRARY), 0x1084_b199_0109_bdfc, "and the README's digest");
    let facts: Vec<_> = built.iter().map(|b| (b.id, b.groups, b.temps, b.pass_type)).collect();
    assert_eq!(facts, vec![(1, 3, 1, PASS_OPAQUE), (2, 11, 3, PASS_TRANSLUCENT)]);
}

#[test]
fn the_library_carries_the_references_and_their_setup() {
    let lib = parse(LIBRARY, CoreId::BOARD.0, feature::FUSED_ITERATE_SAMPLE).expect("parses");
    assert!(lib.refused.is_empty());
    let [opaque, premultiplied] = lib.accepted.as_slice() else { panic!("two entries") };
    assert_eq!((opaque.id, opaque.name, opaque.budget), (1, "layer-opaque", 3));
    assert_eq!(opaque.program, reference::opaque_itrsmp().unwrap().encode().unwrap());
    assert_eq!(premultiplied.program, reference::premultiplied_itrsmp().unwrap().encode().unwrap());
    for (e, temps, pass) in [(opaque, 1, PASS_OPAQUE), (premultiplied, 3, PASS_TRANSLUCENT)] {
        assert_eq!(e.features, feature::FUSED_ITERATE_SAMPLE);
        let s = Setup::from_sections(&e.setup, e.state, &e.interface).expect("the setup decodes");
        assert_eq!((s.state.temps, s.state.pass_type), (temps, pass));
    }
    // A device without iterate-and-sample takes neither.
    assert_eq!(parse(LIBRARY, CoreId::BOARD.0, 0).unwrap().accepted.len(), 0);
}

#[test]
fn test_reject_a_pipeline_over_its_budget() {
    let tight = MANIFEST.replace("budget = 3", "budget = 2");
    let manifest = parse_manifest(&tight).expect("parses");
    let e = build(&manifest, source).unwrap_err();
    assert!(e.contains("layer-opaque: 3 groups above its budget of 2"), "{e}");
}

#[test]
fn test_reject_a_manifest_the_tool_does_not_know() {
    for (from, to) in [
        ("target = \"board\"", "target = \"other\""),
        ("blend = \"replace\"", "blend = \"multiply\""),
        ("budget = 3", "budget = 3\ncolour = 1"),
        ("id = 1", "id = 0"),
        ("overrides = { tinted = false }", "overrides = { tinted = \"no\" }"),
    ] {
        assert!(parse_manifest(&MANIFEST.replacen(from, to, 1)).is_err(), "{to}");
    }
    let unknown = MANIFEST.replace("{ tinted = false }", "{ shaded = false }");
    assert!(build(&parse_manifest(&unknown).unwrap(), source).is_err());
}
