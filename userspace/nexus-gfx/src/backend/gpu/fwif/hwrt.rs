// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The kernel side's render objects (TASK-0215 5a, RFC-0101 §8d): the render-target dataset
//! — its common block (the tile geometry of the attachment, the open kernel driver's formulas)
//! and its per-frame data (the client's addresses, the free lists, the render-target-array
//! control) — and the free list of parameter memory (the firmware struct the open kernel driver
//! initialises — in [`super::freelist`]). The open userspace driver passes the dataset's
//! arguments as one 136-byte block; `HwrtArgs::parse` reads it. Part of [`super`] (`fwif`); the
//! offsets are asserted against the open kernel driver's layout.

use core::mem::{offset_of, size_of};

use super::{put32, put64};
use crate::backend::gpu::fw_features::{DeviceInfoBlock, Feature};

/// What the core's tiler needs to know about itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TileGeometry {
    pub tile_x: u32,
    pub tile_y: u32,
    /// Parameter format version 2: 16 macrotiles of 2×2-tile-aligned groups.
    pub parameter_format_v2: bool,
    pub isp_samples_per_pixel: u32,
}

impl TileGeometry {
    /// The board's core (its device-info block: 16×16 tiles, format 2, one sample a pixel).
    pub const BOARD: Self =
        Self { tile_x: 16, tile_y: 16, parameter_format_v2: true, isp_samples_per_pixel: 1 };

    /// From the firmware's device-info block; `None` when a value is missing.
    pub fn of(info: &DeviceInfoBlock<'_>) -> Option<Self> {
        let small = |f: Feature| info.value(f).and_then(|v| u32::try_from(v).ok());
        Some(Self {
            tile_x: small(Feature::TileSizeX)?,
            tile_y: small(Feature::TileSizeY)?,
            parameter_format_v2: small(Feature::SimpleParameterFormatVersion) == Some(2),
            isp_samples_per_pixel: small(Feature::IspSamplesPerPixel)?,
        })
    }
}

/// One render-target data's addresses (the client's memory).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RtDataArgs {
    pub pm_mlist_dev_addr: u64,
    pub macrotile_array_dev_addr: u64,
    pub region_header_dev_addr: u64,
}

/// The dataset's arguments as the open userspace driver passes them (136 bytes).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HwrtArgs {
    pub tpc_dev_addr: u64,
    pub tpc_stride: u32,
    pub tpc_size: u32,
    pub vheap_table_dev_addr: u64,
    pub rtc_dev_addr: u64,
    pub rt_data: [RtDataArgs; 2],
    /// The open driver's handles: `[local, global]`.
    pub free_list_handles: [u32; 2],
    pub width: u32,
    pub height: u32,
    pub samples: u32,
    pub layers: u32,
    pub isp_merge_lower_x: u32,
    pub isp_merge_lower_y: u32,
    pub isp_merge_scale_x: u32,
    pub isp_merge_scale_y: u32,
    pub isp_merge_upper_x: u32,
    pub isp_merge_upper_y: u32,
    pub region_header_size: u32,
}

/// Why the arguments were refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HwrtError {
    Short,
    /// Not one sample, one layer: this layer renders one plane.
    Unsupported {
        samples: u32,
        layers: u32,
    },
    /// A zero or oversized attachment.
    Size {
        width: u32,
        height: u32,
    },
}

fn get32(b: &[u8], off: usize) -> u32 {
    let mut w = [0u8; 4];
    w.copy_from_slice(&b[off..off + 4]);
    u32::from_le_bytes(w)
}

fn get64(b: &[u8], off: usize) -> u64 {
    let mut w = [0u8; 8];
    w.copy_from_slice(&b[off..off + 8]);
    u64::from_le_bytes(w)
}

impl HwrtArgs {
    pub const SIZE: usize = 136;
    /// The largest attachment this layer binds: a plane of the ring.
    pub const MAX_SIDE: u32 = 4096;

