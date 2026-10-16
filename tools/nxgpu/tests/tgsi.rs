// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The virtual GPU's programs (TASK-0215 5c-2 P6): the same WGSL sources as the board's, compiled
//! into the host renderer's shader text — the quad's vertex program and the opaque layer word for
//! word in the dialect the hand-written shaders used; `compositor-virgl.toml` builds the committed
//! `compositor-virgl.nxgp` byte for byte, every entry its stage's text with its blend as its
//! state, held to its instructions; a program reading its output is refused (the blend is the
//! pipeline's state on this target); the gradient gpud's probe draws runs from its top colour to
//! its bottom one with its corners cut (`Quad::gradient`).

use nexus_gfx::backend::gpu::quads::Quad;
use nexus_gfx::backend::gpu::scene::fnv1a64;
use nexus_gfx::shader::ir::builder::Blend;
use nexus_gfx::shader::ir::interp::{run, Invocation};
use nexus_gfx::shader::ir::Stage;
use nexus_gfx::shader::library::parse;
use nexus_gfx::shader::tgsi::{self, State, TgsiError};
use nxgpu::library::{build, parse_manifest};
use nxgpu::{lower_fragment, lower_vertex};

const MANIFEST: &str = include_str!("../../../resources/gpu/pipelines/compositor-virgl.toml");
const LIBRARY: &[u8] = include_bytes!("../../../resources/gpu/pipelines/compositor-virgl.nxgp");
const QUAD: &str = include_str!("../../../resources/gpu/shaders/quad.wgsl");
const LAYER: &str = include_str!("../../../resources/gpu/shaders/layer.wgsl");
const SHADOW: &str = include_str!("../../../resources/gpu/shaders/shadow.wgsl");
const BLUR: &str = include_str!("../../../resources/gpu/shaders/blur.wgsl");
const GRADIENT: &str = include_str!("../../../resources/gpu/shaders/gradient.wgsl");

fn source(rel: &str) -> Result<String, String> {
    Ok(match rel {
        "../shaders/quad.wgsl" => QUAD,
        "../shaders/layer.wgsl" => LAYER,
        "../shaders/shadow.wgsl" => SHADOW,
        "../shaders/blur.wgsl" => BLUR,
        "../shaders/gradient.wgsl" => GRADIENT,
        other => return Err(format!("no source {other}")),
    }
    .into())
}

#[test]
fn the_quads_vertex_program_passes_its_corner_and_gives_uv() {
    let m = lower_vertex(QUAD, "main", &[]).expect("the quad lowers");
    let text = tgsi::compile(&m).expect("compiles");
    assert_eq!(
        text,
        "VERT\n\
         DCL IN[0]\n\
         DCL OUT[0], POSITION\n\
         DCL OUT[1], GENERIC[0]\n\
         DCL TEMP[0..0]\n\
         IMM[0] FLT32 { 0.5, 0, 0, 0}\n\
         MOV OUT[0], IN[0].xyzw\n\
         MAD TEMP[0].xy, IN[0].xyyy, IMM[0].xxxx, IMM[0].xxxx\n\
         MOV OUT[1].xy, TEMP[0].xyyy\n\
         END\n"
    );
}

#[test]
fn the_opaque_layer_is_one_sample_of_the_varying() {
    let m = lower_fragment(LAYER, "main", &[("tinted", 0.0)], Blend::Replace).expect("lowers");
    let text = tgsi::compile(&m).expect("compiles");
    assert_eq!(
        text,
        "FRAG\n\
         DCL IN[0], GENERIC[0], PERSPECTIVE\n\
         DCL OUT[0], COLOR\n\
         DCL SAMP[0]\n\
         DCL SVIEW[0], 2D, FLOAT\n\
         DCL CONST[0..3]\n\
         DCL TEMP[0..0]\n\
         TEX TEMP[0], IN[0].xyyy, SAMP[0], 2D\n\
         MOV OUT[0], TEMP[0].xyzw\n\
         END\n"
    );
}

