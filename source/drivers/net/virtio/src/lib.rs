// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: VirtIO network driver for network communication
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 2 unit tests
//!
//! PUBLIC API:
//!   - VirtioNet: Network driver implementation
//!   - read_status(): Read network status
//!   - kick_queue(): Kick network queue
//!
//! DEPENDENCIES:
//!   - nexus-hal::{Bus, DmaBuffer, Fence}: Hardware abstraction layer
//!
//! ADR: docs/adr/0016-kernel-libs-architecture.md

use nexus_hal::{Bus, DmaBuffer, Fence};

pub struct VirtioNet<B: Bus> {
    bus: B,
}

impl<B: Bus> VirtioNet<B> {
    pub fn new(bus: B) -> Self {
        Self { bus }
    }

    pub fn read_status(&self) -> u32 {
        self.bus.read(0)
    }

    pub fn kick_queue<T: DmaBuffer>(&self, buffer: &T, fence: &dyn Fence) {
        let _ = buffer.as_ptr();
        fence.signal();
    }
}

#[cfg(test)]
mod tests {
    use super::VirtioNet;
    use nexus_hal::{Bus, DmaBuffer, Fence};

    struct MockBus;

    impl Bus for MockBus {
        fn read(&self, _addr: usize) -> u32 {
            0xABCD
        }

        fn write(&self, _addr: usize, _value: u32) {}
    }

    struct MockBuf;

    impl DmaBuffer for MockBuf {
        fn as_ptr(&self) -> *const u8 {
            core::ptr::null()
        }

        fn as_mut_ptr(&mut self) -> *mut u8 {
            core::ptr::null_mut()
        }

        fn len(&self) -> usize {
            0
        }
    }

    struct MockFence;

    impl Fence for MockFence {
        fn signal(&self) {}
    }

    #[test]
    fn status_read() {
        let net = VirtioNet::new(MockBus);
        assert_eq!(net.read_status(), 0xABCD);
    }

    #[test]
    fn queue_kick() {
        let net = VirtioNet::new(MockBus);
        net.kick_queue(&MockBuf, &MockFence);
    }
}
