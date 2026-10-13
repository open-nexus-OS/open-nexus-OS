// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The contexts and the init blocks (TASK-0329 P1): the firmware memory context (one page
//! catalogue; the firmware assigns the slot), the firmware common context (one queue), and
//! what the host writes before the start — SYS init with its liveness word, OS init with the
//! command buffers' addresses, the runtime config, the connection control, the trace control,
//! the opaque firmware-written blocks' sizes and offsets — with the open driver's init values
//! and the byte encoders at the pinned offsets. Part of [`super`] (`fwif`).

use super::{
    dm, put32, put64, DllistNode, DmaAddr, Ufo, BIF_INVALID_PCSET, COMPCHECKS_LAYOUT_VERSION,
    MAX_DEADLINE_MS, TRACE_BUF_DEFAULT_DWORDS,
};

/// The firmware memory context: one page catalogue; the slot the firmware assigns.
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug)]
pub struct FwMemContext {
    pub pc_dev_paddr: u64,
    pub page_cat_base_reg_set: u32,
    pub breakpoint_addr: u32,
    pub bp_handler_addr: u32,
    pub breakpoint_ctl: u32,
    pub fw_mem_ctx_flags: u32,
    pub padding: u32,
}

impl FwMemContext {
    pub const fn new(pc_phys: u64) -> Self {
        Self {
            pc_dev_paddr: pc_phys,
            page_cat_base_reg_set: BIF_INVALID_PCSET,
            breakpoint_addr: 0,
            bp_handler_addr: 0,
            breakpoint_ctl: 0,
            fw_mem_ctx_flags: 0,
            padding: 0,
        }
    }
}

/// The firmware common context: one queue.
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug, Default)]
pub struct FwCommonContext {
    pub ccbctl_fw_addr: u32,
    pub ccb_fw_addr: u32,
    pub ccb_meta_dma_addr: DmaAddr,
    pub context_state_addr: u32,
    pub fw_com_ctx_flags: u32,
    pub priority: u32,
    pub priority_seq_num: u32,
    pub rf_cmd_addr: u32,
    pub stats_pending: u32,
    pub stats_num_stores: i32,
    pub stats_num_out_of_memory: i32,
    pub stats_num_partial_renders: i32,
    pub dm: u32,
    pub wait_signal_address: u64,
    pub wait_signal_node: DllistNode,
    pub buf_stalled_node: DllistNode,
    pub cbuf_queue_ctrl_addr: u64,
    pub robustness_address: u64,
    pub max_deadline_ms: u32,
    pub read_offset_needs_reset: u32,
    pub waiting_node: DllistNode,
    pub run_node: DllistNode,
    pub last_failed_ufo: Ufo,
    pub fw_mem_context_fw_addr: u32,
    pub server_common_context_id: u32,
    pub pid: u32,
    pub geom_oom_disabled: u32,
}

/// What a queue's firmware context is made of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueInit {
    pub cccb_ctl_fw_addr: u32,
    pub cccb_fw_addr: u32,
    pub dm: u32,
    pub priority: u32,
    pub fw_mem_context_fw_addr: u32,
    pub context_state_fw_addr: u32,
    pub server_common_context_id: u32,
    pub pid: u32,
}

impl FwCommonContext {
    pub fn for_queue(q: &QueueInit) -> Self {
        Self {
            ccbctl_fw_addr: q.cccb_ctl_fw_addr,
            ccb_fw_addr: q.cccb_fw_addr,
            dm: q.dm,
            priority: q.priority,
            priority_seq_num: 0,
            max_deadline_ms: MAX_DEADLINE_MS,
            pid: q.pid,
            server_common_context_id: q.server_common_context_id,
            fw_mem_context_fw_addr: q.fw_mem_context_fw_addr,
            context_state_addr: q.context_state_fw_addr,
            ..Default::default()
        }
    }
}

/// A transfer queue's register-state save area on a core with the XE memory hierarchy: the
/// fragment context state with one ISP store register (12 + 4 bytes, 8-aligned).
pub const TRANSFER_CTX_STATE_SIZE: u32 = 16;

// ---- the init blocks ---------------------------------------------------------------------

#[repr(C, align(8))]
#[derive(Clone, Copy, Debug)]
pub struct CompChecksBvnc {
    pub layout_version: u32,
    pub padding: u32,
    pub bvnc: u64,
}

