// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(test), no_std)]

//! CONTEXT: VirtIO net (virtio-mmio) bring-up driver (userspace-first)
//! OWNERS: @runtime
//! STATUS: In Progress
//! API_STABILITY: Unstable (bring-up)
//! TEST_COVERAGE: basic probe unit test (host); OS/QEMU proof via selftest markers
//!
//! PUBLIC API:
//! - `VirtioNetMmio`: minimal virtio-mmio net device control plane (probe + queue programming)
//!
//! Gate 3: the device-agnostic virtio-mmio transport (register map, the
//! reset/ack/driver/features/queue/driver_ok handshake, queue programming) now
//! lives in [`nexus_virtio`]; this crate is a thin net-specific wrapper (device
//! id + the polling-oriented data plane in userspace).

use nexus_hal::Bus;
use nexus_virtio::VirtioMmio;

// Re-export the shared wire/transport types so existing consumers
// (`net_virtio::QueueSetup`, `VIRTIO_MMIO_MAGIC`, …) keep resolving unchanged.
pub use nexus_virtio::{
    DeviceInfo, QueueSetup, VirtioError, VIRTIO_MMIO_MAGIC, VIRTIO_MMIO_VERSION_LEGACY,
    VIRTIO_MMIO_VERSION_MODERN,
};

/// VirtIO device id for network cards.
pub const VIRTIO_DEVICE_ID_NET: u32 = 1;

/// A minimal virtio-mmio net device wrapper.
///
/// `bus.read/write(offset)` addresses are interpreted as **MMIO register offsets**.
pub struct VirtioNetMmio<B: Bus> {
    mmio: VirtioMmio<B>,
}

impl<B: Bus> VirtioNetMmio<B> {
    pub fn new(bus: B) -> Self {
        Self { mmio: VirtioMmio::new(bus) }
    }

    /// Validate MMIO identity as a virtio-net device.
    pub fn probe(&self) -> Result<DeviceInfo, VirtioError> {
        self.mmio.probe(VIRTIO_DEVICE_ID_NET)
    }

    /// Resets the device status to 0.
    pub fn reset(&self) {
        self.mmio.reset();
    }

    /// Feature negotiation: accept `driver_features ∩ device_features`; returns
    /// the accepted mask. Bring-up callers typically pass 0 or a small set.
    pub fn negotiate_features(&self, driver_features: u64) -> Result<u64, VirtioError> {
        self.mmio.negotiate_features(driver_features)
    }

    /// Programs a queue's descriptor/avail/used addresses (physical) and marks it
    /// READY. Caller must ensure memory is DMA-safe and correctly aligned.
    pub fn setup_queue(&self, index: u32, cfg: &QueueSetup) -> Result<(), VirtioError> {
        self.mmio.setup_queue(index, cfg)
    }

    pub fn set_driver_ok(&self) {
        self.mmio.driver_ok();
    }

    pub fn notify_queue(&self, queue_index: u32) {
        self.mmio.notify_queue(queue_index);
    }

    /// Borrow the underlying transport (device-config registers, raw bus access).
    pub fn mmio(&self) -> &VirtioMmio<B> {
        &self.mmio
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ZeroBus;
    impl Bus for ZeroBus {
        fn read(&self, _addr: usize) -> u32 {
            0
        }
        fn write(&self, _addr: usize, _value: u32) {}
    }

    #[test]
    fn probe_rejects_bad_magic() {
        let dev = VirtioNetMmio::new(ZeroBus);
        assert_eq!(dev.probe(), Err(VirtioError::BadMagic));
    }
}
