// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: QEMU `ramfb` / `fw_cfg` setup for the service-owned display path.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `fbdevd` host reject tests plus visible-bootstrap QEMU proofs.
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

use crate::error::{FbdevdError, Result};

#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
const RAMFB_FILE_NAME: &[u8] = b"etc/ramfb";
#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
const SELFTEST_MODE_FILE_NAME: &[u8] = b"opt/org.open-nexus/selftest-mode";
const RAMFB_CONFIG_LEN: usize = 28;
const RAMFB_CONFIG_OFFSET: usize = 0;
const DMA_ACCESS_OFFSET: usize = 64;
const DMA_STAGING_LEN: u64 = 4096;
#[cfg(any(test, all(feature = "os-lite", nexus_env = "os", target_os = "none")))]
const DMA_STAGE_MAP_FLAGS: u32 = (1 << 0) | (1 << 1) | (1 << 2) | (1 << 4);
const DRM_FORMAT_ARGB8888: u32 = 0x3432_5241;

#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
use nexus_abi::{
    cap_query, mmio_map, page_flags, vmo_create, vmo_map_page_sys, vmo_write, AbiError, CapQuery,
    Handle,
};

#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
const FW_CFG_SLOT: Handle = 0x31;
#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
const FW_CFG_MMIO_VA: usize = 0x2001_0000;
#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
const FW_CFG_DATA: usize = FW_CFG_MMIO_VA;
#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
const FW_CFG_SELECTOR: usize = FW_CFG_MMIO_VA + 8;
#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
const FW_CFG_DMA: usize = FW_CFG_MMIO_VA + 16;
#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
const FW_CFG_FILE_DIR: u16 = 0x19;
const FW_CFG_DMA_CTL_ERROR: u32 = 1 << 0;
const FW_CFG_DMA_CTL_SELECT: u32 = 1 << 3;
const FW_CFG_DMA_CTL_WRITE: u32 = 1 << 4;
#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
const DMA_VMO_VA: usize = 0x2002_0000;
#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
const DMA_VMO_VA_FALLBACK: usize = 0x2003_0000;
#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
const DMA_VMO_VA_LAST_RESORT: usize = 0x2004_0000;
#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
const DMA_STAGE_VA_CANDIDATES: [usize; 3] =
    [DMA_VMO_VA, DMA_VMO_VA_FALLBACK, DMA_VMO_VA_LAST_RESORT];

#[cfg(any(test, all(feature = "os-lite", nexus_env = "os", target_os = "none")))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use = "dma stage map attempts must drive a concrete decision"]
enum DmaStageMapAttempt {
    Mapped,
    RetryNextCandidate,
    Fatal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use = "validated ramfb file metadata must be used"]
pub struct RamfbFileInfo {
    select: u16,
    size: u32,
}

impl RamfbFileInfo {
    pub const fn select(self) -> u16 {
        self.select
    }

    pub const fn size(self) -> u32 {
        self.size
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use = "validated dma capability metadata must be used"]
pub struct DmaCapabilityInfo {
    base: u64,
    len: u64,
}

impl DmaCapabilityInfo {
    pub const fn config_addr(self) -> u64 {
        self.base.wrapping_add(RAMFB_CONFIG_OFFSET as u64)
    }

    pub const fn descriptor_addr(self) -> u64 {
        self.base.wrapping_add(DMA_ACCESS_OFFSET as u64)
    }

    pub const fn len(self) -> u64 {
        self.len
    }

