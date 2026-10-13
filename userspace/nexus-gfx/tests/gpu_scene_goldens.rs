// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The first quad's bundle against the capture (TASK-0169B P1, RFC-0101 §8c): the committed
//! `first-quad.scene` parses for the board's core, its two transfer streams decode to the
//! registers the measurement README lists, its target is one plane of the ring and its texture
//! the checker — and the file's digest is pinned, so a silent repack is seen here first.

use nexus_gfx::backend::gpu::fwif::{flags, StreamFeatures, TransferCmd};
use nexus_gfx::backend::gpu::scene::{fnv1a64, JobKind, Kind, Scene, Texture};
use nexus_gfx::backend::gpu::{heaps, CoreId};

const BUNDLE: &[u8] = include_bytes!("../../../resources/gpu/scenes/first-quad.scene");

/// One plane of the ring: 1080 rows of 7680 bytes.
const PLANE_BYTES: u64 = 1080 * 7680;

fn scene() -> Scene<'static> {
    Scene::parse(BUNDLE, CoreId::BOARD).expect("the bundle parses for the board's core")
}

#[test]
fn the_bundle_is_the_capture_of_2026_10_13() {
    assert_eq!(
        fnv1a64(BUNDLE),
        0x699f_f414_b267_2125,
        "repacked: update the digest here and in resources/gpu/scenes/README.md"
    );
}

#[test]
fn the_bundle_names_the_core_and_the_texture() {
    let s = scene();
    assert_eq!(s.core, CoreId::BOARD);
    assert_eq!(s.texture, Texture::CheckerV1);
    assert_eq!((s.buffer_count, s.job_count), (29, 2));
    assert_eq!(s.data_bytes(), 33031);
}

#[test]
fn the_target_is_one_plane_and_the_texture_its_capture_buffer() {
    let s = scene();
    let target = s.target().expect("one target");
    assert_eq!((target.va, target.len), (0x80_000b_1000, PLANE_BYTES));
    let texture = s.buffers().find(|b| b.kind == Kind::Texture).expect("one texture");
    assert_eq!((texture.va, texture.len, texture.run_count), (0x80_0089_a000, 256 * 1024, 0));
    for b in s.buffers() {
        let heap = [
            (heaps::GENERAL_BASE, heaps::GENERAL_SIZE),
            (heaps::PDS_CODE_DATA_BASE, heaps::PDS_CODE_DATA_SIZE),
            (heaps::USC_CODE_BASE, heaps::USC_CODE_SIZE),
        ]
        .iter()
        .find(|(base, size)| b.va >= *base && b.va + b.len <= base + size);
        assert!(heap.is_some(), "buffer {:#x} lies in one of the three heaps", b.va);
    }
}

#[test]
fn the_two_jobs_decode_to_the_measured_registers() {
    let s = scene();
    let jobs: Vec<_> = s.jobs().collect();
    assert_eq!(jobs.len(), 2);
    for j in &jobs {
        assert_eq!((j.kind, j.single_core(), j.stream.len()), (JobKind::Transfer, true, 176));
    }
    let clear = TransferCmd::from_stream(jobs[0].stream, StreamFeatures::BOARD, true).unwrap();
    let blit = TransferCmd::from_stream(jobs[1].stream, StreamFeatures::BOARD, true).unwrap();
    // The clear: the colour as the pixel program outputs it (R 0x1a, G 0x1f, B 0x29, A 0xff),
    // no background program.
    assert_eq!(clear.regs.usc_clear_register0, 0xff29_1f1a);
    assert_eq!(clear.regs.pds_bgnd0_base, 0);
    assert_eq!(clear.regs.isp_mtile_size, 0x0078_0044);
    assert_eq!(clear.regs.pbe_wordx_mrty[0], 0x0200_0efc_60a0_0000);
    assert_eq!(clear.regs.pbe_wordx_mrty[1], 0x0000_077f_0000_0437);
    // The blit: the per-blit PDS data at 0xda00041000 behind the background base, the tile walk.
    assert_eq!(blit.regs.pds_bgnd0_base, 0x1e40_0004_1000);
    assert_eq!(blit.regs.pds_bgnd1_base, 0x2640_0000_0000);
    assert_eq!(blit.regs.pds_bgnd3_sizeinfo, 0x0100_0000_0001_0001);
    assert_eq!(blit.regs.isp_bgobjvals, 0x200);
    assert_eq!(blit.regs.isp_mtile_size, 0x0020_0021);
    assert_eq!(blit.regs.isp_render_origin, 0x002c_0011);
    assert_eq!(blit.regs.usc_clear_register0, 0);
    assert_eq!(blit.regs.event_pixel_pds_code, 0x26a0);
    // Shared by both: the pixel output control, the ISP control, the frame's screen word.
    for cmd in [&clear, &blit] {
        assert_eq!(cmd.regs.usc_pixel_output_ctrl, 0x7f8);
        assert_eq!(cmd.regs.isp_ctl, 0x21000);
        assert_eq!(cmd.regs.frag_screen, 0x0437_077f);
        assert_eq!(cmd.regs.isp_render, 0x3);
        assert_eq!(cmd.flags, flags::SINGLE_CORE);
        assert_eq!(cmd.to_bytes().len(), TransferCmd::SIZE);
    }
    // The job-1-only buffer the blit's background base points at.
    assert!(s.buffers().any(|b| b.va == 0xda_0004_1000 && b.run_count == 1));
}
