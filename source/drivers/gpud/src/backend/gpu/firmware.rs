// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The firmware in memory (TASK-0329 G1): the published image (`resources/firmware/gpu/`, its
//! provenance beside it) parsed and refused unless it is the board core's open build; its code
//! and data regions placed at the heap offsets the open driver uses (code at 0, data at the
//! private-data section's fixed offset); the boot data the firmware's boot loader reads; the
//! structures the host interface fixes — the three config slots, the command buffers, the
//! sync words, the fault page, the trace and utilisation blocks — written with the open
//! driver's init values; the MIPS page table with one uncached entry per mapped heap page.
//! Everything written here is cleaned to memory before the processor starts.

use nexus_gfx::backend::gpu::fw_image::{Feature, FirmwareImage, Quirk, SectionId, SectionKind};
use nexus_gfx::backend::gpu::fwif::{
    self, CcbCtl, OsInit, OsInitAddrs, RuntimeCfg, SysInit, SysInitAddrs, TRACE_BUF_DEFAULT_DWORDS,
};
use nexus_gfx::backend::gpu::mips_boot::{self, BootData, BootSections, PAGE_TABLE_PAGES};
use nexus_gfx::backend::gpu::start::CoreFeatures;
use nexus_gfx::backend::gpu::CoreId;

use super::memory::{FwMemory, HeapObj, Page};

/// The published firmware for the board's core — unmodified, with its licence in the tree.
const BLOB: &[u8] =
    include_bytes!("../../../../../../resources/firmware/gpu/rogue_36.29.52.182_v1.fw");

/// Backing pages the firmware needs: code (0x19000) + data (0x5000) = 30, the config slots 3,
/// the structures about 20, the trace buffer 12, the page table 4 — and room for the contexts
/// and page tables of the device's address space (layer 4 binds a plane and the first quad's
/// 28 buffers: about 2400 pages behind some 15 table pages).
pub(super) const BACKING_PAGES: usize = 192;

/// Where everything landed.
pub(super) struct Loaded {
    pub boot: BootSections,
    pub features: CoreFeatures,
    /// The feature-gated words of a transfer stream, from the device-info block (RFC-0101 §8c).
    pub stream: nexus_gfx::backend::gpu::fwif::StreamFeatures,
    pub num_clusters: u32,
    pub sysinit: HeapObj,
    pub osdata: HeapObj,
    pub sysdata: HeapObj,
    pub kccb_ctl: HeapObj,
    pub kccb: HeapObj,
    pub kccb_rtn: HeapObj,
    pub fwccb_ctl: HeapObj,
    pub fwccb: HeapObj,
    pub mmucache_sync: HeapObj,
    pub pt_pages: [Page; PAGE_TABLE_PAGES],
    /// The boot-data section inside the data region (its boot-stage word is a witness).
    pub boot_data: HeapObj,
}

