// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: MMIO selftest helpers — the RFC-0085 `vm_map` roundtrip proof and
//!   the `cap_query` probes consumed by `phases::mmio`, plus the `MmioBus`
//!   adapter used by the opt-in `smoltcp-probe` bring-up lane.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os) — mmio phase.
//!
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

/// MMIO bus adapter for the bounded smoltcp-over-virtio bring-up probe.
/// `smoltcp-probe` is off by default (see Cargo.toml), and `net/smoltcp_probe.rs`
/// is its only consumer — so the adapter is gated with it rather than left to
/// warn as never-constructed in every default build.
#[cfg(feature = "smoltcp-probe")]
pub(crate) struct MmioBus {
    pub(crate) base: usize,
}

#[cfg(feature = "smoltcp-probe")]
impl nexus_hal::Bus for MmioBus {
    fn read(&self, addr: usize) -> u32 {
        unsafe { core::ptr::read_volatile((self.base + addr) as *const u32) }
    }
    fn write(&self, addr: usize, value: u32) {
        unsafe { core::ptr::write_volatile((self.base + addr) as *mut u32, value) }
    }
}

/// RFC-0085 vm_map roundtrip — the userspace end-to-end proof of the
/// kernel-chosen-VA path: map a whole VMO in ONE syscall, write/read through
/// the mapping, destroy-while-mapped must refuse (EBUSY), unmap, and an
/// equal re-map must return the SAME va (first-fit reuse).
pub(crate) fn vm_map_roundtrip_probe() -> core::result::Result<(), ()> {
    use nexus_abi::{page_flags, AbiError};
    const LEN: usize = 4 * 4096;
    let vmo = nexus_abi::vmo_create(LEN).map_err(|_| ())?;
    nexus_abi::vmo_write(vmo, 0, &[0xA5]).map_err(|_| ())?;
    let flags = page_flags::VALID | page_flags::READ | page_flags::WRITE | page_flags::USER;
    let va = nexus_abi::vm_map(vmo, 0, LEN, flags).map_err(|_| ())?;
    let seeded = unsafe { core::ptr::read_volatile(va as *const u8) } == 0xA5;
    unsafe { core::ptr::write_volatile((va + LEN - 1) as *mut u8, 0x5A) };
    let mut back = [0u8; 1];
    nexus_abi::vmo_read(vmo, LEN - 1, &mut back).map_err(|_| ())?;
    let busy = matches!(nexus_abi::vmo_destroy(vmo), Err(AbiError::Busy));
    nexus_abi::vm_unmap(va, LEN).map_err(|_| ())?;
    let va2 = nexus_abi::vm_map(vmo, 0, LEN, flags).map_err(|_| ())?;
    let reused = va2 == va;
    nexus_abi::vm_unmap(va2, LEN).map_err(|_| ())?;
    nexus_abi::vmo_destroy(vmo).map_err(|_| ())?;
    if seeded && back[0] == 0x5A && busy && reused {
        Ok(())
    } else {
        Err(())
    }
}

// RFC-0068 mmio migration (task #103): RETIRED — queries the dead virtio-net MMIO cap (slot 48),
// which blocks like mmio_map. No longer called by phases/mmio.rs.
#[allow(dead_code)]
pub(crate) fn cap_query_mmio_probe() -> core::result::Result<(), ()> {
    const MMIO_CAP_SLOT: u32 = 48;
    let mut info = nexus_abi::CapQuery { kind_tag: 0, reserved: 0, base: 0, len: 0 };
    nexus_abi::cap_query(MMIO_CAP_SLOT, &mut info).map_err(|_| ())?;
    // 2 = DeviceMmio
    if info.kind_tag != 2 || info.base == 0 || info.len == 0 {
        return Err(());
    }
    Ok(())
}

pub(crate) fn cap_query_vmo_probe() -> core::result::Result<(), ()> {
    // Allocate a small VMO and ensure we can query its physical window deterministically.
    let vmo = nexus_abi::vmo_create(4096).map_err(|_| ())?;
    let mut info = nexus_abi::CapQuery { kind_tag: 0, reserved: 0, base: 0, len: 0 };
    nexus_abi::cap_query(vmo, &mut info).map_err(|_| ())?;
    // 1 = VMO
    if info.kind_tag != 1 || info.base == 0 || info.len < 4096 {
        return Err(());
    }
    Ok(())
}