    pub fn parse(b: &[u8]) -> Result<Self, HwrtError> {
        if b.len() < Self::SIZE {
            return Err(HwrtError::Short);
        }
        let rt = |o: usize| RtDataArgs {
            pm_mlist_dev_addr: get64(b, o),
            macrotile_array_dev_addr: get64(b, o + 8),
            region_header_dev_addr: get64(b, o + 16),
        };
        let a = Self {
            tpc_dev_addr: get64(b, 0),
            tpc_stride: get32(b, 8),
            tpc_size: get32(b, 12),
            vheap_table_dev_addr: get64(b, 16),
            rtc_dev_addr: get64(b, 24),
            rt_data: [rt(32), rt(56)],
            free_list_handles: [get32(b, 80), get32(b, 84)],
            width: get32(b, 88),
            height: get32(b, 92),
            samples: get32(b, 96),
            layers: get32(b, 100),
            isp_merge_lower_x: get32(b, 104),
            isp_merge_lower_y: get32(b, 108),
            isp_merge_scale_x: get32(b, 112),
            isp_merge_scale_y: get32(b, 116),
            isp_merge_upper_x: get32(b, 120),
            isp_merge_upper_y: get32(b, 124),
            region_header_size: get32(b, 128),
        };
        if a.samples != 1 || a.layers != 1 {
            return Err(HwrtError::Unsupported { samples: a.samples, layers: a.layers });
        }
        if a.width == 0 || a.height == 0 || a.width > Self::MAX_SIDE || a.height > Self::MAX_SIDE {
            return Err(HwrtError::Size { width: a.width, height: a.height });
        }
        Ok(a)
    }
}

/// The dataset's common block: the attachment's tile geometry for the firmware.
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HwrtDataCommon {
    pub geom_caches_need_zeroing: u32,
    pub screen_pixel_max: u32,
    pub multi_sample_ctl: u64,
    pub flipped_multi_sample_ctl: u64,
    pub tpc_stride: u32,
    pub tpc_size: u32,
    pub te_screen: u32,
    pub mtile_stride: u32,
    pub teaa: u32,
    pub te_mtile1: u32,
    pub te_mtile2: u32,
    pub isp_merge_lower_x: u32,
    pub isp_merge_lower_y: u32,
    pub isp_merge_upper_x: u32,
    pub isp_merge_upper_y: u32,
    pub isp_merge_scale_x: u32,
    pub isp_merge_scale_y: u32,
    pub rgn_header_size: u32,
    pub isp_mtile_size: u32,
    pub padding: u32,
}

/// The sample positions the open kernel driver programs, by sample count (1, 2, 4, 8).
const SAMPLE_POSITIONS: [([u8; 8], [u8; 8]); 4] = [
    ([8, 0, 0, 0, 0, 0, 0, 0], [8, 0, 0, 0, 0, 0, 0, 0]),
    ([12, 4, 0, 0, 0, 0, 0, 0], [12, 4, 0, 0, 0, 0, 0, 0]),
    ([6, 14, 2, 10, 0, 0, 0, 0], [2, 6, 10, 14, 0, 0, 0, 0]),
    ([9, 7, 13, 5, 3, 1, 11, 15], [5, 11, 9, 3, 13, 7, 15, 1]),
];

/// `MULTISAMPLECTL` for `samples` (1, 2, 4 or 8), the y positions flipped when asked.
pub fn multi_sample_ctl(samples: u32, y_flip: bool) -> Option<u64> {
    let idx = match samples {
        1 => 0,
        2 => 1,
        4 => 2,
        8 => 3,
        _ => return None,
    };
    let (xs, ys) = SAMPLE_POSITIONS[idx];
    let mut v = 0u64;
    for i in 0..8 {
        v |= u64::from(xs[i]) << (i * 8);
        let y = if y_flip { (16 - u64::from(ys[i])) & 0xf } else { u64::from(ys[i]) };
        v |= y << (i * 8 + 4);
    }
    Some(v)
}

