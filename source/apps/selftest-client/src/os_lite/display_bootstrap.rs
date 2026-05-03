// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Visible QEMU `ramfb` bootstrap path for TASK-0055B/TASK-0055C/TASK-0056B.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU marker ladder plus `windowd`/`ui_windowd_host` host reject tests.
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

use nexus_abi::{
    cap_query, mmio_map, nsec, page_flags, vmo_create, vmo_map_page, vmo_write, yield_, CapQuery,
    Handle,
};

const FW_CFG_SLOT: Handle = 0x31;
const FW_CFG_MMIO_VA: usize = 0x2001_0000;
const DMA_VMO_VA: usize = 0x2002_0000;
const PAGE_SIZE: usize = 4096;

const FW_CFG_DATA: usize = FW_CFG_MMIO_VA;
const FW_CFG_SELECTOR: usize = FW_CFG_MMIO_VA + 8;
const FW_CFG_DMA: usize = FW_CFG_MMIO_VA + 16;

const FW_CFG_FILE_DIR: u16 = 0x19;
const FW_CFG_DMA_CTL_ERROR: u32 = 1 << 0;
const FW_CFG_DMA_CTL_SELECT: u32 = 1 << 3;
const FW_CFG_DMA_CTL_WRITE: u32 = 1 << 4;

const RAMFB_FILE_NAME: &[u8] = b"etc/ramfb";
const RAMFB_CONFIG_LEN: usize = 28;
const RAMFB_CONFIG_OFFSET: usize = 0;
const DMA_ACCESS_OFFSET: usize = 64;
const DRM_FORMAT_ARGB8888: u32 = 0x3432_5241; // "AR24"
const DISPLAY_SETTLE_NS: u64 = 750_000_000;
const DISPLAY_SETTLE_MAX_YIELDS: usize = 200_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BootstrapFailure {
    WindowdEvidence,
    VisibleInputEvidence,
    InvalidMode,
    FramebufferVmo,
    InvalidFramebufferCap,
    InvalidDisplayCapability,
    FrameWrite,
    FwCfgMap,
    FwCfgSignature,
    RamfbFileMissing,
    DmaVmo,
    InvalidDmaCap,
    DmaFailed,
}

pub(crate) struct BootstrapEvidence {
    pub(crate) systemui: windowd::VisibleSystemUiEvidence,
    pub(crate) visible_input: windowd::UiVisibleInputEvidence,
}

pub(crate) fn enabled() -> bool {
    option_env!("NEXUS_DISPLAY_BOOTSTRAP") == Some("1")
}

pub(crate) fn run() -> Option<BootstrapEvidence> {
    run_result().ok()
}

pub(crate) fn run_result() -> Result<BootstrapEvidence, BootstrapFailure> {
    let evidence =
        windowd::run_visible_systemui_smoke().map_err(|_| BootstrapFailure::WindowdEvidence)?;
    let mode = evidence.mode.validate().map_err(|_| BootstrapFailure::InvalidMode)?;
    let fb_len = mode.byte_len().map_err(|_| BootstrapFailure::InvalidMode)?;
    let framebuffer = vmo_create(fb_len).map_err(|_| BootstrapFailure::FramebufferVmo)?;
    let fb_query = query_cap(framebuffer).ok_or(BootstrapFailure::InvalidFramebufferCap)?;
    if fb_query.kind_tag != 1 || fb_query.len < fb_len as u64 {
        return Err(BootstrapFailure::InvalidFramebufferCap);
    }

    let cap = windowd::VisibleDisplayCapability { byte_len: fb_len, mapped: true, writable: true };
    windowd::validate_visible_bootstrap_capability(mode, cap)
        .map_err(|_| BootstrapFailure::InvalidDisplayCapability)?;
    write_windowd_composed_rows(framebuffer, mode, &evidence)?;
    configure_ramfb(fb_query.base, mode)?;
    settle_visible_display();
    let systemui = windowd::visible_systemui_marker_postflight_ready(Some(evidence))
        .map_err(|_| BootstrapFailure::WindowdEvidence)?;
    let visible_input =
        windowd::run_visible_input_smoke().map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    let visible_input = windowd::visible_input_marker_postflight_ready(Some(visible_input))
        .map_err(|_| BootstrapFailure::VisibleInputEvidence)?;
    write_windowd_visible_cursor_rows(framebuffer, mode, &visible_input)?;
    settle_visible_display();
    write_windowd_visible_hover_rows(framebuffer, mode, &visible_input)?;
    settle_visible_display();
    write_windowd_visible_input_rows(framebuffer, mode, &visible_input)?;
    settle_visible_display();
    Ok(BootstrapEvidence { systemui, visible_input })
}

