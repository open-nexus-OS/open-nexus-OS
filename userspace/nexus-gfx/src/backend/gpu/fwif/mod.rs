// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The firmware's host interface (TASK-0329 P1; firmware 1.1, build 6976702): the
//! structures the host writes and the firmware reads (SYS init, OS init, the connection
//! control, the runtime config, the command buffers' control words, the kernel commands, the
//! memory context, the common context) and the ones the firmware writes (the return slots,
//! the OS data's interrupt count, the SYS data's power state, the timeline words) — as `#[repr(C)]`
//! Rust with every size and field offset pinned by `const` asserts mirrored from the interface's
//! own check headers (MIT). The large firmware-written blocks (SYS data, the HWR info buffer,
//! the GPU-utilisation block) are opaque byte arrays with the few offsets the host reads.
//! The command type values carry the interface's magic in their upper half; a command without
//! it is dropped by both sides.
//! OWNERS: @gpu
//! STATUS: Functional
//! API_STABILITY: Internal (the firmware's; a new firmware major re-pins every number here)
//! TEST_COVERAGE: the `const` asserts (compile-time) + unit tests below

use core::mem::{offset_of, size_of};

mod commands;
mod contexts;
mod freelist;
mod hwrt;
mod render;
mod render_stream;
mod transfer;

pub use commands::*;
pub use contexts::*;
pub use freelist::*;
pub use hwrt::*;
pub use render::*;
pub use render_stream::{StreamBytes, MAX_STREAM_BYTES};
pub use transfer::*;

/// The magic every command type carries in its upper half.
pub const CMD_MAGIC: u32 = 0x2ABC << 16;
pub const CMD_MAGIC_MASK: u32 = 0xFFFF_0000;
/// Client command types with this bit are dispatched to a data master (a "task").
pub const CCB_TYPE_TASK: u32 = 1 << 15;

/// Kernel command buffer command types.
pub mod kccb {
    use super::CMD_MAGIC;
    pub const KICK: u32 = 101 | CMD_MAGIC;
    pub const MMUCACHE: u32 = 102 | CMD_MAGIC;
    pub const CLEANUP: u32 = 106 | CMD_MAGIC;
    pub const POW: u32 = 107 | CMD_MAGIC;
    pub const HEALTH_CHECK: u32 = 115 | CMD_MAGIC;
    pub const COMBINED_GEOM_FRAG_KICK: u32 = 117 | CMD_MAGIC;
    pub const LOGTYPE_UPDATE: u32 = 206 | CMD_MAGIC;
    /// The buffer holds 2^7 commands.
    pub const NUM_CMDS_LOG2: u32 = 7;
    pub const NUM_CMDS: u32 = 1 << NUM_CMDS_LOG2;
    /// Return-slot bits the firmware sets.
    pub const RTN_SLOT_CMD_EXECUTED: u32 = 1;
    pub const RTN_SLOT_CLEANUP_BUSY: u32 = 2;
    pub const RTN_SLOT_POLL_FAILURE: u32 = 4;
    pub const RTN_SLOT_NO_RESPONSE: u32 = 0;
}

/// What a `CLEANUP` names (the interface's cleanup types).
pub mod cleanup {
    /// A queue's common context.
    pub const COMMON_CONTEXT: u32 = 0;
    /// One render-target data of a dataset.
    pub const HWRT_DATA: u32 = 1;
    pub const FREELIST: u32 = 2;
}

/// Firmware-to-host command buffer command types.
pub mod fwccb {
    use super::CMD_MAGIC;
    pub const ZSBUFFER_BACKING: u32 = 101 | CMD_MAGIC;
    pub const ZSBUFFER_UNBACKING: u32 = 102 | CMD_MAGIC;
    pub const FREELIST_GROW: u32 = 103 | CMD_MAGIC;
    pub const FREELISTS_RECONSTRUCTION: u32 = 104 | CMD_MAGIC;
    pub const CONTEXT_RESET_NOTIFICATION: u32 = 105 | CMD_MAGIC;
    pub const DEBUG_DUMP: u32 = 106 | CMD_MAGIC;
    pub const UPDATE_STATS: u32 = 107 | CMD_MAGIC;
    pub const CORE_CLK_RATE_CHANGE: u32 = 108 | CMD_MAGIC;
    pub const REQUEST_GPU_RESTART: u32 = 109 | CMD_MAGIC;
    pub const CONTEXT_FW_PF_NOTIFICATION: u32 = 112 | CMD_MAGIC;
    pub const NUM_CMDS_LOG2: u32 = 5;
    pub const NUM_CMDS: u32 = 1 << NUM_CMDS_LOG2;
}

