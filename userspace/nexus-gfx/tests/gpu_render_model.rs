// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The render model's tests (TASK-0215 5a, RFC-0101 §8d): the geometry and fragment commands
//! decoded from streams in the open driver's order and refused at the wrong length, the static
//! render-context state, the render-target dataset's words for a 1080p plane by the open kernel
//! driver's formulas, the free lists' initialisation and checks.

mod render {
    use nexus_gfx::backend::gpu::fwif::*;

    fn geom_stream() -> Vec<u8> {
        let mut s = Vec::new();
        s.extend_from_slice(&40u32.to_le_bytes());
        s.extend_from_slice(&0u32.to_le_bytes());
        s.extend_from_slice(&0x80_0008_da00u64.to_le_bytes()); // vdm_ctrl_stream_base
        s.extend_from_slice(&0x80_0007_0000u64.to_le_bytes()); // tpu_border_colour_table
        for w in [0x1234u32, 0x5678, 0x9abc, 0] {
            s.extend_from_slice(&w.to_le_bytes());
        }
        s
    }

    fn frag_stream() -> Vec<u8> {
        let mut s = Vec::new();
        s.extend_from_slice(&384u32.to_le_bytes());
        s.extend_from_slice(&0u32.to_le_bytes());
        for i in 0..6u64 {
            s.extend_from_slice(&(0x1000 + i).to_le_bytes());
        }
        for i in 0..24u64 {
            s.extend_from_slice(&(0x2000 + i).to_le_bytes());
        }
        s.extend_from_slice(&0x3000u64.to_le_bytes());
        for i in 0..6u64 {
            s.extend_from_slice(&(0x4000 + i).to_le_bytes());
        }
        for i in 0..8u32 {
            s.extend_from_slice(&(0x5000 + i).to_le_bytes());
        }
        for i in 0..12u32 {
            s.extend_from_slice(&(0x6000 + i).to_le_bytes());
        }
        assert_eq!(s.len(), 384);
        s
    }

    #[test]
    fn stream_lengths_are_the_captured_ones() {
        assert_eq!(RenderFeatures::BOARD.geom_stream_len(), 40);
        assert_eq!(RenderFeatures::BOARD.frag_stream_len(), 384);
        let all = RenderFeatures {
            vdm_drawindirect: true,
            vdm_object_level_lls: true,
            tessellation: true,
            requires_fb_cdc_zls_setup: true,
            s7_top_infrastructure: true,
            cluster_grouping: true,
            gpu_multicore_support: true,
            zls_subtile: true,
            isp_zls_d24_s8_packing_ogl_mode: true,
        };
        assert_eq!(all.geom_stream_len(), 60);
        assert_eq!(all.frag_stream_len(), 384 + 8 + 4 * 4);
        assert_eq!(StaticRenderContextState::STREAM_LEN, 192);
    }

    #[test]
    fn decodes_a_geometry_stream_in_the_open_drivers_order() {
        let pr = Ufo { addr: 0xc000_1234, value: 6 };
        let cmd = GeomCmd::from_stream(&geom_stream(), RenderFeatures::BOARD, 0x3, 0xc000_0100, pr)
            .unwrap();
        assert_eq!(cmd.regs.vdm_ctrl_stream_base, 0x80_0008_da00);
        assert_eq!(cmd.regs.tpu_border_colour_table, 0x80_0007_0000);
        assert_eq!((cmd.regs.ppp_ctrl, cmd.regs.te_psg), (0x1234, 0x5678));
        assert_eq!(cmd.regs.vdm_context_resume_task0_size, 0x9abc);
        assert_eq!(cmd.regs.view_idx, 0);
        assert_eq!(cmd.flags, geom_flags::FIRSTKICK | geom_flags::LASTKICK);
        assert_eq!(cmd.shared.hwrt_data_fw_addr, 0xc000_0100);
        let b = cmd.to_bytes();
        assert_eq!(&b[4..8], &0xc000_0100u32.to_le_bytes());
        assert_eq!(&b[16..24], &0x80_0008_da00u64.to_le_bytes());
        assert_eq!(&b[80..84], &3u32.to_le_bytes());
        assert_eq!(&b[84..92], &[0x34, 0x12, 0x00, 0xc0, 6, 0, 0, 0]);
    }

