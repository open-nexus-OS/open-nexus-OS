// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The compositor's pipeline library (TASK-0215 5c-2 P4c): `compositor.toml` builds the committed
//! `compositor.nxgp` byte for byte; its entries are the hand-assembled references with the setup
//! programs the device decodes strictly, the blur passes with their constant blocks (P5d); a
//! variant above its budget and a manifest the tool does not know are refused.

use nexus_gfx::backend::gpu::scene::fnv1a64;
use nexus_gfx::backend::gpu::CoreId;
use nexus_gfx::shader::library::{feature, parse};
use nexus_gfx::shader::rogue::reference::{self, PASS_OPAQUE, PASS_TRANSLUCENT};
use nexus_gfx::shader::rogue::setup::Setup;
use nxgpu::library::{build, parse_manifest};

const MANIFEST: &str = include_str!("../../../resources/gpu/pipelines/compositor.toml");
const LIBRARY: &[u8] = include_bytes!("../../../resources/gpu/pipelines/compositor.nxgp");
const LAYER: &str = include_str!("../../../resources/gpu/shaders/layer.wgsl");
const SHADOW: &str = include_str!("../../../resources/gpu/shaders/shadow.wgsl");
const BLUR: &str = include_str!("../../../resources/gpu/shaders/blur.wgsl");

fn source(rel: &str) -> Result<String, String> {
    match rel {
        "../shaders/layer.wgsl" => Ok(LAYER.into()),
        "../shaders/shadow.wgsl" => Ok(SHADOW.into()),
        "../shaders/blur.wgsl" => Ok(BLUR.into()),
        other => Err(format!("no source {other}")),
    }
}

#[test]
fn the_manifest_builds_the_committed_library() {
    let manifest = parse_manifest(MANIFEST).expect("the manifest parses");
    let (bytes, built) = build(&manifest, source).expect("the library builds");
    assert_eq!(bytes, LIBRARY, "rebuild compositor.nxgp (README.md)");
    assert_eq!(fnv1a64(LIBRARY), 0xaa0d_92e8_58e9_f2dc, "and the README's digest");
    let facts: Vec<_> = built.iter().map(|b| (b.id, b.groups, b.temps, b.pass_type)).collect();
    assert_eq!(
        facts,
        vec![
            (1, 3, 1, PASS_OPAQUE),
            (2, 11, 3, PASS_TRANSLUCENT),
            (3, 15, 3, PASS_TRANSLUCENT),
            (4, 34, 3, PASS_TRANSLUCENT),
            (5, 43, 3, PASS_TRANSLUCENT),
            (6, 89, 3, PASS_OPAQUE),
            (7, 70, 3, PASS_OPAQUE)
        ]
    );
}

#[test]
fn the_library_carries_the_references_and_their_setup() {
    let lib = parse(LIBRARY, CoreId::BOARD.0, feature::FUSED_ITERATE_SAMPLE).expect("parses");
    assert!(lib.refused.is_empty());
    let [opaque, premultiplied, straight, rounded, shadow, across, down] = lib.accepted.as_slice()
    else {
        panic!("seven entries")
    };
    assert_eq!((opaque.id, opaque.name, opaque.budget), (1, "layer-opaque", 3));
    assert_eq!(opaque.program, reference::opaque_itrsmp().unwrap().encode().unwrap());
    assert_eq!(premultiplied.program, reference::premultiplied_itrsmp().unwrap().encode().unwrap());
    assert_eq!((straight.name, rounded.name), ("layer-straight", "layer-rounded"));
    assert_eq!((across.name, down.name), ("blur-horizontal-20", "blur-vertical-20"));
    // The layers sample with iterate-and-sample; the shadow samples nothing, the blur passes at
    // coordinates they compute. Only the blur passes carry a constant block (their weights).
    for (e, temps, pass, block) in [
        (opaque, 1, PASS_OPAQUE, false),
        (premultiplied, 3, PASS_TRANSLUCENT, false),
        (straight, 3, PASS_TRANSLUCENT, false),
        (rounded, 3, PASS_TRANSLUCENT, false),
        (shadow, 3, PASS_TRANSLUCENT, false),
        (across, 3, PASS_OPAQUE, true),
        (down, 3, PASS_OPAQUE, true),
    ] {
        assert_eq!(e.features != 0, e.id <= 4, "{}", e.name);
        let s = Setup::from_sections(&e.setup, e.state, &e.interface).expect("the setup decodes");
        assert_eq!((s.state.temps, s.state.pass_type), (temps, pass));
        assert_eq!(!s.constants.is_empty(), block, "{}", e.name);
    }
    // A device without iterate-and-sample takes the shadow and the blur passes.
    let bare = parse(LIBRARY, CoreId::BOARD.0, 0).unwrap();
    assert_eq!(bare.accepted.iter().map(|e| e.id).collect::<Vec<_>>(), vec![5, 6, 7]);
}

#[test]
fn test_reject_a_pipeline_over_its_budget() {
    let tight = MANIFEST.replacen("budget = 3\n", "budget = 2\n", 1);
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
