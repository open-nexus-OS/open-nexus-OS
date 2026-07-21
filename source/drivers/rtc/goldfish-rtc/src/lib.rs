// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: goldfish-RTC read path (RFC-0076): maps the policy-gated MMIO
//! window (`device.mmio.rtc`, QEMU virt `rtc@101000`, dtb-verified) and reads
//! the wall-clock epoch. Read-only, no IRQ, no write-back — the smallest
//! honest hardware seam; `timed` (the time authority) is the only caller.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: Register-decode unit test below; the live read is proven by
//! the QEMU markers (`timed: walltime anchored`, `SELFTEST: walltime rtc ok`).
//! RFC: docs/rfcs/RFC-0076-wallclock-v1-rtcd-timed-tz.md

#![cfg_attr(all(nexus_env = "os", target_os = "none"), no_std)]

/// goldfish-rtc registers (Android goldfish platform spec): reading
/// `TIME_LOW` latches the full 64-bit nanosecond value; `TIME_HIGH` returns
/// the latched upper half.
#[cfg(nexus_env = "os")]
const REG_TIME_LOW: usize = 0x00;
#[cfg(nexus_env = "os")]
const REG_TIME_HIGH: usize = 0x04;

/// Why an RTC read failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RtcError {
    /// The MMIO window could not be mapped (missing/denied grant).
    MapFailed,
    /// The device returned an implausible epoch (zero — device absent).
    Implausible,
}

/// Combines the two 32-bit register halves into the epoch value.
#[inline]
#[must_use]
pub fn combine_epoch(low: u32, high: u32) -> u64 {
    (u64::from(high) << 32) | u64::from(low)
}

/// Maps the RTC window (idempotent) and reads the current UTC epoch in
/// nanoseconds. Fail-closed: a zero epoch (absent device reads as zeros)
/// is `Implausible`, never returned as time. OS-only (MMIO syscalls).
#[cfg(nexus_env = "os")]
pub fn read_epoch_ns(mmio_cap_slot: u32, mmio_base_va: usize) -> Result<u64, RtcError> {
    nexus_abi::mmio_map(mmio_cap_slot, mmio_base_va, 0)
        .or_else(|e| if e == nexus_abi::AbiError::InvalidArgument { Ok(()) } else { Err(e) })
        .map_err(|_| RtcError::MapFailed)?;
    // Order matters: TIME_LOW latches TIME_HIGH (goldfish contract).
    let low = unsafe { core::ptr::read_volatile((mmio_base_va + REG_TIME_LOW) as *const u32) };
    let high = unsafe { core::ptr::read_volatile((mmio_base_va + REG_TIME_HIGH) as *const u32) };
    let epoch_ns = combine_epoch(low, high);
    if epoch_ns == 0 {
        return Err(RtcError::Implausible);
    }
    Ok(epoch_ns)
}

// Host builds never touch hardware; the register-combine math above is the
// host-testable slice.
#[cfg(not(nexus_env = "os"))]
pub fn read_epoch_ns(_mmio_cap_slot: u32, _mmio_base_va: usize) -> Result<u64, RtcError> {
    Err(RtcError::MapFailed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combine_orders_halves_correctly() {
        // 2026-07-21T00:00:00Z ≈ 1_784_678_400 s → ns fits the split.
        let epoch_ns: u64 = 1_784_678_400_000_000_000;
        let low = (epoch_ns & 0xFFFF_FFFF) as u32;
        let high = (epoch_ns >> 32) as u32;
        assert_eq!(combine_epoch(low, high), epoch_ns);
        assert_eq!(combine_epoch(0, 0), 0);
    }
}