    #[test]
    fn decodes_a_fragment_stream_in_the_open_drivers_order() {
        let cmd =
            FragCmd::from_stream(&frag_stream(), RenderFeatures::BOARD, 0x80, 0xc000_0100).unwrap();
        assert_eq!(cmd.regs.isp_scissor_base, 0x1000);
        assert_eq!(cmd.regs.isp_stencil_load_store_base, 0x1005);
        assert_eq!(cmd.regs.fb_cdc_zls, 0, "not carried without the feature");
        assert_eq!(cmd.regs.pbe_word[0], 0x2000);
        assert_eq!(cmd.regs.pbe_word[23], 0x2017);
        assert_eq!(cmd.regs.tpu_border_colour_table, 0x3000);
        assert_eq!(cmd.regs.pds_bgnd, [0x4000, 0x4001, 0x4002]);
        assert_eq!(cmd.regs.pds_pr_bgnd, [0x4003, 0x4004, 0x4005]);
        assert_eq!(cmd.regs.usc_clear_register[7], 0x5007);
        assert_eq!(cmd.regs.usc_pixel_output_ctrl, 0x6000);
        assert_eq!(cmd.regs.isp_aa, 0x6003);
        assert_eq!(cmd.regs.isp_ctl, 0x6004, "no xtp word on this core");
        assert_eq!(cmd.regs.view_idx, 0x6006, "no pixel_phantom on this core");
        assert_eq!(cmd.regs.isp_oclqry_stride, 0x6008, "multicore");
        assert_eq!((cmd.zls_stride, cmd.sls_stride, cmd.execute_count), (0x6009, 0x600a, 0x600b));
        assert_eq!(cmd.flags, frag_flags::DISABLE_PIXELMERGE);
        let b = cmd.to_bytes();
        assert_eq!(&b[16 + 96..16 + 104], &0x1000u64.to_le_bytes(), "isp_scissor_base");
        assert_eq!(&b[464..468], &frag_flags::DISABLE_PIXELMERGE.to_le_bytes());
        assert_eq!(&b[476..480], &0x600bu32.to_le_bytes());
    }

    #[test]
    fn partial_render_is_a_command_type_not_a_flag() {
        let cmd = FragCmd::from_stream(&frag_stream(), RenderFeatures::BOARD, 0xc0, 1).unwrap();
        assert_eq!(cmd.flags, frag_flags::DISABLE_PIXELMERGE);
        assert_eq!(
            frag_flags::from_submit(0xff),
            frag_flags::SINGLE_CORE
                | frag_flags::DEPTHBUFFER
                | frag_flags::STENCILBUFFER
                | frag_flags::PREVENT_CDM_OVERLAP
                | frag_flags::SCRATCHBUFFER
                | frag_flags::GET_VIS_RESULTS
                | frag_flags::DISABLE_PIXELMERGE
        );
        assert_eq!(
            geom_flags::from_submit(0x7),
            geom_flags::FIRSTKICK | geom_flags::LASTKICK | geom_flags::SINGLE_CORE
        );
    }

    #[test]
    fn test_reject_geom_stream_length() {
        let s = geom_stream();
        let all = RenderFeatures { vdm_drawindirect: true, ..RenderFeatures::BOARD };
        assert_eq!(
            GeomCmd::from_stream(&s, all, 0, 0, Ufo { addr: 0, value: 0 }),
            Err(StreamError::Length { expected: 52, got: 40 })
        );
        let mut ext = s.clone();
        ext.extend_from_slice(&[0u8; 8]);
        assert_eq!(
            GeomCmd::from_stream(&ext, RenderFeatures::BOARD, 0, 0, Ufo { addr: 0, value: 0 }),
            Err(StreamError::Extension(8))
        );
    }