    pub const fn is_empty(self) -> bool {
        self.len == 0
    }
}

pub fn validate_ramfb_file(select: u16, size: u32) -> Result<RamfbFileInfo> {
    if size < RAMFB_CONFIG_LEN as u32 {
        return Err(FbdevdError::RamfbFileTooSmall);
    }
    Ok(RamfbFileInfo { select, size })
}

pub fn validate_dma_capability(kind_tag: u32, base: u64, len: u64) -> Result<DmaCapabilityInfo> {
    if kind_tag != 1 || len < DMA_STAGING_LEN {
        return Err(FbdevdError::DmaCapInvalid);
    }
    Ok(DmaCapabilityInfo { base, len })
}

pub fn encode_ramfb_config(
    base: u64,
    mode: windowd::VisibleBootstrapMode,
) -> [u8; RAMFB_CONFIG_LEN] {
    let mut config = [0u8; RAMFB_CONFIG_LEN];
    config[0..8].copy_from_slice(&base.to_be_bytes());
    config[8..12].copy_from_slice(&DRM_FORMAT_ARGB8888.to_be_bytes());
    config[12..16].copy_from_slice(&0u32.to_be_bytes());
    config[16..20].copy_from_slice(&mode.width.to_be_bytes());
    config[20..24].copy_from_slice(&mode.height.to_be_bytes());
    config[24..28].copy_from_slice(&mode.stride.to_be_bytes());
    config
}

pub fn require_fw_cfg_signature(signature_ok: bool) -> Result<()> {
    if signature_ok {
        Ok(())
    } else {
        Err(FbdevdError::InvalidRamfbFwCfg)
    }
}

pub fn encode_ramfb_dma_request(select: u16, config_addr: u64) -> [u8; 16] {
    let mut dma_request = [0u8; 16];
    let control = (u32::from(select) << 16) | FW_CFG_DMA_CTL_SELECT | FW_CFG_DMA_CTL_WRITE;
    dma_request[0..4].copy_from_slice(&control.to_be_bytes());
    dma_request[4..8].copy_from_slice(&(RAMFB_CONFIG_LEN as u32).to_be_bytes());
    dma_request[8..16].copy_from_slice(&config_addr.to_be_bytes());
    dma_request
}

pub fn dma_transfer_complete(control: u32) -> Result<bool> {
    if (control & FW_CFG_DMA_CTL_ERROR) != 0 {
        return Err(FbdevdError::DmaDeviceError);
    }
    Ok(control == 0)
}

#[cfg(any(test, all(feature = "os-lite", nexus_env = "os", target_os = "none")))]
#[must_use = "dma stage mapping must resolve to a selected VA or stable failure gate"]
fn resolve_dma_stage_map_candidate<F>(mut probe: F) -> Result<usize>
where
    F: FnMut(usize) -> DmaStageMapAttempt,
{
    #[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
    let candidates = &DMA_STAGE_VA_CANDIDATES[..];
    #[cfg(not(all(feature = "os-lite", nexus_env = "os", target_os = "none")))]
    let candidates = &[0x2002_0000usize, 0x2003_0000usize, 0x2004_0000usize][..];

    for &candidate in candidates {
        match probe(candidate) {
            DmaStageMapAttempt::Mapped => return Ok(candidate),
            DmaStageMapAttempt::RetryNextCandidate => continue,
            DmaStageMapAttempt::Fatal => return Err(FbdevdError::DmaMapPage),
        }
    }
    Err(FbdevdError::DmaMapPage)
}

#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
pub fn display_bootstrap_requested() -> bool {
    find_file_select(SELFTEST_MODE_FILE_NAME).is_some()
}

#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
pub fn configure_ramfb(base: u64, mode: windowd::VisibleBootstrapMode) -> Result<()> {
    let mode = mode.validate().map_err(|_| FbdevdError::InvalidMode)?;
    require_fw_cfg_signature(fw_cfg_signature_ok())?;
    let Some((select, size)) = find_file_select(RAMFB_FILE_NAME) else {
        return Err(FbdevdError::RamfbFileMissing);
    };
    let file = validate_ramfb_file(select, size)?;
    let config = encode_ramfb_config(base, mode);
    let dma_vmo = vmo_create(DMA_STAGING_LEN as usize).map_err(|_| FbdevdError::DmaVmoCreate)?;
    let mut query = CapQuery { kind_tag: 0, reserved: 0, base: 0, len: 0 };
    cap_query(dma_vmo, &mut query).map_err(|_| FbdevdError::DmaCapQuery)?;
    let dma_cap = validate_dma_capability(query.kind_tag, query.base, query.len)?;
    ensure_fw_cfg_mapped()?;
    let flags = page_flags::VALID | page_flags::READ | page_flags::WRITE | page_flags::USER;
    debug_assert_eq!(flags, DMA_STAGE_MAP_FLAGS);
    let dma_stage_va = map_dma_stage_page(dma_vmo, flags)?;
    vmo_write(dma_vmo, RAMFB_CONFIG_OFFSET, &config).map_err(|_| FbdevdError::DmaConfigWrite)?;
    let dma_request = encode_ramfb_dma_request(file.select(), dma_cap.config_addr());
    vmo_write(dma_vmo, DMA_ACCESS_OFFSET, &dma_request)
        .map_err(|_| FbdevdError::DmaDescriptorWrite)?;
    unsafe {
        core::ptr::write_volatile(FW_CFG_DMA as *mut u64, dma_cap.descriptor_addr().to_be());
    }
    let mut polls = 0usize;
    let dma_control_ptr = (dma_stage_va + DMA_ACCESS_OFFSET) as *const u32;
    loop {
        let control = unsafe { core::ptr::read_volatile(dma_control_ptr).to_be() };
        if dma_transfer_complete(control)? {
            break;
        }
        polls = polls.saturating_add(1);
        if polls > 100_000 {
            return Err(FbdevdError::DmaTimeout);
        }
        let _ = nexus_abi::yield_();
    }
    Ok(())
}

#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
fn fw_cfg_signature_ok() -> bool {
    if ensure_fw_cfg_mapped().is_err() {
        return false;
    }
    select_fw_cfg(0);
    let mut sig = [0u8; 4];
    for byte in &mut sig {
        *byte = read_fw_cfg_u8();
    }
    sig == *b"QEMU"
}

#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
fn ensure_fw_cfg_mapped() -> Result<()> {
    match mmio_map(FW_CFG_SLOT, FW_CFG_MMIO_VA, 0) {
        Ok(()) | Err(AbiError::InvalidArgument) => Ok(()),
        Err(_) => Err(FbdevdError::FwCfgMap),
    }
}

#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
fn map_dma_stage_page(dma_vmo: Handle, flags: u32) -> Result<usize> {
    resolve_dma_stage_map_candidate(|candidate| {
        match vmo_map_page_sys(dma_vmo, candidate, 0, flags) {
            Ok(()) => DmaStageMapAttempt::Mapped,
            Err(AbiError::InvalidArgument) => DmaStageMapAttempt::RetryNextCandidate,
            Err(_) => DmaStageMapAttempt::Fatal,
        }
    })
}

#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
fn find_file_select(target_name: &[u8]) -> Option<(u16, u32)> {
    if !fw_cfg_signature_ok() {
        return None;
    }
    select_fw_cfg(FW_CFG_FILE_DIR);
    let count = read_fw_cfg_be_u32();
    if count > 128 {
        return None;
    }
    for _ in 0..count {
        let size = read_fw_cfg_be_u32();
        let select = read_fw_cfg_be_u16();
        let _reserved = read_fw_cfg_be_u16();
        let mut name = [0u8; 56];
        for byte in &mut name {
            *byte = read_fw_cfg_u8();
        }
        if name_matches(&name, target_name) {
            return Some((select, size));
        }
    }
    None
}

#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
fn select_fw_cfg(select: u16) {
    unsafe {
        core::ptr::write_volatile(FW_CFG_SELECTOR as *mut u16, select.to_be());
    }
}

#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
fn read_fw_cfg_u8() -> u8 {
    unsafe { core::ptr::read_volatile(FW_CFG_DATA as *const u8) }
}

#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
fn read_fw_cfg_be_u16() -> u16 {
    let b0 = read_fw_cfg_u8();
    let b1 = read_fw_cfg_u8();
    u16::from_be_bytes([b0, b1])
}

#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
fn read_fw_cfg_be_u32() -> u32 {
    let b0 = read_fw_cfg_u8();
    let b1 = read_fw_cfg_u8();
    let b2 = read_fw_cfg_u8();
    let b3 = read_fw_cfg_u8();
    u32::from_be_bytes([b0, b1, b2, b3])
}

#[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
fn name_matches(actual: &[u8; 56], expected: &[u8]) -> bool {
    let expected_len = expected.len();
    if expected_len > actual.len() {
        return false;
    }
    actual[..expected_len] == expected[..]
        && match actual.get(expected_len).copied() {
            Some(byte) => matches!(byte, 0 | b' ' | b'\n' | b'\r' | b'\t'),
            None => true,
        }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{vec, vec::Vec};

    #[test]
    fn test_reject_ramfb_file_too_small() {
        assert_eq!(
            validate_ramfb_file(0x20, (RAMFB_CONFIG_LEN - 1) as u32),
            Err(FbdevdError::RamfbFileTooSmall)
        );
    }

    #[test]
    fn test_reject_invalid_dma_capability() {
        assert_eq!(validate_dma_capability(0, 0x1234, 4096), Err(FbdevdError::DmaCapInvalid));
        assert_eq!(validate_dma_capability(1, 0x1234, 4095), Err(FbdevdError::DmaCapInvalid));
    }

    #[test]
    fn dma_request_layout_matches_fw_cfg_dma_contract() {
        let request = encode_ramfb_dma_request(0x29, 0x1234_5678_9000_abcd);

        assert_eq!(
            &request[0..4],
            &((0x29u32 << 16) | FW_CFG_DMA_CTL_SELECT | FW_CFG_DMA_CTL_WRITE).to_be_bytes()
        );
        assert_eq!(&request[4..8], &(RAMFB_CONFIG_LEN as u32).to_be_bytes());
        assert_eq!(&request[8..16], &0x1234_5678_9000_abcdu64.to_be_bytes());
    }

    #[test]
    fn dma_transfer_state_is_success_only_on_zero_control() {
        assert_eq!(dma_transfer_complete(0), Ok(true));
        assert_eq!(dma_transfer_complete(FW_CFG_DMA_CTL_SELECT), Ok(false));
        assert_eq!(dma_transfer_complete(FW_CFG_DMA_CTL_ERROR), Err(FbdevdError::DmaDeviceError));
    }

    #[test]
    fn dma_stage_map_flags_include_valid_user_rw_without_execute() {
        const VALID: u32 = 1 << 0;
        const READ: u32 = 1 << 1;
        const WRITE: u32 = 1 << 2;
        const EXECUTE: u32 = 1 << 3;
        const USER: u32 = 1 << 4;

        assert_ne!(DMA_STAGE_MAP_FLAGS & VALID, 0);
        assert_ne!(DMA_STAGE_MAP_FLAGS & READ, 0);
        assert_ne!(DMA_STAGE_MAP_FLAGS & WRITE, 0);
        assert_ne!(DMA_STAGE_MAP_FLAGS & USER, 0);
        assert_eq!(DMA_STAGE_MAP_FLAGS & EXECUTE, 0);
    }

    #[test]
    fn dma_stage_map_retries_until_candidate_succeeds() {
        let mut seen = Vec::new();
        let selected = resolve_dma_stage_map_candidate(|candidate| {
            seen.push(candidate);
            if seen.len() < 3 {
                DmaStageMapAttempt::RetryNextCandidate
            } else {
                DmaStageMapAttempt::Mapped
            }
        });

        assert_eq!(selected, Ok(0x2004_0000));
        assert_eq!(seen, vec![0x2002_0000, 0x2003_0000, 0x2004_0000]);
    }

    #[test]
    fn dma_stage_map_fails_after_all_candidates_reject() {
        let mut attempts = 0usize;
        let result = resolve_dma_stage_map_candidate(|_| {
            attempts += 1;
            DmaStageMapAttempt::RetryNextCandidate
        });

        assert_eq!(result, Err(FbdevdError::DmaMapPage));
        assert_eq!(attempts, 3);
    }

    #[test]
    fn dma_stage_map_stops_on_first_fatal_error() {
        let mut attempts = 0usize;
        let result = resolve_dma_stage_map_candidate(|candidate| {
            attempts += 1;
            match candidate {
                0x2002_0000 => DmaStageMapAttempt::RetryNextCandidate,
                _ => DmaStageMapAttempt::Fatal,
            }
        });

        assert_eq!(result, Err(FbdevdError::DmaMapPage));
        assert_eq!(attempts, 2);
    }
}