/// `TE_AA` for `samples` on a core with `spp` samples a pixel.
pub fn te_aa(samples: u32, spp: u32) -> Option<u32> {
    const X2: u32 = 1 << 0;
    const X: u32 = 1 << 1;
    const Y: u32 = 1 << 2;
    const Y2: u32 = 1 << 3;
    let mut v = 0;
    match spp {
        1 => {
            if samples >= 2 {
                v |= Y;
            }
            if samples >= 4 {
                v |= X;
            }
        }
        2 => {
            if samples >= 2 {
                v |= X2;
            }
            if samples >= 4 {
                v |= Y;
            }
            if samples >= 8 {
                v |= X;
            }
        }
        4 => {
            if samples >= 2 {
                v |= X2;
            }
            if samples >= 4 {
                v |= Y2;
            }
            if samples >= 8 {
                v |= Y;
            }
        }
        _ => return None,
    }
    Some(v)
}

impl HwrtDataCommon {
    pub const SIZE: usize = 88;

    /// The open kernel driver's `hwrt_init_common_fw_structure` for `args` on a core of `geo`.
    pub fn new(args: &HwrtArgs, geo: TileGeometry) -> Option<Self> {
        let num_tiles_x = args.width.div_ceil(geo.tile_x);
        let num_tiles_y = args.height.div_ceil(geo.tile_y);
        let (mx, my, tile_max_x, tile_max_y) = if geo.parameter_format_v2 {
            let mx0 = num_tiles_x.div_ceil(8) * 2;
            let my0 = num_tiles_y.div_ceil(8) * 2;
            (
                [mx0, 0, 0],
                [my0, 0, 0],
                num_tiles_x.next_multiple_of(2) - 1,
                num_tiles_y.next_multiple_of(2) - 1,
            )
        } else {
            let mx0 = num_tiles_x.div_ceil(4).next_multiple_of(4);
            let my0 = num_tiles_y.div_ceil(4).next_multiple_of(4);
            ([mx0, mx0 * 2, mx0 * 3], [my0, my0 * 2, my0 * 3], num_tiles_x - 1, num_tiles_y - 1)
        };
        let (mut sx, mut sy) = (mx[0], my[0]);
        match geo.isp_samples_per_pixel {
            1 => {
                if args.samples >= 4 {
                    sx <<= 1;
                }
                if args.samples >= 2 {
                    sy <<= 1;
                }
            }
            2 => {
                if args.samples >= 8 {
                    sx <<= 1;
                }
                if args.samples >= 4 {
                    sy <<= 1;
                }
            }
            4 => {
                if args.samples >= 8 {
                    sy <<= 1;
                }
            }
            _ => return None,
        }
        Some(Self {
            geom_caches_need_zeroing: 0,
            screen_pixel_max: ((args.width - 1) & 0x7fff) | (((args.height - 1) & 0x7fff) << 16),
            multi_sample_ctl: multi_sample_ctl(args.samples, false)?,
            flipped_multi_sample_ctl: multi_sample_ctl(args.samples, true)?,
            tpc_stride: args.tpc_stride,
            tpc_size: args.tpc_size,
            te_screen: (tile_max_x & 0x1ff) | ((tile_max_y & 0x1ff) << 12),
            mtile_stride: mx[0] * my[0],
            teaa: te_aa(args.samples, geo.isp_samples_per_pixel)?,
            te_mtile1: ((mx[0] & 0x1ff) << 18) | ((mx[1] & 0x1ff) << 9) | (mx[2] & 0x1ff),
            te_mtile2: ((my[0] & 0x1ff) << 18) | ((my[1] & 0x1ff) << 9) | (my[2] & 0x1ff),
            isp_merge_lower_x: args.isp_merge_lower_x,
            isp_merge_lower_y: args.isp_merge_lower_y,
            isp_merge_upper_x: args.isp_merge_upper_x,
            isp_merge_upper_y: args.isp_merge_upper_y,
            isp_merge_scale_x: args.isp_merge_scale_x,
            isp_merge_scale_y: args.isp_merge_scale_y,
            rgn_header_size: args.region_header_size,
            isp_mtile_size: ((sx & 0x3ff) << 16) | (sy & 0x3ff),
            padding: 0,
        })
    }

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut b = [0u8; Self::SIZE];
        put32(&mut b, 0, self.geom_caches_need_zeroing);
        put32(&mut b, 4, self.screen_pixel_max);
        put64(&mut b, 8, self.multi_sample_ctl);
        put64(&mut b, 16, self.flipped_multi_sample_ctl);
        for (i, v) in [
            self.tpc_stride,
            self.tpc_size,
            self.te_screen,
            self.mtile_stride,
            self.teaa,
            self.te_mtile1,
            self.te_mtile2,
            self.isp_merge_lower_x,
            self.isp_merge_lower_y,
            self.isp_merge_upper_x,
            self.isp_merge_upper_y,
            self.isp_merge_scale_x,
            self.isp_merge_scale_y,
            self.rgn_header_size,
            self.isp_mtile_size,
        ]
        .iter()
        .enumerate()
        {
            put32(&mut b, 24 + i * 4, *v);
        }
        b
    }
}