impl CompChecksBvnc {
    pub const fn init() -> Self {
        Self { layout_version: COMPCHECKS_LAYOUT_VERSION, padding: 0, bvnc: 0 }
    }
}

#[repr(C, align(8))]
#[derive(Clone, Copy, Debug)]
pub struct CompChecks {
    pub hw_bvnc: CompChecksBvnc,
    pub fw_bvnc: CompChecksBvnc,
    pub fw_processor_version: u32,
    pub ddk_version: u32,
    pub ddk_build: u32,
    pub build_options: u32,
    pub init_options: [u8; 8],
    pub updated: u32,
    pub padding: u32,
}

impl CompChecks {
    pub const fn init() -> Self {
        Self {
            hw_bvnc: CompChecksBvnc::init(),
            fw_bvnc: CompChecksBvnc::init(),
            fw_processor_version: 0,
            ddk_version: 0,
            ddk_build: 0,
            build_options: 0,
            init_options: [0; 8],
            updated: 0,
            padding: 0,
        }
    }
}

/// OS init: where the command buffers and the OS data are.
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug)]
pub struct OsInit {
    pub kernel_ccbctl_fw_addr: u32,
    pub kernel_ccb_fw_addr: u32,
    pub kernel_ccb_rtn_slots_fw_addr: u32,
    pub firmware_ccbctl_fw_addr: u32,
    pub firmware_ccb_fw_addr: u32,
    pub work_est_firmware_ccbctl_fw_addr: u32,
    pub work_est_firmware_ccb_fw_addr: u32,
    pub hwr_info_buf_ctl_fw_addr: u32,
    pub hwr_debug_dump_limit: u32,
    pub fw_os_data_fw_addr: u32,
    pub comp_checks: CompChecks,
}

/// The firmware addresses OS init names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OsInitAddrs {
    pub kccb_ctl: u32,
    pub kccb: u32,
    pub kccb_rtn_slots: u32,
    pub fwccb_ctl: u32,
    pub fwccb: u32,
    pub hwr_info_buf: u32,
    pub os_data: u32,
}

impl OsInit {
    pub const fn new(a: &OsInitAddrs) -> Self {
        Self {
            kernel_ccbctl_fw_addr: a.kccb_ctl,
            kernel_ccb_fw_addr: a.kccb,
            kernel_ccb_rtn_slots_fw_addr: a.kccb_rtn_slots,
            firmware_ccbctl_fw_addr: a.fwccb_ctl,
            firmware_ccb_fw_addr: a.fwccb,
            work_est_firmware_ccbctl_fw_addr: 0,
            work_est_firmware_ccb_fw_addr: 0,
            hwr_info_buf_ctl_fw_addr: a.hwr_info_buf,
            hwr_debug_dump_limit: 0,
            fw_os_data_fw_addr: a.os_data,
            comp_checks: CompChecks::init(),
        }
    }
}

#[repr(C, align(8))]
#[derive(Clone, Copy, Debug, Default)]
pub struct SigbufCtl {
    pub buffer_fw_addr: u32,
    pub left_size_in_regs: u32,
}

#[repr(C, align(8))]
#[derive(Clone, Copy, Debug)]
pub struct PdvfsOpp {
    pub opp_values: [[u32; 2]; 16],
    pub min_opp_point: u32,
    pub max_opp_point: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct HwperfBvnc {
    pub bvnc_string: [u8; 24],
    pub bvnc_km_feature_flags: u32,
    pub num_bvnc_blocks: u16,
    pub bvnc_gpu_cores: u16,
    pub bvnc_blocks: [[u16; 4]; 16],
}

/// SYS init: the heaps' bases, the fault page, the clock, where the runtime config, trace
/// control, SYS data and utilisation block are — and `firmware_started`, the liveness word.
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug)]
pub struct SysInit {
    pub fault_phys_addr: u64,
    pub pds_exec_base: u64,
    pub usc_exec_base: u64,
    pub fbcdc_state_table_base: u64,
    pub fbcdc_large_state_table_base: u64,
    pub texture_heap_base: u64,
    pub hw_perf_filter: u64,
    pub slc3_fence_dev_addr: u64,
    pub tpu_trilinear_frac_mask: [u32; 4],
    pub sigbuf_ctl: [SigbufCtl; dm::MAX],
    pub pdvfs_opp_info: PdvfsOpp,
    pub coremem_data_store: DmaAddr,
    pub counter_dump_ctl: [u32; 2],
    pub filter_flags: u32,
    pub runtime_cfg_fw_addr: u32,
    pub trace_buf_ctl_fw_addr: u32,
    pub fw_sys_data_fw_addr: u32,
    pub gpu_util_fw_cb_ctl_fw_addr: u32,
    pub reg_cfg_fw_addr: u32,
    pub hwperf_ctl_fw_addr: u32,
    pub align_checks: u32,
    pub initial_core_clock_speed: u32,
    pub active_pm_latency_ms: u32,
    pub firmware_started: u32,
    pub marker_val: u32,
    pub firmware_started_timestamp: u32,
    pub jones_disable_mask: u32,
    pub firmware_perf: u32,
    pub core_clock_rate_fw_addr: u32,
    pub gpio_validation_mode: u32,
    pub bvnc_km_feature_flags: HwperfBvnc,
    pub tfbc_compression_control: u32,
}