    #[test]
    fn test_reject_frag_stream_length() {
        let s = frag_stream();
        let no_mc = RenderFeatures { gpu_multicore_support: false, ..RenderFeatures::BOARD };
        assert_eq!(
            FragCmd::from_stream(&s, no_mc, 0, 0),
            Err(StreamError::Length { expected: 376, got: 384 })
        );
        let mut bad = s.clone();
        bad[4] = 1;
        assert_eq!(
            FragCmd::from_stream(&bad, RenderFeatures::BOARD, 0, 0),
            Err(StreamError::Padding(1))
        );
    }

    #[test]
    fn test_reject_static_state_length() {
        let mut s = Vec::new();
        s.extend_from_slice(&192u32.to_le_bytes());
        s.extend_from_slice(&0u32.to_le_bytes());
        for i in 0..23u64 {
            s.extend_from_slice(&(0x100 + i).to_le_bytes());
        }
        let st = StaticRenderContextState::from_stream(&s).unwrap();
        assert_eq!(st.regs[0], 0x100);
        assert_eq!(st.regs[22], 0x116);
        let fw = st.to_fw_bytes();
        assert_eq!(fw.len(), 368);
        assert_eq!(&fw[0..8], &0x100u64.to_le_bytes());
        assert!(fw[184..].iter().all(|&b| b == 0), "the second core's set stays zero");
        let mut short = s.clone();
        short[0..4].copy_from_slice(&184u32.to_le_bytes());
        assert_eq!(
            StaticRenderContextState::from_stream(&short),
            Err(StreamError::Length { expected: 192, got: 184 })
        );
    }
}

mod hwrt {
    use nexus_gfx::backend::gpu::fwif::*;

    fn put32(b: &mut [u8], off: usize, v: u32) {
        b[off..off + 4].copy_from_slice(&v.to_le_bytes());
    }
    fn put64(b: &mut [u8], off: usize, v: u64) {
        b[off..off + 8].copy_from_slice(&v.to_le_bytes());
    }

    /// The captured arguments of 2026-10-13 (`hwrt1.args`).
    fn captured() -> HwrtArgs {
        HwrtArgs {
            tpc_dev_addr: 0x80_010c_7000,
            tpc_stride: 32768,
            tpc_size: 0,
            vheap_table_dev_addr: 0x80_010c_6000,
            rtc_dev_addr: 0,
            rt_data: [
                RtDataArgs {
                    pm_mlist_dev_addr: 0x80_010c_f000,
                    macrotile_array_dev_addr: 0,
                    region_header_dev_addr: 0xdb_f000_0000,
                },
                RtDataArgs {
                    pm_mlist_dev_addr: 0x80_010d_0000,
                    macrotile_array_dev_addr: 0,
                    region_header_dev_addr: 0xdb_f000_32c0,
                },
            ],
            free_list_handles: [2, 1],
            width: 1920,
            height: 1080,
            samples: 1,
            layers: 1,
            region_header_size: 2160,
            ..HwrtArgs::default()
        }
    }

    fn captured_bytes() -> [u8; HwrtArgs::SIZE] {
        let a = captured();
        let mut b = [0u8; HwrtArgs::SIZE];
        put64(&mut b, 0, a.tpc_dev_addr);
        put32(&mut b, 8, a.tpc_stride);
        put32(&mut b, 12, a.tpc_size);
        put64(&mut b, 16, a.vheap_table_dev_addr);
        put64(&mut b, 24, a.rtc_dev_addr);
        for (i, rt) in a.rt_data.iter().enumerate() {
            put64(&mut b, 32 + i * 24, rt.pm_mlist_dev_addr);
            put64(&mut b, 40 + i * 24, rt.macrotile_array_dev_addr);
            put64(&mut b, 48 + i * 24, rt.region_header_dev_addr);
        }
        put32(&mut b, 80, 2);
        put32(&mut b, 84, 1);
        put32(&mut b, 88, 1920);
        put32(&mut b, 92, 1080);
        put32(&mut b, 96, 1);
        put32(&mut b, 100, 1);
        put32(&mut b, 128, 2160);
        b
    }