pub(super) struct LoadError(pub &'static str);

fn obj(mem: &mut FwMemory, len: u32, what: &'static str) -> Result<HeapObj, LoadError> {
    mem.alloc(len).ok_or(LoadError(what))
}

/// Parse, place, build.
#[inline(never)]
pub(super) fn load(
    mem: &mut FwMemory,
    reg_base_phys: u64,
    core_clock_hz: u32,
) -> Result<Loaded, LoadError> {
    let image =
        FirmwareImage::parse(BLOB, CoreId::BOARD).map_err(|_| LoadError("image refused"))?;
    let info = image.device_info();
    let features = CoreFeatures {
        pbe2_in_xe: info.has(Feature::Pbe2InXe),
        xe_tpu2: info.has(Feature::XeTpu2),
        sys_bus_secure_reset: info.has(Feature::SysBusSecureReset),
        slc_cache_line_size_bits: info.value(Feature::SlcCacheLineSizeBits).unwrap_or(512) as u32,
        gpu_multicore_support: info.has(Feature::GpuMulticoreSupport),
        layout_mars: info.value(Feature::LayoutMars).unwrap_or(0) > 0,
        xt_top_infrastructure: info.has(Feature::XtTopInfrastructure),
        brn_63553: info.has_quirk(Quirk::Brn63553),
        brn_71242: info.has_quirk(Quirk::Brn71242),
    };
    if !info.has(Feature::Mips) {
        return Err(LoadError("not a MIPS firmware"));
    }
    let num_clusters = info.value(Feature::NumClusters).unwrap_or(1) as u32;

    // The regions: code at heap offset 0, data at the private-data section's fixed offset.
    let code_len = image.region_size(SectionKind::Code);
    let data_len = image.region_size(SectionKind::Data);
    let data_off = image.private_data().ok_or(LoadError("no private data"))?.base_addr
        & mips_boot::HEAP_OFFSET_MASK;
    let code = mem.reserve(0, code_len).ok_or(LoadError("code region"))?;
    let data = mem.reserve(data_off, data_len).ok_or(LoadError("data region"))?;
    for placement in image.placements().map_err(|_| LoadError("elf"))? {
        let p = placement.map_err(|_| LoadError("segment"))?;
        let region = match p.region {
            SectionKind::Code => &code,
            SectionKind::Data => &data,
            _ => return Err(LoadError("coremem segment")),
        };
        if !mem.write(region, p.offset, p.bytes) {
            return Err(LoadError("segment outside region"));
        }
        // The zero tail is the kernel's zero at create: nothing to write.
    }

    // The three boot sections' physical addresses (the remaps) and the boot data.
    let section = |id: SectionId| image.section(id).ok_or(LoadError("layout section"));
    let boot_code = section(SectionId::MipsBootCode)?;
    let exception = section(SectionId::MipsExceptionsCode)?;
    let boot_data_e = section(SectionId::MipsBootData)?;
    let stack = section(SectionId::MipsStack)?;
    let boot = BootSections {
        boot_code_phys: mem
            .phys_at(&code, boot_code.alloc_offset)
            .ok_or(LoadError("boot code phys"))?,
        boot_data_phys: mem
            .phys_at(&data, boot_data_e.alloc_offset)
            .ok_or(LoadError("boot data phys"))?,
        exception_code_phys: mem
            .phys_at(&code, exception.alloc_offset)
            .ok_or(LoadError("exception phys"))?,
    };
    let mut pt_pages = [Page { index: 0, phys: 0 }; PAGE_TABLE_PAGES];
    for p in pt_pages.iter_mut() {
        *p = mem.alloc_page().ok_or(LoadError("page table page"))?;
    }
    let boot_data =
        HeapObj { off: data.off + boot_data_e.alloc_offset, len: boot_data_e.alloc_size };
    let bd = BootData {
        stack_phys: mem.phys_at(&data, stack.alloc_offset).ok_or(LoadError("stack phys"))?,
        reg_base: reg_base_phys,
        pt_phys: [pt_pages[0].phys, pt_pages[1].phys, pt_pages[2].phys, pt_pages[3].phys],
    };
    if !mem.write(&boot_data, 0, &bd.to_bytes()) {
        return Err(LoadError("boot data"));
    }

    // The config slots (fixed offsets) and the structures (first fit).
    // The config slots: reserved by the heap map at creation (first fit never lands there),
    // backed here — one page each (cycle 1 of 2026-10-13 reserved them twice: `Overlap`).
    let conn = mem
        .back_config_slot(mips_boot::CONNECTION_CTL_OFFSET)
        .ok_or(LoadError("connection ctl"))?;
    let osinit = mem.back_config_slot(mips_boot::OSINIT_OFFSET).ok_or(LoadError("osinit"))?;
    let sysinit = mem.back_config_slot(mips_boot::SYSINIT_OFFSET).ok_or(LoadError("sysinit"))?;
    let _ = conn; // all zero: firmware and OS offline until the firmware says otherwise
    let kccb_ctl = obj(mem, CcbCtl::SIZE as u32, "kccb ctl")?;
    let kccb = obj(mem, fwif::kccb::NUM_CMDS * fwif::KccbCmd::SIZE as u32, "kccb")?;
    let kccb_rtn = obj(mem, fwif::kccb::NUM_CMDS * 4, "kccb rtn")?;
    let fwccb_ctl = obj(mem, CcbCtl::SIZE as u32, "fwccb ctl")?;
    let fwccb = obj(mem, fwif::fwccb::NUM_CMDS * fwif::FwccbCmd::SIZE as u32, "fwccb")?;
    let power_sync = obj(mem, 4, "power sync")?;
    let hwrinfo = obj(mem, fwif::HWRINFOBUF_SIZE as u32, "hwr info")?;
    let mmucache_sync = obj(mem, 4, "mmucache sync")?;
    let sysdata = obj(mem, fwif::SYSDATA_SIZE as u32, "sysdata")?;
    let fault_page = mem.alloc_page().ok_or(LoadError("fault page"))?;
    let gpu_util = obj(mem, fwif::GPU_UTIL_FWCB_SIZE as u32, "gpu util")?;
    let runtime_cfg = obj(mem, RuntimeCfg::SIZE as u32, "runtime cfg")?;
    let tracebuf_ctl = obj(mem, fwif::TRACEBUF_SIZE as u32, "tracebuf ctl")?;
    let trace_buf = obj(mem, TRACE_BUF_DEFAULT_DWORDS * 4, "trace buffer")?;
    let osdata = obj(mem, fwif::OSDATA_SIZE as u32, "osdata")?;

    mem.write(
        &kccb_ctl,
        0,
        &CcbCtl::new(fwif::kccb::NUM_CMDS, fwif::KccbCmd::SIZE as u32).to_bytes(),
    );
    mem.write(
        &fwccb_ctl,
        0,
        &CcbCtl::new(fwif::fwccb::NUM_CMDS, fwif::FwccbCmd::SIZE as u32).to_bytes(),
    );
    {
        let bytes = mem.page_bytes_mut(&fault_page);
        for word in bytes.chunks_exact_mut(4) {
            word.copy_from_slice(&fwif::FAULT_PAGE_WORD.to_le_bytes());
        }
        mem.clean_page(&fault_page);
    }
    // The utilisation block starts IDLE (0): the zero page says so. SYS data: config flags 0
    // (the SLC is 16 KiB, above the data-master-overlap floor).
    mem.write(&runtime_cfg, 0, &RuntimeCfg::new(core_clock_hz, num_clusters).to_bytes());
    {
        let mut t = [0u8; fwif::TRACEBUF_SIZE];
        fwif::tracebuf_init(&mut t, trace_buf.fw_addr());
        mem.write(&tracebuf_ctl, 0, &t);
    }
    mem.write_u32(&osdata, fwif::OSDATA_POWER_SYNC_FW_ADDR as u32, power_sync.fw_addr());
    mem.write(
        &osinit,
        0,
        &OsInit::new(&OsInitAddrs {
            kccb_ctl: kccb_ctl.fw_addr(),
            kccb: kccb.fw_addr(),
            kccb_rtn_slots: kccb_rtn.fw_addr(),
            fwccb_ctl: fwccb_ctl.fw_addr(),
            fwccb: fwccb.fw_addr(),
            hwr_info_buf: hwrinfo.fw_addr(),
            os_data: osdata.fw_addr(),
        })
        .to_bytes(),
    );
    mem.write(
        &sysinit,
        0,
        &SysInit::new(&SysInitAddrs {
            fault_page_phys: fault_page.phys,
            runtime_cfg: runtime_cfg.fw_addr(),
            trace_buf_ctl: tracebuf_ctl.fw_addr(),
            sys_data: sysdata.fw_addr(),
            gpu_util_fwcb: gpu_util.fw_addr(),
            core_clock_hz,
        })
        .to_bytes(),
    );

    // The MIPS page table: one uncached entry per mapped heap page (the regions, the slots,
    // the structures); the table pages themselves are reached by their physical addresses in
    // the boot data.
    let entries: alloc::vec::Vec<(u32, u64)> = mem.heap_pages().collect();
    for (heap_page, phys) in entries {
        let (table, index) = mips_boot::pte_slot(heap_page * mips_boot::PAGE);
        mem.page_write_u32(&pt_pages[table], index, mips_boot::pte_uncached(phys));
    }

    Ok(Loaded {
        boot,
        features,
        stream: nexus_gfx::backend::gpu::fwif::StreamFeatures::of(&info),
        num_clusters,
        sysinit,
        osdata,
        sysdata,
        kccb_ctl,
        kccb,
        kccb_rtn,
        fwccb_ctl,
        fwccb,
        mmucache_sync,
        pt_pages,
        boot_data,
    })
}

impl Loaded {
    /// Maps one more heap object into the firmware's page table (an object allocated after the
    /// load: a context, a client command buffer, a timeline word).
    pub(super) fn map_object(&self, mem: &mut FwMemory, obj: &HeapObj) {
        let first = obj.off / mips_boot::PAGE;
        let count = obj.len.div_ceil(mips_boot::PAGE);
        for heap_page in first..first + count {
            if let Some(phys) = mem.phys_at(obj, (heap_page - first) * mips_boot::PAGE) {
                let (table, index) = mips_boot::pte_slot(heap_page * mips_boot::PAGE);
                mem.page_write_u32(&self.pt_pages[table], index, mips_boot::pte_uncached(phys));
            }
        }
    }
}
