// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The GPU host half against the published firmware blob and the stock system's live
//! registers (TASK-0329 P1, RFC-0101 §8b): the blob in `resources/firmware/gpu/` parses with the
//! header the provenance names, its layout is the six-section MIPS layout, its device-info
//! block says what the G0 research read, its segments land inside the code and data regions;
//! the start sequence for that core writes the words the stock dump holds. If this test moves,
//! either the blob changed (the provenance gate catches a hash change first), the interface
//! moved, or the model did — never silently.
//! OWNERS: @gpu
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: this file

use nexus_gfx::backend::gpu::fw_image::{
    Feature, FirmwareImage, FwImageError, Quirk, SectionId, SectionKind,
};
use nexus_gfx::backend::gpu::mips_boot::{self, BootSections, HeapMap};
use nexus_gfx::backend::gpu::start::{self, cr, CoreFeatures, Recorder};
use nexus_gfx::backend::gpu::{fwif, mmu, CoreId};

const BLOB: &[u8] = include_bytes!("../../../resources/firmware/gpu/rogue_36.29.52.182_v1.fw");

#[test]
fn the_published_blob_is_the_one_the_provenance_names() {
    assert_eq!(BLOB.len(), 126_976, "31 blocks of 4 KiB");
    let img =
        FirmwareImage::parse(BLOB, CoreId::BOARD).expect("the blob parses for the board's core");
    let h = img.header();
    assert_eq!(h.core, CoreId::BOARD);
    assert_eq!((h.version_major, h.version_minor, h.version_build), (1, 1, 6_976_702));
    assert_eq!(h.fw_page_size, 4096);
    assert_eq!(h.flags & 1, 1, "the open-source build");
    assert_eq!(h.layout_entry_num, 6);
    assert_eq!(h.device_info_size, 4096);
}

#[test]
fn the_layout_is_the_six_section_mips_layout() {
    let img = FirmwareImage::parse(BLOB, CoreId::BOARD).unwrap();
    let e = |id| *img.section(id).unwrap();
    let code = e(SectionId::MipsCode);
    assert_eq!(
        (code.kind, code.base_addr, code.max_size, code.alloc_size, code.alloc_offset),
        (Some(SectionKind::Code), 0xC000_0000, 0x2F000, 0x17000, 0)
    );
    let exc = e(SectionId::MipsExceptionsCode);
    assert_eq!((exc.base_addr, exc.alloc_size, exc.alloc_offset), (0x9FC0_2000, 0x1000, 0x17000));
    let boot = e(SectionId::MipsBootCode);
    assert_eq!(
        (boot.base_addr, boot.alloc_size, boot.alloc_offset),
        (0xBFC0_0000, 0x1000, 0x18000)
    );
    let data = e(SectionId::MipsPrivateData);
    assert_eq!(
        (data.kind, data.base_addr, data.alloc_size, data.alloc_offset),
        (Some(SectionKind::Data), 0xC003_2000, 0x3000, 0)
    );
    let bd = e(SectionId::MipsBootData);
    assert_eq!((bd.base_addr, bd.alloc_size, bd.alloc_offset), (0xBFC0_1000, 0x1000, 0x3000));
    let stack = e(SectionId::MipsStack);
    assert_eq!(
        (stack.base_addr, stack.alloc_size, stack.alloc_offset),
        (0xCF60_0000, 0x1000, 0x4000)
    );
    assert_eq!(img.region_size(SectionKind::Code), 0x19000);
    assert_eq!(img.region_size(SectionKind::Data), 0x5000);
    assert_eq!(img.private_data().map(|e| e.id), Some(SectionId::MipsPrivateData));
    // The data region's fixed heap offset: the private-data base masked to the heap.
    assert_eq!(data.base_addr & mips_boot::HEAP_OFFSET_MASK, 0x32000);
}