#[test]
fn the_manifest_builds_the_committed_virgl_library() {
    let manifest = parse_manifest(MANIFEST).expect("the manifest parses");
    assert_eq!(manifest.target, tgsi::CODE_TARGET);
    let (bytes, built) = build(&manifest, source).expect("the library builds");
    assert_eq!(bytes, LIBRARY, "rebuild compositor-virgl.nxgp (README.md)");
    assert_eq!(fnv1a64(LIBRARY), 0x7189_c506_645a_6fb2, "and the README's digest");
    // Every budget is the program's own count: a change in the toolchain shows here first.
    assert!(built.iter().all(|b| b.groups == usize::from(b.budget)), "{built:?}");
}

#[test]
fn every_entry_is_its_stages_text_with_its_blend_as_its_state() {
    let lib = parse(LIBRARY, tgsi::CODE_TARGET, 0).expect("parses for the virtual GPU");
    assert!(lib.refused.is_empty());
    let manifest = parse_manifest(MANIFEST).expect("the manifest parses");
    assert_eq!(lib.accepted.len(), manifest.pipelines.len());
    for (e, p) in lib.accepted.iter().zip(&manifest.pipelines) {
        assert_eq!((e.id, e.name), (p.id, p.name.as_str()));
        assert_eq!(e.interface.stage, p.stage, "{}", e.name);
        let text = tgsi::check_text(e.program, e.interface.stage).expect("the device takes it");
        assert_eq!(tgsi::instructions(text), usize::from(e.budget), "{}", e.name);
        assert!(e.setup.is_empty(), "{}: no setup programs on this target", e.name);
        assert_eq!(State::from_bytes(e.state), Ok(State { blend: p.blend }), "{}", e.name);
    }
    // The board takes none of it.
    assert!(parse(LIBRARY, nexus_gfx::backend::gpu::CoreId::BOARD.0, 0).is_err());
    assert_eq!(lib.accepted[0].interface.stage, Stage::Vertex);
}

#[test]
fn test_reject_a_program_that_reads_its_output() {
    let m = lower_fragment(LAYER, "main", &[], Blend::OverPremultiplied).expect("lowers");
    assert!(
        matches!(tgsi::compile(&m), Err(TgsiError::Unsupported { what, .. }) if what.contains("tile read"))
    );
}

#[test]
fn test_reject_a_manifest_stage_the_target_does_not_have() {
    let board = include_str!("../../../resources/gpu/pipelines/compositor.toml");
    let vertex = board.replacen("blend = \"replace\"\n", "stage = \"vertex\"\n", 1);
    assert!(parse_manifest(&vertex).is_err(), "the board's library declares no vertex program");
    let blended =
        MANIFEST.replacen("stage = \"vertex\"\n", "stage = \"vertex\"\nblend = \"replace\"\n", 1);
    assert!(parse_manifest(&blended).is_err(), "a vertex program blends nothing");
}

#[test]
fn the_gradient_runs_down_its_rectangle_with_its_corners_cut() {
    // The pointer's fallback (`Quad::gradient`, the one place its block is written down): red at
    // the top, blue at the bottom, uv 0 at the top; a 16-pixel corner cut away, premultiplied.
    let m = lower_fragment(GRADIENT, "main", &[], Blend::Replace).expect("the gradient lowers");
    let q = Quad::gradient(0.0, 0.0, 64.0, 64.0, [1.0, 0.0, 0.0, 1.0], [0.0, 0.0, 1.0, 1.0], 16.0);
    let push: Vec<u8> = q.push.iter().flat_map(|x| x.to_le_bytes()).collect();
    let at = |x: f32, y: f32| {
        let uv = [(x + 0.5) / 64.0, (y + 0.5) / 64.0, 0.0, 0.0];
        let inv = Invocation { inputs: &[uv], push: &push, textures: &[], dst: &[[0.0; 4]] };
        run(&m, &inv).expect("the gradient runs")[0]
    };
    let (top, bottom, corner) = (at(32.0, 0.0), at(32.0, 63.0), at(0.0, 0.0));
    assert!(top[0] > 0.95 && top[2] < 0.05 && top[3] == 1.0, "{top:?}");
    assert!(bottom[2] > 0.95 && bottom[0] < 0.05 && bottom[3] == 1.0, "{bottom:?}");
    assert_eq!(corner, [0.0; 4]);
}