/// Client command buffer command types.
pub mod ccb {
    use super::{CCB_TYPE_TASK, CMD_MAGIC};
    pub const GEOM: u32 = 201 | CMD_MAGIC | CCB_TYPE_TASK;
    pub const TQ_3D: u32 = 202 | CMD_MAGIC | CCB_TYPE_TASK;
    pub const FRAG: u32 = 203 | CMD_MAGIC | CCB_TYPE_TASK;
    pub const FRAG_PR: u32 = 204 | CMD_MAGIC | CCB_TYPE_TASK;
    pub const CDM: u32 = 205 | CMD_MAGIC | CCB_TYPE_TASK;
    pub const TQ_TDM: u32 = 206 | CMD_MAGIC | CCB_TYPE_TASK;
    pub const NULL: u32 = 210 | CMD_MAGIC | CCB_TYPE_TASK;
    pub const FENCE: u32 = 212 | CMD_MAGIC;
    pub const UPDATE: u32 = 213 | CMD_MAGIC;
    pub const FENCE_PR: u32 = 215 | CMD_MAGIC;
    pub const PRIORITY: u32 = 216 | CMD_MAGIC;
    pub const PADDING: u32 = 221 | CMD_MAGIC;
    /// Every per-queue client buffer: 32 KiB.
    pub const SIZE_LOG2: u32 = 15;
    pub const SIZE: u32 = 1 << SIZE_LOG2;
}

/// Data masters.
pub mod dm {
    pub const GP: u32 = 0;
    pub const TDM: u32 = 1;
    pub const GEOM: u32 = 2;
    pub const FRAG: u32 = 3;
    pub const CDM: u32 = 4;
    pub const RAY: u32 = 5;
    pub const MAX: usize = 9;
}

/// MMU-cache command flags.
pub mod mmucache {
    pub const PT: u32 = 0x1;
    pub const PD: u32 = 0x2;
    pub const PC: u32 = 0x4;
    pub const PMTLB: u32 = 0x10;
    pub const TLB: u32 = PMTLB | 0x8;
    pub const CTX_ALL: u32 = 0x800;
    pub const INTERRUPT: u32 = 0x400_0000;
    /// A level-0 change.
    pub const LEVEL_0: u32 = PT | INTERRUPT | TLB;
    pub const LEVEL_1: u32 = LEVEL_0 | PD;
    pub const LEVEL_2: u32 = LEVEL_1 | PC;
}

/// Firmware context priorities (the firmware's `priority` field).
pub mod priority {
    pub const LOW: u32 = 0;
    pub const MEDIUM: u32 = 1;
    pub const HIGH: u32 = 2;
}

/// SYS data power states.
pub mod pow_state {
    pub const OFF: u32 = 0;
    pub const ON: u32 = 1;
    pub const FORCED_IDLE: u32 = 2;
    pub const IDLE: u32 = 3;
}

/// The invalid catalogue-base set: the firmware assigns the slot.
pub const BIF_INVALID_PCSET: u32 = 0xFFFF_FFFF;
pub const COMPCHECKS_LAYOUT_VERSION: u32 = 3;
pub const TRACE_BUF_DEFAULT_DWORDS: u32 = 12000;
pub const MAX_DEADLINE_MS: u32 = 30_000;

// ---- small shared types -----------------------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ufo {
    pub addr: u32,
    pub value: u32,
}

#[repr(C, align(8))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DmaAddr {
    pub dev_addr: u64,
    pub fw_addr: u32,
    pub padding: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DllistNode {
    pub p: u32,
    pub n: u32,
}

// ---- the command buffers' control words --------------------------------------------------

/// Kernel/firmware command buffer control: the write offset on its own 128-byte line.
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug)]
pub struct CcbCtl {
    pub write_offset: u32,
    pub padding: [u8; 124],
    pub read_offset: u32,
    pub wrap_mask: u32,
    pub cmd_size: u32,
    pub padding2: u32,
}