/// The render-target-array control inside the data.
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RtaCtl {
    pub render_target_index: u32,
    pub current_render_target: u32,
    pub active_render_targets: u32,
    pub cumul_active_render_targets: u32,
    pub valid_render_targets_fw_addr: u32,
    pub rta_num_partial_renders_fw_addr: u32,
    pub max_rts: u32,
    pub rta_ctl_flags: u32,
}

/// The firmware's cleanup control, on its own cache line.
#[repr(C, align(64))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CleanupCtl {
    pub submitted_commands: u32,
    pub executed_commands: u32,
}

/// One render-target data: the firmware's per-frame view of the dataset.
#[repr(C, align(64))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HwrtData {
    pub pm_mlist_dev_addr: u64,
    pub vce_cat_base: [u64; 4],
    pub vce_last_cat_base: [u64; 4],
    pub te_cat_base: [u64; 4],
    pub te_last_cat_base: [u64; 4],
    pub alist_cat_base: u64,
    pub alist_last_cat_base: u64,
    pub pm_alist_stack_pointer: u64,
    pub pm_mlist_stack_pointer: u32,
    pub hwrt_data_common_fw_addr: u32,
    pub hwrt_data_flags: u32,
    /// `ROGUE_FWIF_RTDATA_STATE_NONE` at creation.
    pub state: u32,
    pub freelists_fw_addr: [u32; 3],
    pub freelist_hwr_snapshot: [u32; 3],
    pub vheap_table_dev_addr: u64,
    pub rta_ctl: RtaCtl,
    pub tail_ptrs_dev_addr: u64,
    pub macrotile_array_dev_addr: u64,
    pub rgn_header_dev_addr: u64,
    pub rtc_dev_addr: u64,
    pub owner_geom_not_used_by_host: u32,
    pub geom_caches_need_zeroing: u32,
    pub padding0: [u8; 32],
    pub padding1: [u8; 8],
    pub cleanup_state: CleanupCtl,
}

impl HwrtData {
    pub const SIZE: usize = 384;
    /// The cleanup control's offset: every kick of a job on this data names it.
    pub const CLEANUP_OFFSET: usize = 320;

