// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Our texture descriptors (TASK-0215 5c-2 P5d-1) against the render bundle's: the checker's
//! image and sampler words word for word; the open driver's query extras are not ours; a
//! texture the fields cannot hold is refused.

use nexus_gfx::backend::gpu::scene::Scene;
use nexus_gfx::backend::gpu::texstate::{descriptor, format, swizzle, Image, Sampler, TexError};
use nexus_gfx::backend::gpu::CoreId;

const RENDER: &[u8] = include_bytes!("../../../resources/gpu/scenes/render-quad.scene");

/// The bundle's descriptor block: the general arena's line at 0x80.
fn captured() -> Vec<u32> {
    let scene = Scene::parse(RENDER, CoreId::BOARD).expect("the render bundle parses");
    let buf = scene.buffers().find(|b| b.va == 0x80_0004_b000).expect("the general arena");
    let mut bytes = vec![0u8; buf.len as usize];
    for run in buf.runs() {
        bytes[run.off as usize..run.off as usize + run.bytes.len()].copy_from_slice(run.bytes);
    }
    bytes[0x80..0x80 + 80]
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

const CHECKER: Image = Image {
    address: 0x80_0089_a000,
    width: 256,
    height: 256,
    stride: 256,
    format: format::U8U8U8U8,
    swizzle: swizzle::IDENTITY,
};

#[test]
fn the_checkers_image_and_sampler_words_are_the_bundles() {
    let ours = descriptor(&CHECKER, &Sampler::BILINEAR_CLAMP).unwrap();
    let theirs = captured();
    assert_eq!(ours[..4], theirs[..4], "the image words");
    assert_eq!(ours[8..12], theirs[8..12], "the sampler words");
    // The open driver's extras for size queries: the image's bytes (256 x 256 x 4) and its
    // query word, a second sampler copy — none of ours.
    assert_eq!((theirs[4], theirs[7]), (256 * 256 * 4, 0x140));
    assert_eq!(theirs[16..18], theirs[8..10]);
    assert!(ours[4..8].iter().chain(&ours[12..]).all(|w| *w == 0));
}

#[test]
fn a_plane_rectangle_is_a_stride_texture_of_its_own() {
    // A 960x540 rectangle at (100, 50) of a 1920-wide plane of blue-green-red-alpha words.
    let base = 0x80_2000_0000u64;
    let img = Image {
        address: base + (50 * 1920 + 100) * 4,
        width: 960,
        height: 540,
        stride: 1920,
        format: format::U8U8U8U8,
        swizzle: swizzle::BGRA,
    };
    let d = descriptor(&img, &Sampler::BILINEAR_CLAMP).unwrap();
    let w0 = u64::from(d[0]) | (u64::from(d[1]) << 32);
    let w1 = u64::from(d[2]) | (u64::from(d[3]) << 32);
    let f = |v: u64, s: u32, e: u32| (v >> s) & ((1u64 << (e - s + 1)) - 1);
    assert_eq!((f(w0, 34, 47), f(w0, 48, 61)), (959, 539));
    assert_eq!((f(w0, 14, 16), f(w0, 11, 13), f(w0, 8, 10), f(w0, 5, 7)), (2, 1, 0, 3));
    assert_eq!((f(w1, 0, 14), f(w1, 16, 53) << 2), (1919, img.address));
}

#[test]
fn test_reject_a_texture_its_fields_cannot_hold() {
    let s = Sampler::BILINEAR_CLAMP;
    assert_eq!(descriptor(&Image { width: 0, ..CHECKER }, &s), Err(TexError("size")));
    assert_eq!(descriptor(&Image { stride: 100, ..CHECKER }, &s), Err(TexError("size")));
    assert_eq!(
        descriptor(&Image { address: CHECKER.address + 2, ..CHECKER }, &s),
        Err(TexError("address"))
    );
    assert_eq!(
        descriptor(&Image { width: 20000, stride: 20000, ..CHECKER }, &s),
        Err(TexError("width"))
    );
    assert_eq!(
        descriptor(&Image { swizzle: [6, 1, 2, 3], ..CHECKER }, &s),
        Err(TexError("swizzle"))
    );
    assert_eq!(descriptor(&Image { address: 1 << 41, ..CHECKER }, &s), Err(TexError("address")));
}
