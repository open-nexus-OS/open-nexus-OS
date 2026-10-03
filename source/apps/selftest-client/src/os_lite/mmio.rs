// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: MMIO selftest helpers — the RFC-0085 `vm_map` roundtrip proof and
//!   the `cap_query` probes consumed by `phases::mmio` (the opt-in `smoltcp-probe`
//!   lane maps its device through `nexus_driverkit::Mmio`, the MMIO seam).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os) — mmio phase.
//!
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

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
    const MMIO_CAP_SLOT: u32 = nexus_service_topology::DEVICE_MMIO_SLOT;
    let mut info = nexus_abi::CapQuery::default();
    nexus_abi::cap_query(MMIO_CAP_SLOT, &mut info).map_err(|_| ())?;
    // 2 = DeviceMmio
    if info.kind_tag != 2 || info.base == 0 || info.len == 0 {
        return Err(());
    }
    Ok(())
}

/// Whether init granted this client a device capability in its MMIO slot (a device plane the
/// tree names; absent on a board without that plane).
pub(crate) fn device_granted() -> bool {
    const DEVICE: u32 = nexus_service_topology::DEVICE_MMIO_SLOT;
    let mut info = nexus_abi::CapQuery::default();
    nexus_abi::cap_query(DEVICE, &mut info).is_ok() && info.kind_tag == 2
}

pub(crate) fn cap_query_vmo_probe() -> core::result::Result<(), ()> {
    // A VMO queries as kind 1 with its length — and NO physical base: a physical
    // address leaves the kernel only through `vmo_runs` (RFC-0098 C4, TASK-0286
    // P4a), never through `cap_query`. A contiguous object is the case that used
    // to leak one, so that is the one probed — made for the device window init
    // grants this harness (a contiguous object is a device's, TASK-0246 P1).
    const DEVICE: u32 = nexus_service_topology::DEVICE_MMIO_SLOT;
    let vmo = nexus_abi::vmo_create_contiguous(DEVICE, 4096).map_err(|_| ())?;
    let mut info = nexus_abi::CapQuery::default();
    let queried = nexus_abi::cap_query(vmo, &mut info);
    let _ = nexus_abi::vmo_destroy(vmo);
    queried.map_err(|_| ())?;
    // 1 = VMO
    if info.kind_tag != 1 || info.base != 0 || info.len < 4096 {
        return Err(());
    }
    Ok(())
}

/// What `dma_buffer_probe` measured: the device's own coherence, the harts' Zicbom
/// block and the runs of the buffer.
pub(crate) struct DmaBufferProof {
    pub coherent: bool,
    pub block: usize,
    pub runs: usize,
}

/// RFC-0098 C4 (TASK-0286 P4b): a `DmaBuffer` over a real `DmaVmo` with the Zicbom
/// instructions forced on — QEMU's virtio devices are coherent, so the device's own
/// coherence would skip them; forcing them proves that user mode may execute
/// `cbo.clean`/`cbo.flush` (the kernel's `senvcfg`) and that every byte survives the
/// ToDevice and Bidirectional round trips. The device capability is the net window
/// init grants this harness; a VMO must be refused as a device.
pub(crate) fn dma_buffer_probe() -> core::result::Result<DmaBufferProof, ()> {
    use nexus_abi::{DmaCoherence, DmaVmo};
    use nexus_driverkit::{Direction, DmaBuffer, Zicbom};
    const DEVICE: u32 = nexus_service_topology::DEVICE_MMIO_SLOT;
    const LEN: usize = 3 * 4096 + 100;
    let mut query = nexus_abi::CapQuery::default();
    nexus_abi::cap_query(DEVICE, &mut query).map_err(|_| ())?;
    let coherent =
        nexus_abi::device_dma_coherence(DEVICE).map_err(|_| ())? == DmaCoherence::Coherent;
    let block = query.cache_block as usize;
    if block == 0 {
        return Err(()); // no Zicbom on the harts: nothing to prove the instructions with
    }
    let vmo = DmaVmo::anonymous(DEVICE, LEN).map_err(|_| ())?;
    let refused = nexus_abi::device_dma_coherence(vmo.handle()).is_err();
    let runs = vmo.runs().len();
    let covered: u64 = vmo.runs().iter().map(|r| r.len).sum();
    let mut buf =
        DmaBuffer::new(vmo, DmaCoherence::Maintained { block }, Zicbom).map_err(|_| ())?;
    for (i, byte) in buf.bytes_mut().iter_mut().enumerate() {
        *byte = (i % 251) as u8;
    }
    let buf = buf.for_device(Direction::ToDevice).for_cpu();
    let buf = buf.for_device(Direction::Bidirectional).for_cpu();
    let intact = buf.bytes().iter().enumerate().all(|(i, byte)| *byte == (i % 251) as u8);
    if refused && intact && covered == LEN as u64 && runs >= 1 {
        Ok(DmaBufferProof { coherent, block, runs })
    } else {
        Err(())
    }
}