impl CcbCtl {
    pub const fn new(num_cmds: u32, cmd_size: u32) -> Self {
        Self {
            write_offset: 0,
            padding: [0; 124],
            read_offset: 0,
            wrap_mask: num_cmds - 1,
            cmd_size,
            padding2: 0,
        }
    }
}

/// Client command buffer control.
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug, Default)]
pub struct CccbCtl {
    pub write_offset: u32,
    pub read_offset: u32,
    pub dep_offset: u32,
    pub wrap_mask: u32,
    pub read_offset2: u32,
    pub read_offset3: u32,
    pub read_offset4: u32,
    pub padding: u32,
}

// ---- the bytes (safe, field by field, at the pinned offsets) ------------------------------

pub(super) fn put32(b: &mut [u8], off: usize, v: u32) {
    b[off..off + 4].copy_from_slice(&v.to_le_bytes());
}
pub(super) fn put64(b: &mut [u8], off: usize, v: u64) {
    b[off..off + 8].copy_from_slice(&v.to_le_bytes());
}

impl CcbCtl {
    pub const SIZE: usize = 144;
    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut b = [0u8; Self::SIZE];
        put32(&mut b, 0, self.write_offset);
        put32(&mut b, 128, self.read_offset);
        put32(&mut b, 132, self.wrap_mask);
        put32(&mut b, 136, self.cmd_size);
        b
    }
    /// Offsets of the two moving words.
    pub const WRITE_OFFSET: usize = 0;
    pub const READ_OFFSET: usize = 128;
}

impl CccbCtl {
    pub const SIZE: usize = 32;
    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut b = [0u8; Self::SIZE];
        put32(&mut b, 0, self.write_offset);
        put32(&mut b, 4, self.read_offset);
        put32(&mut b, 8, self.dep_offset);
        put32(&mut b, 12, self.wrap_mask);
        b
    }
    pub const WRITE_OFFSET: usize = 0;
    pub const READ_OFFSET: usize = 4;
}

// ---- the pins ----------------------------------------------------------------------------