fn settle_visible_display() {
    let start = nsec().ok();
    for _ in 0..DISPLAY_SETTLE_MAX_YIELDS {
        if let (Some(start), Ok(now)) = (start, nsec()) {
            if now.saturating_sub(start) >= DISPLAY_SETTLE_NS {
                break;
            }
        }
        let _ = yield_();
    }
}

fn query_cap(handle: Handle) -> Option<CapQuery> {
    let mut query = CapQuery { kind_tag: 0, reserved: 0, base: 0, len: 0 };
    cap_query(handle, &mut query).ok()?;
    Some(query)
}

fn write_windowd_composed_rows(
    handle: Handle,
    mode: windowd::VisibleBootstrapMode,
    evidence: &windowd::VisibleSystemUiEvidence,
) -> Result<(), BootstrapFailure> {
    let row_len = mode.stride as usize;
    let mut row = [0u8; windowd::VISIBLE_BOOTSTRAP_WIDTH as usize * 4];
    for y in 0..mode.height {
        evidence.copy_composed_row(y, &mut row).map_err(|_| BootstrapFailure::FrameWrite)?;
        let offset = y as usize * row_len;
        vmo_write(handle, offset, &row[..row_len]).map_err(|_| BootstrapFailure::FrameWrite)?;
    }
    Ok(())
}

fn write_windowd_visible_input_rows(
    handle: Handle,
    mode: windowd::VisibleBootstrapMode,
    evidence: &windowd::UiVisibleInputEvidence,
) -> Result<(), BootstrapFailure> {
    let row_len = mode.stride as usize;
    let mut row = [0u8; windowd::VISIBLE_BOOTSTRAP_WIDTH as usize * 4];
    for y in 0..mode.height {
        evidence.copy_composed_row(mode, y, &mut row).map_err(|_| BootstrapFailure::FrameWrite)?;
        let offset = y as usize * row_len;
        vmo_write(handle, offset, &row[..row_len]).map_err(|_| BootstrapFailure::FrameWrite)?;
    }
    Ok(())
}

fn write_windowd_visible_cursor_rows(
    handle: Handle,
    mode: windowd::VisibleBootstrapMode,
    evidence: &windowd::UiVisibleInputEvidence,
) -> Result<(), BootstrapFailure> {
    let row_len = mode.stride as usize;
    let mut row = [0u8; windowd::VISIBLE_BOOTSTRAP_WIDTH as usize * 4];
    for y in 0..mode.height {
        evidence.copy_cursor_row(mode, y, &mut row).map_err(|_| BootstrapFailure::FrameWrite)?;
        let offset = y as usize * row_len;
        vmo_write(handle, offset, &row[..row_len]).map_err(|_| BootstrapFailure::FrameWrite)?;
    }
    Ok(())
}

fn write_windowd_visible_hover_rows(
    handle: Handle,
    mode: windowd::VisibleBootstrapMode,
    evidence: &windowd::UiVisibleInputEvidence,
) -> Result<(), BootstrapFailure> {
    let row_len = mode.stride as usize;
    let mut row = [0u8; windowd::VISIBLE_BOOTSTRAP_WIDTH as usize * 4];
    for y in 0..mode.height {
        evidence.copy_hover_row(mode, y, &mut row).map_err(|_| BootstrapFailure::FrameWrite)?;
        let offset = y as usize * row_len;
        vmo_write(handle, offset, &row[..row_len]).map_err(|_| BootstrapFailure::FrameWrite)?;
    }
    Ok(())
}

