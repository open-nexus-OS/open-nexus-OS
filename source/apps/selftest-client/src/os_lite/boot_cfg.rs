// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Runtime boot-configuration reader sourced from QEMU `fw_cfg`.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: Unit tests for mode/profile token parsing
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

use core::cmp::min;
use core::sync::atomic::{AtomicU8, Ordering};

use nexus_abi::{mmio_map, yield_, Handle};

use crate::runtime_mode::{parse_runtime_mode, parse_runtime_profile, RuntimeMode, RuntimeProfile};

pub(crate) const FW_CFG_SLOT: Handle = 0x31;
pub(crate) const FW_CFG_MMIO_VA: usize = 0x2001_0000;
pub(crate) const FW_CFG_DATA: usize = FW_CFG_MMIO_VA;
pub(crate) const FW_CFG_SELECTOR: usize = FW_CFG_MMIO_VA + 8;
pub(crate) const FW_CFG_DMA: usize = FW_CFG_MMIO_VA + 16;
pub(crate) const FW_CFG_FILE_DIR: u16 = 0x19;
pub(crate) const FW_CFG_DMA_CTL_ERROR: u32 = 1 << 0;
pub(crate) const FW_CFG_DMA_CTL_SELECT: u32 = 1 << 3;
pub(crate) const FW_CFG_DMA_CTL_WRITE: u32 = 1 << 4;

const SELFTEST_MODE_FILE_NAME: &[u8] = b"opt/org.open-nexus/selftest-mode";
const SELFTEST_PROFILE_FILE_NAME: &[u8] = b"opt/org.open-nexus/selftest-profile";

const MAP_STATE_UNMAPPED: u8 = 0;
const MAP_STATE_MAPPED: u8 = 1;
const RUNTIME_CFG_RETRY_YIELDS: usize = 8_192;

static FW_CFG_MAP_STATE: AtomicU8 = AtomicU8::new(MAP_STATE_UNMAPPED);

#[must_use]
pub(crate) fn display_bootstrap_enabled() -> bool {
    runtime_mode().is_some()
}

#[must_use]
pub(crate) fn runtime_mode() -> Option<RuntimeMode> {
    let mut buf = [0u8; 32];
    let len = read_named_file(SELFTEST_MODE_FILE_NAME, &mut buf)?;
    parse_runtime_mode(&buf[..len])
}

#[must_use]
pub(crate) fn runtime_mode_with_retry() -> Option<RuntimeMode> {
    retry_runtime_config(runtime_mode)
}

#[must_use]
pub(crate) fn runtime_profile() -> Option<RuntimeProfile> {
    let mut buf = [0u8; 16];
    let len = read_named_file(SELFTEST_PROFILE_FILE_NAME, &mut buf)?;
    parse_runtime_profile(&buf[..len])
}

#[must_use]
pub(crate) fn runtime_profile_with_retry() -> Option<RuntimeProfile> {
    retry_runtime_config(runtime_profile)
}

pub(crate) fn ensure_mapped() -> Result<(), ()> {
    match FW_CFG_MAP_STATE.load(Ordering::Acquire) {
        MAP_STATE_MAPPED => return Ok(()),
        _ => {}
    }
    match mmio_map(FW_CFG_SLOT, FW_CFG_MMIO_VA, 0) {
        Ok(()) => {
            FW_CFG_MAP_STATE.store(MAP_STATE_MAPPED, Ordering::Release);
            Ok(())
        }
        Err(_) => Err(()),
    }
}

#[must_use]
pub(crate) fn fw_cfg_signature_ok() -> bool {
    if ensure_mapped().is_err() {
        return false;
    }
    select_fw_cfg(0);
    let mut sig = [0u8; 4];
    for byte in &mut sig {
        *byte = read_fw_cfg_u8();
    }
    sig == *b"QEMU"
}

#[must_use]
pub(crate) fn find_file_select(target_name: &[u8]) -> Option<(u16, u32)> {
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

pub(crate) fn select_fw_cfg(select: u16) {
    unsafe {
        core::ptr::write_volatile(FW_CFG_SELECTOR as *mut u16, select.to_be());
    }
}

#[must_use]
pub(crate) fn read_fw_cfg_u8() -> u8 {
    unsafe { core::ptr::read_volatile(FW_CFG_DATA as *const u8) }
}

#[must_use]
pub(crate) fn read_fw_cfg_be_u16() -> u16 {
    let b0 = read_fw_cfg_u8();
    let b1 = read_fw_cfg_u8();
    u16::from_be_bytes([b0, b1])
}

#[must_use]
pub(crate) fn read_fw_cfg_be_u32() -> u32 {
    let b0 = read_fw_cfg_u8();
    let b1 = read_fw_cfg_u8();
    let b2 = read_fw_cfg_u8();
    let b3 = read_fw_cfg_u8();
    u32::from_be_bytes([b0, b1, b2, b3])
}

fn read_named_file(target_name: &[u8], out: &mut [u8]) -> Option<usize> {
    let (select, size) = find_file_select(target_name)?;
    let len = min(out.len(), usize::try_from(size).ok()?);
    select_fw_cfg(select);
    for byte in &mut out[..len] {
        *byte = read_fw_cfg_u8();
    }
    Some(len)
}

fn retry_runtime_config<T>(mut read: impl FnMut() -> Option<T>) -> Option<T> {
    for _ in 0..RUNTIME_CFG_RETRY_YIELDS {
        if let Some(value) = read() {
            return Some(value);
        }
        if FW_CFG_MAP_STATE.load(Ordering::Acquire) == MAP_STATE_MAPPED {
            return None;
        }
        let _ = yield_();
    }
    read()
}

#[must_use]
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