    /// The open kernel driver's `hwrt_data_init_fw_structure` for render-target data `index`
    /// of `args`: `common` is the common block's firmware address, `freelists` the local and
    /// global lists' firmware addresses.
    pub fn new(args: &HwrtArgs, index: usize, common: u32, freelists: [u32; 2]) -> Self {
        let rt = args.rt_data[index.min(1)];
        Self {
            pm_mlist_dev_addr: rt.pm_mlist_dev_addr,
            hwrt_data_common_fw_addr: common,
            freelists_fw_addr: [freelists[0], freelists[1], 0],
            vheap_table_dev_addr: args.vheap_table_dev_addr,
            rta_ctl: RtaCtl { max_rts: args.layers, ..RtaCtl::default() },
            tail_ptrs_dev_addr: args.tpc_dev_addr,
            macrotile_array_dev_addr: rt.macrotile_array_dev_addr,
            rgn_header_dev_addr: rt.region_header_dev_addr,
            rtc_dev_addr: args.rtc_dev_addr,
            ..Self::default()
        }
    }

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut b = [0u8; Self::SIZE];
        put64(&mut b, 0, self.pm_mlist_dev_addr);
        for (i, v) in self
            .vce_cat_base
            .iter()
            .chain(&self.vce_last_cat_base)
            .chain(&self.te_cat_base)
            .chain(&self.te_last_cat_base)
            .enumerate()
        {
            put64(&mut b, 8 + i * 8, *v);
        }
        put64(&mut b, 136, self.alist_cat_base);
        put64(&mut b, 144, self.alist_last_cat_base);
        put64(&mut b, 152, self.pm_alist_stack_pointer);
        put32(&mut b, 160, self.pm_mlist_stack_pointer);
        put32(&mut b, 164, self.hwrt_data_common_fw_addr);
        put32(&mut b, 168, self.hwrt_data_flags);
        put32(&mut b, 172, self.state);
        for (i, v) in self.freelists_fw_addr.iter().enumerate() {
            put32(&mut b, 176 + i * 4, *v);
        }
        for (i, v) in self.freelist_hwr_snapshot.iter().enumerate() {
            put32(&mut b, 188 + i * 4, *v);
        }
        put64(&mut b, 200, self.vheap_table_dev_addr);
        let r = offset_of!(HwrtData, rta_ctl);
        for (i, v) in [
            self.rta_ctl.render_target_index,
            self.rta_ctl.current_render_target,
            self.rta_ctl.active_render_targets,
            self.rta_ctl.cumul_active_render_targets,
            self.rta_ctl.valid_render_targets_fw_addr,
            self.rta_ctl.rta_num_partial_renders_fw_addr,
            self.rta_ctl.max_rts,
            self.rta_ctl.rta_ctl_flags,
        ]
        .iter()
        .enumerate()
        {
            put32(&mut b, r + i * 4, *v);
        }
        put64(&mut b, 240, self.tail_ptrs_dev_addr);
        put64(&mut b, 248, self.macrotile_array_dev_addr);
        put64(&mut b, 256, self.rgn_header_dev_addr);
        put64(&mut b, 264, self.rtc_dev_addr);
        put32(&mut b, 272, self.owner_geom_not_used_by_host);
        put32(&mut b, 276, self.geom_caches_need_zeroing);
        let c = offset_of!(HwrtData, cleanup_state);
        put32(&mut b, c, self.cleanup_state.submitted_commands);
        put32(&mut b, c + 4, self.cleanup_state.executed_commands);
        b
    }
}

// The open kernel driver's layouts, pinned.
const _: () = assert!(size_of::<HwrtDataCommon>() == HwrtDataCommon::SIZE);
const _: () = assert!(offset_of!(HwrtDataCommon, isp_mtile_size) == 80);
const _: () = assert!(size_of::<HwrtData>() == HwrtData::SIZE);
const _: () = assert!(offset_of!(HwrtData, freelists_fw_addr) == 176);
const _: () = assert!(offset_of!(HwrtData, vheap_table_dev_addr) == 200);
const _: () = assert!(offset_of!(HwrtData, rta_ctl) == 208);
const _: () = assert!(offset_of!(HwrtData, tail_ptrs_dev_addr) == 240);
const _: () = assert!(offset_of!(HwrtData, geom_caches_need_zeroing) == 276);
const _: () = assert!(offset_of!(HwrtData, cleanup_state) == 320);
const _: () = assert!(size_of::<RtaCtl>() == 32);