/// What SYS init is made of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SysInitAddrs {
    pub fault_page_phys: u64,
    pub runtime_cfg: u32,
    pub trace_buf_ctl: u32,
    pub sys_data: u32,
    pub gpu_util_fwcb: u32,
    pub core_clock_hz: u32,
}

/// Byte offset of `firmware_started` inside SYS init (the liveness word).
pub const SYSINIT_FIRMWARE_STARTED: usize = 352;
pub const SYSINIT_FIRMWARE_STARTED_TIMESTAMP: usize = 360;

impl SysInit {
    pub const fn new(a: &SysInitAddrs) -> Self {
        Self {
            fault_phys_addr: a.fault_page_phys,
            pds_exec_base: crate::backend::gpu::heaps::PDS_CODE_DATA_BASE,
            usc_exec_base: crate::backend::gpu::heaps::USC_CODE_BASE,
            fbcdc_state_table_base: 0,
            fbcdc_large_state_table_base: 0,
            texture_heap_base: 0,
            hw_perf_filter: 0,
            slc3_fence_dev_addr: 0,
            tpu_trilinear_frac_mask: [0; 4],
            sigbuf_ctl: [SigbufCtl { buffer_fw_addr: 0, left_size_in_regs: 0 }; dm::MAX],
            pdvfs_opp_info: PdvfsOpp {
                opp_values: [[0; 2]; 16],
                min_opp_point: 0,
                max_opp_point: 0,
            },
            coremem_data_store: DmaAddr { dev_addr: 0, fw_addr: 0, padding: 0 },
            counter_dump_ctl: [0; 2],
            filter_flags: 0,
            runtime_cfg_fw_addr: a.runtime_cfg,
            trace_buf_ctl_fw_addr: a.trace_buf_ctl,
            fw_sys_data_fw_addr: a.sys_data,
            gpu_util_fw_cb_ctl_fw_addr: a.gpu_util_fwcb,
            reg_cfg_fw_addr: 0,
            hwperf_ctl_fw_addr: 0,
            align_checks: 0,
            initial_core_clock_speed: a.core_clock_hz,
            active_pm_latency_ms: 0,
            firmware_started: 0,
            marker_val: 1,
            firmware_started_timestamp: 0,
            jones_disable_mask: 0,
            firmware_perf: 0,
            core_clock_rate_fw_addr: 0,
            gpio_validation_mode: 0,
            bvnc_km_feature_flags: HwperfBvnc {
                bvnc_string: [0; 24],
                bvnc_km_feature_flags: 0,
                num_bvnc_blocks: 0,
                bvnc_gpu_cores: 0,
                bvnc_blocks: [[0; 4]; 16],
            },
            tfbc_compression_control: 0,
        }
    }
}

/// The runtime config.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct RuntimeCfg {
    pub active_pm_latency_ms: u32,
    pub runtime_cfg_flags: u32,
    pub active_pm_latency_persistant: u32,
    pub core_clock_speed: u32,
    pub default_dusts_num_init: u32,
    pub phr_mode: u32,
    pub hcs_deadline_ms: u32,
    pub wdg_period_us: u32,
    pub osid_priority: [u32; 8],
    pub hwperf_buf_fw_addr: u32,
    pub padding: u32,
}

impl RuntimeCfg {
    /// The open driver's values: no active power management latency (persistent), the clock,
    /// the cluster count, the firmware watchdog off.
    pub const fn new(core_clock_hz: u32, num_clusters: u32) -> Self {
        Self {
            active_pm_latency_ms: 0,
            runtime_cfg_flags: 0,
            active_pm_latency_persistant: 1,
            core_clock_speed: core_clock_hz,
            default_dusts_num_init: num_clusters,
            phr_mode: 0,
            hcs_deadline_ms: 0,
            wdg_period_us: 0,
            osid_priority: [0; 8],
            hwperf_buf_fw_addr: 0,
            padding: 0,
        }
    }
}

