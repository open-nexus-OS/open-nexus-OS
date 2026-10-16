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

// ---- The render pass's bundle (TASK-0215 5a, RFC-0101 §8d) --------------------------------------

mod render_quad {
    use nexus_gfx::backend::gpu::fwif::{
        frag_flags, geom_flags, FragCmd, FreelistArgs, GeomCmd, HwrtArgs, RenderFeatures,
        StaticRenderContextState, Ufo,
    };
    use nexus_gfx::backend::gpu::scene::{fnv1a64, JobKind, Kind, Scene, Texture};
    use nexus_gfx::backend::gpu::CoreId;

    const BUNDLE: &[u8] = include_bytes!("../../../resources/gpu/scenes/render-quad.scene");

    fn scene() -> Scene<'static> {
        Scene::parse(BUNDLE, CoreId::BOARD).expect("the render bundle parses for the board's core")
    }

    #[test]
    fn the_bundle_is_the_capture_of_2026_10_13() {
        assert_eq!(
            fnv1a64(BUNDLE),
            0x8a7e_d6a6_b444_2413,
            "repacked: update the digest and the README"
        );
        let s = scene();
        assert_eq!((s.buffer_count, s.job_count, s.texture), (37, 3, Texture::CheckerV1));
        assert_eq!(s.data_bytes(), 34614);
    }

    #[test]
    fn the_target_is_the_first_plane_and_the_scratch_is_zero_data() {
        let s = scene();
        let target = s.target().expect("one target");
        assert_eq!((target.va, target.len), (0x80_000b_1000, 1080 * 7680));
        let scratch = s
            .buffers()
            .find(|b| b.va == 0x80_008d_b000)
            .expect("the partial-render scratch buffer");
        assert_eq!(
            (scratch.kind, scratch.len, scratch.run_count),
            (Kind::Data, 1080 * 7680, 0),
            "carried as zero data"
        );
        assert!(s.buffers().any(|b| b.va == 0xdb_f000_0000), "region headers in the RGNHDR heap");
    }

    #[test]
    fn the_trailer_carries_the_datasets_arguments_lists_and_state() {
        let s = scene();
        let r = s.render().expect("the render trailer");
        let args = HwrtArgs::parse(r.hwrt_args).unwrap();
        assert_eq!((args.width, args.height, args.samples, args.layers), (1920, 1080, 1, 1));
        assert_eq!(args.tpc_dev_addr, 0x80_010c_7000);
        assert_eq!(args.rt_data[0].region_header_dev_addr, 0xdb_f000_0000);
        assert_eq!(args.free_list_handles, [2, 1]);
        assert_eq!(r.callstack_addr, 0x80_0006_d000);
        assert_eq!(
            r.freelists,
            [
                FreelistArgs {
                    gpu_addr: 0x80_010c_5000,
                    initial_pages: 32,
                    max_pages: 32,
                    grow_pages: 0,
                    grow_threshold_percent: 0
                },
                FreelistArgs {
                    gpu_addr: 0x80_0000_9000,
                    initial_pages: 512,
                    max_pages: 65536,
                    grow_pages: 256,
                    grow_threshold_percent: 13
                },
            ]
        );
        for fl in &r.freelists {
            assert_eq!(fl.check(25), Ok(()));
            assert!(
                s.buffers().any(|b| b.va == fl.gpu_addr && b.len as usize >= fl.list_bytes()),
                "the list buffer is bound"
            );
        }
        let st = StaticRenderContextState::from_stream(r.static_state).unwrap();
        assert_eq!(st.regs[0], 0x80_0006_e000, "the VDM context state base");
        assert_eq!(st.regs[2], 0x80_0006_f000, "the TA context state base");
    }

    #[test]
    fn the_captured_streams_round_trip_through_the_encoders() {
        let s = scene();
        let jobs: Vec<_> = s.jobs().collect();
        let pr_fence = Ufo { addr: 0, value: 0 };
        let g =
            GeomCmd::from_stream(jobs[0].stream, RenderFeatures::BOARD, jobs[0].flags, 1, pr_fence)
                .unwrap();
        assert_eq!(&*g.to_stream(RenderFeatures::BOARD), jobs[0].stream);
        for job in &jobs[1..] {
            let f = FragCmd::from_stream(job.stream, RenderFeatures::BOARD, job.flags, 1).unwrap();
            assert_eq!(&*f.to_stream(RenderFeatures::BOARD), job.stream);
        }
    }

    #[test]
    fn the_three_jobs_decode_in_the_open_drivers_order() {
        let s = scene();
        let jobs: Vec<_> = s.jobs().collect();
        assert_eq!(
            jobs.iter().map(|j| (j.kind, j.flags)).collect::<Vec<_>>(),
            [(JobKind::Geometry, 0x3), (JobKind::FragmentPartial, 0xc0), (JobKind::Fragment, 0x80)]
        );
        let pr_fence = Ufo { addr: 0xc000_0000, value: 0 };
        let g =
            GeomCmd::from_stream(jobs[0].stream, RenderFeatures::BOARD, jobs[0].flags, 1, pr_fence)
                .unwrap();
        assert_eq!(g.regs.vdm_ctrl_stream_base, 0x80_010d_1000, "the geometry control stream");
        assert_eq!(g.regs.tpu_border_colour_table, 0x20_0001_c400);
        assert_eq!((g.regs.ppp_ctrl, g.regs.te_psg), (0x202, 0x20003));
        assert_eq!(g.regs.vdm_context_resume_task0_size, 0x2002_0000);
        assert_eq!(g.flags, geom_flags::FIRSTKICK | geom_flags::LASTKICK);
        assert!(
            s.buffers().any(|b| b.va == g.regs.vdm_ctrl_stream_base),
            "the control stream is bound"
        );
        let pr =
            FragCmd::from_stream(jobs[1].stream, RenderFeatures::BOARD, jobs[1].flags, 1).unwrap();
        let f =
            FragCmd::from_stream(jobs[2].stream, RenderFeatures::BOARD, jobs[2].flags, 1).unwrap();
        for cmd in [&pr, &f] {
            assert_eq!(
                cmd.regs.pbe_word[0], 0x0200_0efc_60a0_0000,
                "the plane's back-end word, as the transfer capture's"
            );
            assert_eq!(cmd.regs.pbe_word[1], 0x0000_077f_0000_0437);
            assert_eq!(cmd.regs.isp_scissor_base, 0x80_0005_3280);
            assert_eq!(cmd.regs.usc_pixel_output_ctrl, 0x7f8);
            assert_eq!(cmd.regs.isp_bgobjdepth, 0x3f80_0000);
            assert_eq!(cmd.regs.isp_ctl, 0x8003_1000);
            assert_eq!(cmd.regs.event_pixel_pds_data, 0x2980);
            assert_eq!(
                cmd.regs.usc_clear_register, [0; 8],
                "the clear is the background program's, not the register's"
            );
            assert_eq!(cmd.flags, frag_flags::DISABLE_PIXELMERGE);
        }
        assert_ne!(jobs[1].flags & frag_flags::SUBMIT_PARTIAL_RENDER, 0);
        assert_eq!(jobs[2].flags & frag_flags::SUBMIT_PARTIAL_RENDER, 0);
    }
}