#[test]
fn the_device_info_block_says_what_the_research_read() {
    let img = FirmwareImage::parse(BLOB, CoreId::BOARD).unwrap();
    let d = img.device_info();
    assert!(d.has(Feature::Mips));
    assert!(!d.has(Feature::Meta));
    assert!(!d.has(Feature::RiscvFwProcessor));
    assert_eq!(d.value(Feature::LayoutMars), Some(1));
    assert_eq!(d.value(Feature::NumClusters), Some(1));
    assert_eq!(d.value(Feature::PhysBusWidth), Some(36));
    assert_eq!(d.value(Feature::VirtualAddressSpaceBits), Some(40));
    assert_eq!(d.value(Feature::SlcSizeInKilobytes), Some(16));
    assert_eq!(d.value(Feature::SlcCacheLineSizeBits), Some(512));
    assert_eq!(d.value(Feature::TileSizeX), Some(16));
    assert_eq!(d.value(Feature::TileSizeY), Some(16));
    assert_eq!(d.value(Feature::IspMaxTilesInFlight), Some(2));
    assert_eq!(d.value(Feature::NumIspIppPipes), Some(2));
    assert_eq!(d.value(Feature::MaxPartitions), Some(8));
    assert_eq!(d.value(Feature::CommonStoreSizeInDwords), Some(13312));
    assert_eq!(d.value(Feature::XpuMaxSlaves), Some(3));
    assert_eq!(d.value(Feature::XeArchitecture), Some(1));
    assert!(d.has(Feature::Pbe2InXe));
    assert!(d.has(Feature::XeTpu2));
    assert!(d.has(Feature::SysBusSecureReset));
    assert!(d.has(Feature::PbvncCoreidReg));
    assert!(d.has(Feature::IrqPerOs));
    assert!(d.has(Feature::Compute));
    assert!(d.has(Feature::GpuMulticoreSupport));
    assert!(d.has(Feature::XeMemoryHierarchy));
    assert!(d.has(Feature::Roguexe));
    assert!(!d.has(Feature::WatchdogTimer), "no safety events on this core");
    assert!(!d.has(Feature::EccRams));
    assert!(!d.has(Feature::TpuDmGlobalRegisters));
    assert!(!d.has(Feature::XtTopInfrastructure));
    assert!(d.has_quirk(Quirk::Brn63553));
    assert!(!d.has_quirk(Quirk::Brn71242));
    assert!(!d.has_quirk(Quirk::Brn44079));
    assert!(!d.unknown_feature_bits(), "a newer blob would say so here first");
    // The start sequence's view of the core, derived from the blob, equals the constant.
    let f = CoreFeatures {
        pbe2_in_xe: d.has(Feature::Pbe2InXe),
        xe_tpu2: d.has(Feature::XeTpu2),
        sys_bus_secure_reset: d.has(Feature::SysBusSecureReset),
        slc_cache_line_size_bits: d.value(Feature::SlcCacheLineSizeBits).unwrap_or(0) as u32,
        gpu_multicore_support: d.has(Feature::GpuMulticoreSupport),
        layout_mars: d.value(Feature::LayoutMars).unwrap_or(0) > 0,
        xt_top_infrastructure: d.has(Feature::XtTopInfrastructure),
        brn_63553: d.has_quirk(Quirk::Brn63553),
        brn_71242: d.has_quirk(Quirk::Brn71242),
    };
    assert_eq!(f, CoreFeatures::BOARD);
}

#[test]
fn every_segment_lands_inside_its_region() {
    let img = FirmwareImage::parse(BLOB, CoreId::BOARD).unwrap();
    let code = img.region_size(SectionKind::Code);
    let data = img.region_size(SectionKind::Data);
    let mut code_bytes = 0u32;
    let mut data_bytes = 0u32;
    let mut n = 0;
    for p in img.placements().unwrap() {
        let p = p.expect("every loadable segment lies in a section");
        let end = p.offset + p.bytes.len() as u32 + p.zero_tail;
        match p.region {
            SectionKind::Code => {
                assert!(
                    end <= code,
                    "code segment at {:#x}+{:#x} leaves the region",
                    p.offset,
                    end - p.offset
                );
                code_bytes += p.bytes.len() as u32;
            }
            SectionKind::Data => {
                assert!(
                    end <= data,
                    "data segment at {:#x}+{:#x} leaves the region",
                    p.offset,
                    end - p.offset
                );
                data_bytes += p.bytes.len() as u32;
            }
            other => panic!("a MIPS image has no {other:?} segment"),
        }
        n += 1;
    }
    assert!(n >= 3, "code, exceptions, boot code at least: {n}");
    assert!(code_bytes > 0 && data_bytes > 0);
}