/// The connection control block (all zero at init: firmware and OS offline).
#[repr(C, align(8))]
#[derive(Clone, Copy, Debug, Default)]
pub struct ConnectionCtl {
    pub connection_fw_state: u32,
    pub connection_os_state: u32,
    pub alive_fw_token: u32,
    pub alive_os_token: u32,
}

/// Trace buffer control (864 bytes): the log type, per thread the trace pointer, the trace
/// buffer's firmware address and the assert buffer; the size in dwords.
pub const TRACEBUF_SIZE: usize = 864;
pub const TRACEBUF_LOG_TYPE: usize = 0;
pub const TRACEBUF_SPACE: usize = 8;
pub const TRACEBUF_SPACE_SIZE: usize = 424;
pub const TRACEBUF_SPACE_TRACE_POINTER: usize = 0;
pub const TRACEBUF_SPACE_BUFFER_FW_ADDR: usize = 4;
pub const TRACEBUF_SIZE_IN_DWORDS: usize = 856;

/// Fills a zeroed trace control block: thread 0's buffer at `trace_buffer_fw_addr`, the default
/// size, tracing off (`log_type` 0).
pub fn tracebuf_init(block: &mut [u8; TRACEBUF_SIZE], trace_buffer_fw_addr: u32) {
    block.fill(0);
    block[TRACEBUF_SPACE + TRACEBUF_SPACE_BUFFER_FW_ADDR
        ..TRACEBUF_SPACE + TRACEBUF_SPACE_BUFFER_FW_ADDR + 4]
        .copy_from_slice(&trace_buffer_fw_addr.to_le_bytes());
    block[TRACEBUF_SIZE_IN_DWORDS..TRACEBUF_SIZE_IN_DWORDS + 4]
        .copy_from_slice(&TRACE_BUF_DEFAULT_DWORDS.to_le_bytes());
}

/// SYS data (3624 bytes, firmware-written): the offsets the host reads.
pub const SYSDATA_SIZE: usize = 3624;
pub const SYSDATA_CONFIG_FLAGS: usize = 0;
pub const SYSDATA_POW_STATE: usize = 8;
pub const SYSDATA_HWR_STATE_FLAGS: usize = 3576;

/// OS data (584 bytes): the host writes `power_sync_fw_addr`; the firmware the rest.
pub const OSDATA_SIZE: usize = 584;
pub const OSDATA_INTERRUPT_COUNT: usize = 560;
pub const OSDATA_KCCB_CMDS_EXECUTED: usize = 568;
pub const OSDATA_POWER_SYNC_FW_ADDR: usize = 572;

/// The HWR info buffer (firmware-written).
pub const HWRINFOBUF_SIZE: usize = 2336;
pub const HWRINFOBUF_HWR_COUNTER: usize = 2176;

/// The GPU utilisation block: `last_word` starts as IDLE (0).
pub const GPU_UTIL_FWCB_SIZE: usize = 10280;
pub const GPU_UTIL_FWCB_LAST_WORD: usize = 10248;

/// The fault page: 4 KiB of `0xdeadbee0` words.
pub const FAULT_PAGE_SIZE: usize = 4096;
pub const FAULT_PAGE_WORD: u32 = 0xdead_bee0;

impl SysInit {
    pub const SIZE: usize = 544;
    /// The block as the firmware reads it (the zero fields stay zero).
    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut b = [0u8; Self::SIZE];
        put64(&mut b, 0, self.fault_phys_addr);
        put64(&mut b, 8, self.pds_exec_base);
        put64(&mut b, 16, self.usc_exec_base);
        put64(&mut b, 48, self.hw_perf_filter);
        put32(&mut b, 312, self.filter_flags);
        put32(&mut b, 316, self.runtime_cfg_fw_addr);
        put32(&mut b, 320, self.trace_buf_ctl_fw_addr);
        put32(&mut b, 324, self.fw_sys_data_fw_addr);
        put32(&mut b, 328, self.gpu_util_fw_cb_ctl_fw_addr);
        put32(&mut b, 340, self.align_checks);
        put32(&mut b, 344, self.initial_core_clock_speed);
        put32(&mut b, 348, self.active_pm_latency_ms);
        put32(&mut b, SYSINIT_FIRMWARE_STARTED, self.firmware_started);
        put32(&mut b, 356, self.marker_val);
        put32(&mut b, 368, self.firmware_perf);
        put32(&mut b, 376, self.gpio_validation_mode);
        b
    }
}