/// The fragment programs of `resources/gpu/programs` (TASK-0215 5c-1): the open compiler's
/// code for the board's core, by size and digest — and the base program is the render
/// bundle's own, so the cost rung's base probe runs the pass every earlier rung ran.
mod programs {
    use nexus_gfx::backend::gpu::scene::{fnv1a64, Scene};
    use nexus_gfx::backend::gpu::CoreId;

    const RENDER: &[u8] = include_bytes!("../../../resources/gpu/scenes/render-quad.scene");
    const BASE: &[u8] = include_bytes!("../../../resources/gpu/programs/frag-base.usc");
    const PROGRAMS: [(&str, &[u8], u64); 5] = [
        ("frag-base", BASE, 0x0fa6_5ca1_5b93_7497),
        (
            "frag-chain",
            include_bytes!("../../../resources/gpu/programs/frag-chain.usc"),
            0x82b8_ca6f_fbb4_3157,
        ),
        (
            "frag-eft",
            include_bytes!("../../../resources/gpu/programs/frag-eft.usc"),
            0x0c47_42c8_9ca4_bf25,
        ),
        (
            "frag-opaque-eft",
            include_bytes!("../../../resources/gpu/programs/frag-opaque-eft.usc"),
            0x8476_199e_b9c2_813d,
        ),
        (
            "frag-ops",
            include_bytes!("../../../resources/gpu/programs/frag-ops.usc"),
            0x55cb_d264_d929_bcab,
        ),
    ];

    #[test]
    fn the_programs_are_their_captures() {
        let sizes: Vec<(&str, usize, u64)> =
            PROGRAMS.iter().map(|(n, b, _)| (*n, b.len(), fnv1a64(b))).collect();
        let want: Vec<(&str, usize, u64)> = PROGRAMS
            .iter()
            .zip([736, 1312, 728, 608, 624])
            .map(|((n, _, d), len)| (*n, len, *d))
            .collect();
        assert_eq!(sizes, want, "recaptured: update the digests and the README");
    }

    #[test]
    fn the_base_program_is_the_render_bundles_own() {
        let scene = Scene::parse(RENDER, CoreId::BOARD).expect("the render bundle parses");
        let arena = scene
            .buffers()
            .find(|b| b.va == 0xe0_0000_1000)
            .expect("the code arena at the code heap's second page");
        let mut bytes = vec![0u8; arena.len as usize];
        for run in arena.runs() {
            bytes[run.off as usize..run.off as usize + run.bytes.len()].copy_from_slice(run.bytes);
        }
        // The fragment kick word 0x0000_0028_0000_7b80: the code heap's offset 0x7b80.
        assert_eq!(&bytes[0x6b80..0x6b80 + BASE.len()], BASE);
        assert!(
            bytes[0x8000..].iter().all(|&b| b == 0),
            "the arena's tail from 0x8000 is free for the probes' programs"
        );
    }
}