#[test]
fn test_reject_the_blob_for_another_core_and_a_patched_trailer() {
    let other = CoreId::new(36, 52, 104, 182);
    assert_eq!(
        FirmwareImage::parse(BLOB, other).err(),
        Some(FwImageError::WrongId { expected: other, found: CoreId::BOARD })
    );
    let mut patched = BLOB.to_vec();
    let h = patched.len() - 4096;
    patched[h + 28] &= !1; // the open-source flag
    assert_eq!(
        FirmwareImage::parse(&patched, CoreId::BOARD).err(),
        Some(FwImageError::NotOpenBuild)
    );
    let truncated = &BLOB[..BLOB.len() - 1];
    assert_eq!(
        FirmwareImage::parse(truncated, CoreId::BOARD).err(),
        Some(FwImageError::NotBlockAligned)
    );
}

#[test]
fn the_heap_map_places_the_blob_as_the_open_driver_does() {
    let img = FirmwareImage::parse(BLOB, CoreId::BOARD).unwrap();
    let mut heap = HeapMap::new();
    heap.reserve(0, img.region_size(SectionKind::Code)).unwrap();
    let data_off = img.private_data().unwrap().base_addr & mips_boot::HEAP_OFFSET_MASK;
    heap.reserve(data_off, img.region_size(SectionKind::Data)).unwrap();
    // The first allocation after code and data: the kernel command buffer's control block.
    let ctl = heap.alloc(fwif::CcbCtl::new(128, 88).cmd_size.max(144)).unwrap();
    assert_eq!(ctl, 0x19000);
    assert_eq!(mips_boot::fw_addr(ctl), 0xC001_9000);
    assert_eq!(mips_boot::gpu_addr(mips_boot::SYSINIT_OFFSET), 0xE1_C0FF_0000);
}

#[test]
fn the_start_sequence_for_the_blobs_core_reproduces_the_stock_words() {
    // The stock dump: SLC_CTRL_MISC 0x200003 before our write; the remaps with the vendor's
    // physical pages; the wrapper 0x1CF80.
    let boot = BootSections {
        boot_code_phys: 0x8979_8000,
        boot_data_phys: 0x8BDC_8000,
        exception_code_phys: 0x878F_7000,
    };
    let mut r = Recorder::answering(&[(cr::SLC_CTRL_MISC, 0x0020_0003)]);
    start::start(&mut r, &boot, &CoreFeatures::BOARD);
    assert_eq!(r.written(cr::SLC_CTRL_MISC), Some(0x0020_0003));
    assert_eq!(r.written(cr::MIPS_WRAPPER_CONFIG), Some(0x0001_CF80));
    assert_eq!(r.written(cr::MIPS_ADDR_REMAP1_CONFIG1), Some(0x1FC0_0001));
    assert_eq!(r.written(cr::MIPS_ADDR_REMAP1_CONFIG2), Some(0x8979_800C));
    assert_eq!(r.written(cr::MIPS_ADDR_REMAP2_CONFIG1), Some(0x1FC0_1001));
    assert_eq!(r.written(cr::MIPS_ADDR_REMAP2_CONFIG2), Some(0x8BDC_800C));
    assert_eq!(r.written(cr::MIPS_ADDR_REMAP3_CONFIG1), Some(0x1FC0_2001));
    assert_eq!(r.written(cr::MIPS_ADDR_REMAP3_CONFIG2), Some(0x878F_700C));
    assert_eq!(r.written(cr::MIPS_ADDR_REMAP5_CONFIG1), Some(0x0000_0001));
    assert_eq!(r.written(cr::MIPS_ADDR_REMAP5_CONFIG2), Some(0x8979_800C));
    assert_eq!(r.written(cr::MTS_GARTEN_WRAPPER_CONFIG), Some(0));
    assert_eq!(r.written(cr::MIPS_DEBUG_CONFIG), Some(0));
    assert!(
        cr::MULTICORE_SYSTEM < cr::WINDOW_LEN,
        "the highest register used fits the granted window"
    );
}

#[test]
fn the_firmware_heap_pages_encode_for_a_36_bit_bus() {
    // The vendor's own boot code page from the dump, as the firmware page table would hold it.
    let e = mips_boot::pte_uncached(0x8979_8000);
    assert_eq!(e, ((0x8979_8u32) << 6) | 0x17);
    // The GPU MMU's view of the same page, SLC-bypassed and firmware-protected.
    let flags = mmu::PageFlags { read_only: false, slc_bypass: true, pm_fw_protect: true };
    assert_eq!(mmu::pt_entry(0x8979_8000, flags), 0x4000_0000_8979_8009);
}