impl OsInit {
    pub const SIZE: usize = 104;
    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut b = [0u8; Self::SIZE];
        for (i, v) in [
            self.kernel_ccbctl_fw_addr,
            self.kernel_ccb_fw_addr,
            self.kernel_ccb_rtn_slots_fw_addr,
            self.firmware_ccbctl_fw_addr,
            self.firmware_ccb_fw_addr,
            self.work_est_firmware_ccbctl_fw_addr,
            self.work_est_firmware_ccb_fw_addr,
            self.hwr_info_buf_ctl_fw_addr,
            self.hwr_debug_dump_limit,
            self.fw_os_data_fw_addr,
        ]
        .iter()
        .enumerate()
        {
            put32(&mut b, i * 4, *v);
        }
        // The compatibility checks: both id blocks carry the layout version, nothing else.
        put32(&mut b, 40, self.comp_checks.hw_bvnc.layout_version);
        put64(&mut b, 48, self.comp_checks.hw_bvnc.bvnc);
        put32(&mut b, 56, self.comp_checks.fw_bvnc.layout_version);
        put64(&mut b, 64, self.comp_checks.fw_bvnc.bvnc);
        b
    }
}

impl RuntimeCfg {
    pub const SIZE: usize = 72;
    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut b = [0u8; Self::SIZE];
        put32(&mut b, 0, self.active_pm_latency_ms);
        put32(&mut b, 4, self.runtime_cfg_flags);
        put32(&mut b, 8, self.active_pm_latency_persistant);
        put32(&mut b, 12, self.core_clock_speed);
        put32(&mut b, 16, self.default_dusts_num_init);
        put32(&mut b, 20, self.phr_mode);
        put32(&mut b, 24, self.hcs_deadline_ms);
        put32(&mut b, 28, self.wdg_period_us);
        for (i, p) in self.osid_priority.iter().enumerate() {
            put32(&mut b, 32 + i * 4, *p);
        }
        put32(&mut b, 64, self.hwperf_buf_fw_addr);
        b
    }
}

impl FwMemContext {
    pub const SIZE: usize = 32;
    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut b = [0u8; Self::SIZE];
        put64(&mut b, 0, self.pc_dev_paddr);
        put32(&mut b, 8, self.page_cat_base_reg_set);
        put32(&mut b, 12, self.breakpoint_addr);
        put32(&mut b, 16, self.bp_handler_addr);
        put32(&mut b, 20, self.breakpoint_ctl);
        put32(&mut b, 24, self.fw_mem_ctx_flags);
        b
    }
}

impl FwCommonContext {
    pub const SIZE: usize = 152;
    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut b = [0u8; Self::SIZE];
        put32(&mut b, 0, self.ccbctl_fw_addr);
        put32(&mut b, 4, self.ccb_fw_addr);
        put64(&mut b, 8, self.ccb_meta_dma_addr.dev_addr);
        put32(&mut b, 16, self.ccb_meta_dma_addr.fw_addr);
        put32(&mut b, 24, self.context_state_addr);
        put32(&mut b, 28, self.fw_com_ctx_flags);
        put32(&mut b, 32, self.priority);
        put32(&mut b, 36, self.priority_seq_num);
        put32(&mut b, 40, self.rf_cmd_addr);
        put32(&mut b, 44, self.stats_pending);
        put32(&mut b, 60, self.dm);
        put64(&mut b, 64, self.wait_signal_address);
        put64(&mut b, 88, self.cbuf_queue_ctrl_addr);
        put64(&mut b, 96, self.robustness_address);
        put32(&mut b, 104, self.max_deadline_ms);
        put32(&mut b, 108, self.read_offset_needs_reset);
        put32(&mut b, 128, self.last_failed_ufo.addr);
        put32(&mut b, 132, self.last_failed_ufo.value);
        put32(&mut b, 136, self.fw_mem_context_fw_addr);
        put32(&mut b, 140, self.server_common_context_id);
        put32(&mut b, 144, self.pid);
        put32(&mut b, 148, self.geom_oom_disabled);
        b
    }
}