    #[test]
    fn parses_the_captured_arguments() {
        assert_eq!(HwrtArgs::parse(&captured_bytes()).unwrap(), captured());
        assert_eq!(HwrtArgs::parse(&captured_bytes()[..100]), Err(HwrtError::Short));
        let mut b = captured_bytes();
        put32(&mut b, 96, 4);
        assert_eq!(HwrtArgs::parse(&b), Err(HwrtError::Unsupported { samples: 4, layers: 1 }));
        let mut b = captured_bytes();
        put32(&mut b, 88, 0);
        assert_eq!(HwrtArgs::parse(&b), Err(HwrtError::Size { width: 0, height: 1080 }));
    }

    #[test]
    fn the_common_block_is_the_kernels_for_a_1080p_plane() {
        let c = HwrtDataCommon::new(&captured(), TileGeometry::BOARD).unwrap();
        // 120 × 68 tiles of 16; parameter format 2: 30 × 18 macrotiles; tile max 119 × 67.
        assert_eq!(c.te_screen, 119 | (67 << 12));
        assert_eq!(c.te_mtile1, 30 << 18);
        assert_eq!(c.te_mtile2, 18 << 18);
        assert_eq!(c.mtile_stride, 540);
        assert_eq!(c.isp_mtile_size, (30 << 16) | 18);
        assert_eq!(c.screen_pixel_max, 1919 | (1079 << 16));
        assert_eq!(c.multi_sample_ctl, 0x88);
        assert_eq!(c.flipped_multi_sample_ctl, 0x88);
        assert_eq!(c.teaa, 0);
        assert_eq!((c.tpc_stride, c.rgn_header_size), (32768, 2160));
        let b = c.to_bytes();
        assert_eq!(&b[32..36], &c.te_screen.to_le_bytes());
        assert_eq!(&b[80..84], &c.isp_mtile_size.to_le_bytes());
    }

    #[test]
    fn multisample_words_follow_the_kernels_table() {
        assert_eq!(multi_sample_ctl(2, false), Some(0x44cc));
        assert_eq!(multi_sample_ctl(2, true), Some(0xc44c));
        assert_eq!(multi_sample_ctl(3, false), None);
        assert_eq!(te_aa(4, 1), Some(0x6));
        assert_eq!(te_aa(2, 2), Some(0x1));
        assert_eq!(te_aa(1, 8), None);
    }

    #[test]
    fn the_data_carries_the_clients_addresses_and_the_lists() {
        let d = HwrtData::new(&captured(), 1, 0xc000_1000, [0xc000_2000, 0xc000_3000]);
        assert_eq!(d.pm_mlist_dev_addr, 0x80_010d_0000);
        assert_eq!(d.rgn_header_dev_addr, 0xdb_f000_32c0);
        assert_eq!(d.tail_ptrs_dev_addr, 0x80_010c_7000);
        assert_eq!(d.freelists_fw_addr, [0xc000_2000, 0xc000_3000, 0]);
        assert_eq!(d.rta_ctl.max_rts, 1);
        assert_eq!(d.state, 0);
        let b = d.to_bytes();
        assert_eq!(&b[164..168], &0xc000_1000u32.to_le_bytes());
        assert_eq!(&b[256..264], &0xdb_f000_32c0u64.to_le_bytes());
        assert!(b[280..320].iter().all(|&x| x == 0));
        assert_eq!(&b[320..328], &[0u8; 8]);
    }