fn configure_ramfb(
    fb_base: u64,
    mode: windowd::VisibleBootstrapMode,
) -> Result<(), BootstrapFailure> {
    mmio_map(FW_CFG_SLOT, FW_CFG_MMIO_VA, 0).map_err(|_| BootstrapFailure::FwCfgMap)?;
    if !fw_cfg_signature_ok() {
        return Err(BootstrapFailure::FwCfgSignature);
    }
    let select = find_ramfb_file_select().ok_or(BootstrapFailure::RamfbFileMissing)?;
    let dma_vmo = vmo_create(PAGE_SIZE).map_err(|_| BootstrapFailure::DmaVmo)?;
    vmo_map_page(
        dma_vmo,
        DMA_VMO_VA,
        0,
        page_flags::VALID | page_flags::READ | page_flags::WRITE | page_flags::USER,
    )
    .map_err(|_| BootstrapFailure::DmaVmo)?;
    let dma_query = query_cap(dma_vmo).ok_or(BootstrapFailure::InvalidDmaCap)?;
    if dma_query.kind_tag != 1 || dma_query.len < PAGE_SIZE as u64 {
        return Err(BootstrapFailure::InvalidDmaCap);
    }

    // QEMU's etc/ramfb ABI is addr, fourcc, flags, width, height, stride.
    let mut cfg = [0u8; RAMFB_CONFIG_LEN];
    write_be_u64(&mut cfg[0..8], fb_base);
    write_be_u32(&mut cfg[8..12], DRM_FORMAT_ARGB8888);
    write_be_u32(&mut cfg[12..16], 0);
    write_be_u32(&mut cfg[16..20], mode.width);
    write_be_u32(&mut cfg[20..24], mode.height);
    write_be_u32(&mut cfg[24..28], mode.stride);
    vmo_write(dma_vmo, RAMFB_CONFIG_OFFSET, &cfg).map_err(|_| BootstrapFailure::DmaVmo)?;

    let mut access = [0u8; 16];
    let control = ((select as u32) << 16) | FW_CFG_DMA_CTL_SELECT | FW_CFG_DMA_CTL_WRITE;
    write_be_u32(&mut access[0..4], control);
    write_be_u32(&mut access[4..8], RAMFB_CONFIG_LEN as u32);
    write_be_u64(&mut access[8..16], dma_query.base + RAMFB_CONFIG_OFFSET as u64);
    vmo_write(dma_vmo, DMA_ACCESS_OFFSET, &access).map_err(|_| BootstrapFailure::DmaVmo)?;

    let dma_access_pa = dma_query.base + DMA_ACCESS_OFFSET as u64;
    trigger_dma(dma_access_pa);
    wait_dma_complete().then_some(()).ok_or(BootstrapFailure::DmaFailed)
}

fn fw_cfg_signature_ok() -> bool {
    select_fw_cfg(0);
    let mut sig = [0u8; 4];
    for byte in &mut sig {
        *byte = read_fw_cfg_u8();
    }
    &sig == b"QEMU"
}

fn find_ramfb_file_select() -> Option<u16> {
    select_fw_cfg(FW_CFG_FILE_DIR);
    let count = read_fw_cfg_be_u32();
    if count > 128 {
        return None;
    }
    for _ in 0..count {
        let _size = read_fw_cfg_be_u32();
        let select = read_fw_cfg_be_u16();
        let _reserved = read_fw_cfg_be_u16();
        let mut name = [0u8; 56];
        for byte in &mut name {
            *byte = read_fw_cfg_u8();
        }
        if name_matches(&name, RAMFB_FILE_NAME) {
            return Some(select);
        }
    }
    None
}

fn name_matches(name: &[u8; 56], expected: &[u8]) -> bool {
    name.starts_with(expected) && name.get(expected.len()).copied().unwrap_or(1) == 0
}

fn wait_dma_complete() -> bool {
    for _ in 0..100_000 {
        let control = unsafe {
            core::ptr::read_volatile((DMA_VMO_VA + DMA_ACCESS_OFFSET) as *const u32).to_be()
        };
        if control == 0 {
            return true;
        }
        if (control & FW_CFG_DMA_CTL_ERROR) != 0 {
            return false;
        }
    }
    false
}

fn select_fw_cfg(select: u16) {
    unsafe {
        core::ptr::write_volatile(FW_CFG_SELECTOR as *mut u16, select.to_be());
    }
}

fn read_fw_cfg_u8() -> u8 {
    unsafe { core::ptr::read_volatile(FW_CFG_DATA as *const u8) }
}

fn read_fw_cfg_be_u16() -> u16 {
    let b0 = read_fw_cfg_u8();
    let b1 = read_fw_cfg_u8();
    u16::from_be_bytes([b0, b1])
}

fn read_fw_cfg_be_u32() -> u32 {
    let b0 = read_fw_cfg_u8();
    let b1 = read_fw_cfg_u8();
    let b2 = read_fw_cfg_u8();
    let b3 = read_fw_cfg_u8();
    u32::from_be_bytes([b0, b1, b2, b3])
}

fn trigger_dma(addr: u64) {
    unsafe {
        core::ptr::write_volatile(FW_CFG_DMA as *mut u32, ((addr >> 32) as u32).to_be());
        core::ptr::write_volatile((FW_CFG_DMA + 4) as *mut u32, (addr as u32).to_be());
    }
}

fn write_be_u32(dst: &mut [u8], value: u32) {
    dst.copy_from_slice(&value.to_be_bytes());
}

fn write_be_u64(dst: &mut [u8], value: u64) {
    dst.copy_from_slice(&value.to_be_bytes());
}