const _: () = {
    assert!(size_of::<Ufo>() == 8);
    assert!(size_of::<DmaAddr>() == 16);
    assert!(size_of::<CcbCtl>() == 144);
    assert!(offset_of!(CcbCtl, read_offset) == 128);
    assert!(offset_of!(CcbCtl, wrap_mask) == 132);
    assert!(offset_of!(CcbCtl, cmd_size) == 136);
    assert!(size_of::<CccbCtl>() == 32);
    assert!(size_of::<KccbCmd>() == 88);
    assert!(offset_of!(KccbCmd, data) == 8);
    assert!(size_of::<FwccbCmd>() == 88);
    assert!(size_of::<CcbCmdHeader>() == 40);
    assert!(offset_of!(CcbCmdHeader, work_est) == 16);
    assert!(size_of::<FwMemContext>() == 32);
    assert!(offset_of!(FwMemContext, page_cat_base_reg_set) == 8);
    assert!(offset_of!(FwMemContext, fw_mem_ctx_flags) == 24);
    assert!(size_of::<FwCommonContext>() == 152);
    assert!(offset_of!(FwCommonContext, ccb_meta_dma_addr) == 8);
    assert!(offset_of!(FwCommonContext, context_state_addr) == 24);
    assert!(offset_of!(FwCommonContext, priority) == 32);
    assert!(offset_of!(FwCommonContext, dm) == 60);
    assert!(offset_of!(FwCommonContext, wait_signal_address) == 64);
    assert!(offset_of!(FwCommonContext, cbuf_queue_ctrl_addr) == 88);
    assert!(offset_of!(FwCommonContext, max_deadline_ms) == 104);
    assert!(offset_of!(FwCommonContext, waiting_node) == 112);
    assert!(offset_of!(FwCommonContext, last_failed_ufo) == 128);
    assert!(offset_of!(FwCommonContext, fw_mem_context_fw_addr) == 136);
    assert!(offset_of!(FwCommonContext, pid) == 144);
    assert!(offset_of!(FwCommonContext, geom_oom_disabled) == 148);
    assert!(size_of::<CompChecksBvnc>() == 16);
    assert!(offset_of!(CompChecksBvnc, bvnc) == 8);
    assert!(size_of::<CompChecks>() == 64);
    assert!(offset_of!(CompChecks, fw_bvnc) == 16);
    assert!(offset_of!(CompChecks, fw_processor_version) == 32);
    assert!(offset_of!(CompChecks, init_options) == 48);
    assert!(offset_of!(CompChecks, updated) == 56);
    assert!(size_of::<OsInit>() == 104);
    assert!(offset_of!(OsInit, hwr_info_buf_ctl_fw_addr) == 28);
    assert!(offset_of!(OsInit, fw_os_data_fw_addr) == 36);
    assert!(offset_of!(OsInit, comp_checks) == 40);
    assert!(size_of::<SigbufCtl>() == 8);
    assert!(size_of::<PdvfsOpp>() == 136);
    assert!(size_of::<HwperfBvnc>() == 160);
    assert!(offset_of!(HwperfBvnc, bvnc_blocks) == 32);
    assert!(size_of::<SysInit>() == 544);
    assert!(offset_of!(SysInit, tpu_trilinear_frac_mask) == 64);
    assert!(offset_of!(SysInit, sigbuf_ctl) == 80);
    assert!(offset_of!(SysInit, pdvfs_opp_info) == 152);
    assert!(offset_of!(SysInit, coremem_data_store) == 288);
    assert!(offset_of!(SysInit, counter_dump_ctl) == 304);
    assert!(offset_of!(SysInit, filter_flags) == 312);
    assert!(offset_of!(SysInit, runtime_cfg_fw_addr) == 316);
    assert!(offset_of!(SysInit, trace_buf_ctl_fw_addr) == 320);
    assert!(offset_of!(SysInit, fw_sys_data_fw_addr) == 324);
    assert!(offset_of!(SysInit, gpu_util_fw_cb_ctl_fw_addr) == 328);
    assert!(offset_of!(SysInit, align_checks) == 340);
    assert!(offset_of!(SysInit, initial_core_clock_speed) == 344);
    assert!(offset_of!(SysInit, firmware_started) == SYSINIT_FIRMWARE_STARTED);
    assert!(offset_of!(SysInit, marker_val) == 356);
    assert!(offset_of!(SysInit, firmware_started_timestamp) == SYSINIT_FIRMWARE_STARTED_TIMESTAMP);
    assert!(offset_of!(SysInit, firmware_perf) == 368);
    assert!(offset_of!(SysInit, gpio_validation_mode) == 376);
    assert!(offset_of!(SysInit, bvnc_km_feature_flags) == 380);
    assert!(offset_of!(SysInit, tfbc_compression_control) == 540);
    assert!(size_of::<RuntimeCfg>() == 72);
    assert!(offset_of!(RuntimeCfg, core_clock_speed) == 12);
    assert!(offset_of!(RuntimeCfg, wdg_period_us) == 28);
    assert!(offset_of!(RuntimeCfg, osid_priority) == 32);
    assert!(offset_of!(RuntimeCfg, hwperf_buf_fw_addr) == 64);
    assert!(size_of::<ConnectionCtl>() == 16);
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_types_carry_the_magic_and_the_task_bit() {
        assert_eq!(kccb::KICK, 0x2ABC_0065);
        assert_eq!(kccb::MMUCACHE, 0x2ABC_0066);
        assert_eq!(kccb::HEALTH_CHECK, 0x2ABC_0073);
        assert_eq!(ccb::UPDATE, 0x2ABC_00D5);
        assert_eq!(ccb::NULL, 0x2ABC_80D2);
        assert_eq!(ccb::PADDING, 0x2ABC_00DD);
        assert_eq!(fwccb::CONTEXT_RESET_NOTIFICATION, 0x2ABC_0069);
        assert_eq!(mmucache::LEVEL_2, 0x400_001F);
    }

    #[test]
    fn a_kick_and_a_flush_lay_their_payload_where_the_firmware_reads_it() {
        let k = KccbCmd::kick(0xC012_3000, 0x58, 0x7FFF).to_bytes();
        assert_eq!(&k[0..4], &kccb::KICK.to_le_bytes());
        assert_eq!(&k[8..12], &0xC012_3000u32.to_le_bytes());
        assert_eq!(&k[12..16], &0x58u32.to_le_bytes());
        assert_eq!(&k[16..20], &0x7FFFu32.to_le_bytes());
        assert_eq!(&k[20..24], &[0; 4], "no cleanup controls");
        let m = KccbCmd::mmu_cache(mmucache::LEVEL_2, 0xC000_8000, 0).to_bytes();
        assert_eq!(&m[8..12], &mmucache::LEVEL_2.to_le_bytes());
        assert_eq!(&m[12..16], &0xC000_8000u32.to_le_bytes());
    }

    #[test]
    fn a_cleanup_names_its_object_by_kind_and_address() {
        assert_eq!(kccb::CLEANUP, 0x2ABC_006A);
        let c = KccbCmd::cleanup(cleanup::FREELIST, 0xC045_6000).to_bytes();
        assert_eq!(&c[0..4], &kccb::CLEANUP.to_le_bytes());
        assert_eq!(&c[4..8], &[0; 4], "no flags");
        assert_eq!(&c[8..12], &2u32.to_le_bytes());
        assert_eq!(&c[12..16], &0xC045_6000u32.to_le_bytes());
        assert!(c[16..].iter().all(|&b| b == 0));
        let h = KccbCmd::cleanup(cleanup::HWRT_DATA, 1).to_bytes();
        assert_eq!(&h[8..16], &[1, 0, 0, 0, 1, 0, 0, 0]);
        assert_eq!(&KccbCmd::cleanup(cleanup::COMMON_CONTEXT, 0).to_bytes()[8..12], &[0; 4]);
    }

    #[test]
    fn a_firmware_command_without_the_magic_is_not_one() {
        let mut b = [0u8; 88];
        b[0..4].copy_from_slice(&105u32.to_le_bytes());
        assert!(!FwccbCmd::from_bytes(&b).has_magic());
        b[0..4].copy_from_slice(&fwccb::CONTEXT_RESET_NOTIFICATION.to_le_bytes());
        b[8..12].copy_from_slice(&7u32.to_le_bytes());
        b[12..16].copy_from_slice(&1u32.to_le_bytes());
        b[40..48].copy_from_slice(&0x8000_1000u64.to_le_bytes());
        let c = FwccbCmd::from_bytes(&b);
        assert!(c.has_magic());
        assert_eq!(c.kind(), 105);
        assert_eq!(c.context_reset(), Some((7, 1, 0, 0, 0, 0, 0x8000_1000)));
    }

    #[test]
    fn the_init_blocks_carry_the_open_values() {
        let s = SysInit::new(&SysInitAddrs {
            fault_page_phys: 0x8000_0000,
            runtime_cfg: 1,
            trace_buf_ctl: 2,
            sys_data: 3,
            gpu_util_fwcb: 4,
            core_clock_hz: 614_400_000,
        });
        assert_eq!(s.pds_exec_base, 0xDA_0000_0000);
        assert_eq!(s.usc_exec_base, 0xE0_0000_0000);
        assert_eq!(s.marker_val, 1);
        assert_eq!(s.firmware_started, 0);
        assert_eq!(s.initial_core_clock_speed, 614_400_000);
        let r = RuntimeCfg::new(614_400_000, 1);
        assert_eq!(
            (r.wdg_period_us, r.active_pm_latency_persistant, r.default_dusts_num_init),
            (0, 1, 1)
        );
        let o = OsInit::new(&OsInitAddrs {
            kccb_ctl: 1,
            kccb: 2,
            kccb_rtn_slots: 3,
            fwccb_ctl: 4,
            fwccb: 5,
            hwr_info_buf: 6,
            os_data: 7,
        });
        assert_eq!(o.comp_checks.hw_bvnc.layout_version, 3);
        assert_eq!(o.work_est_firmware_ccb_fw_addr, 0);
        let m = FwMemContext::new(0x9000_0000);
        assert_eq!(m.page_cat_base_reg_set, BIF_INVALID_PCSET);
        let c = FwCommonContext::for_queue(&QueueInit {
            cccb_ctl_fw_addr: 1,
            cccb_fw_addr: 2,
            dm: dm::FRAG,
            priority: priority::HIGH,
            fw_mem_context_fw_addr: 3,
            context_state_fw_addr: 4,
            server_common_context_id: 5,
            pid: 6,
        });
        assert_eq!((c.dm, c.max_deadline_ms, c.priority), (3, 30_000, 2));
        let mut t = [0u8; TRACEBUF_SIZE];
        tracebuf_init(&mut t, 0xC00A_0000);
        assert_eq!(&t[12..16], &0xC00A_0000u32.to_le_bytes());
        assert_eq!(&t[856..860], &12000u32.to_le_bytes());
        assert_eq!(&t[0..4], &[0; 4], "tracing off");
    }
}