    #[test]
    fn free_lists_are_initialised_as_the_kernel_does() {
        let local = FreelistArgs {
            gpu_addr: 0x80_010c_5000,
            initial_pages: 32,
            max_pages: 32,
            grow_pages: 0,
            grow_threshold_percent: 0,
        };
        let global = FreelistArgs {
            gpu_addr: 0x80_0000_9000,
            initial_pages: 512,
            max_pages: 65536,
            grow_pages: 256,
            grow_threshold_percent: 13,
        };
        assert_eq!(local.check(25), Ok(()));
        assert_eq!(global.check(25), Ok(()));
        let l = Freelist::new(&local, 1);
        assert_eq!((l.current_pages, l.ready_pages, l.current_stack_top), (32, 0, 31));
        assert_eq!(l.current_dev_addr, 0x80_010c_5000);
        assert_eq!(local.first_entry(), 0);
        let g = Freelist::new(&global, 2);
        assert_eq!(g.ready_pages, 64, "13 % of 512 is 66, rounded down to the alignment");
        assert_eq!((g.current_pages, g.current_stack_top), (448, 447));
        assert_eq!(g.current_dev_addr, 0x80_0000_9000 + (65536 - 448) * 4);
        assert_eq!(global.first_entry(), 65024);
        assert_eq!(global.list_bytes(), 256 * 1024);
        let b = g.to_bytes();
        assert_eq!(&b[16..20], &447u32.to_le_bytes());
        assert_eq!(&b[40..44], &2u32.to_le_bytes());
        assert_eq!(page_entry(0x1_2345_6000), 0x12345_6);
    }

    /// After the firmware's recovery a list is the list as made — whatever the firmware's
    /// words said — and the render-target data on it is in the recovery's state.
    #[test]
    fn a_rebuilt_list_is_the_list_as_made() {
        let global = FreelistArgs {
            gpu_addr: 0x80_0000_9000,
            initial_pages: 512,
            max_pages: 65536,
            grow_pages: 256,
            grow_threshold_percent: 13,
        };
        let made = Freelist::new(&global, 0x205);
        let mut bytes = made.to_bytes();
        // The firmware took pages, used the ready ones, counted them; the reset cut it off.
        for (at, word) in [(8, 0x0bad_f00du32), (16, 120), (28, 512), (32, 328), (36, 5), (48, 0)] {
            bytes[at..at + 4].copy_from_slice(&word.to_le_bytes());
        }
        for (at, word) in Freelist::rebuilt(&global) {
            bytes[at..at + 4].copy_from_slice(&word.to_le_bytes());
        }
        assert_eq!(bytes, made.to_bytes(), "the struct as made, the id and every other word kept");
        // The data on it: the recovery's state, the last-geometry flag cleared, others kept.
        let flags = HWRTDATA_HAS_LAST_GEOM | 1;
        assert_eq!(
            HwrtData::recovered(flags),
            [(HwrtData::FLAGS_OFFSET, 1), (HwrtData::STATE_OFFSET, RTDATA_STATE_HWR)]
        );
    }

    #[test]
    fn test_reject_freelist_bounds() {
        let a = |initial, max, grow| FreelistArgs {
            gpu_addr: 0,
            initial_pages: initial,
            max_pages: max,
            grow_pages: grow,
            grow_threshold_percent: 10,
        };
        assert_eq!(a(64, 32, 0).check(25), Err(FreelistError::Bounds));
        assert_eq!(a(32, 64, 0).check(25), Err(FreelistError::Bounds), "growth needed, none given");
        assert_eq!(a(32, 32, 4).check(25), Err(FreelistError::Bounds), "full, yet growth given");
        assert_eq!(a(30, 64, 4).check(25), Err(FreelistError::Alignment));
        assert_eq!(a(16, 64, 4).check(25), Err(FreelistError::BelowMinimum { min: 25 }));
    }
}
