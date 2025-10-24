// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: VirtIO console driver for serial communication
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 2 unit tests
//!
//! PUBLIC API:
//!   - VirtioConsole: Console driver implementation
//!   - write_byte(): Write single byte to console
//!   - flush(): Flush console output
//!
//! DEPENDENCIES:
//!   - nexus-hal::{Bus, Fence}: Hardware abstraction layer
//!
//! ADR: docs/adr/0016-kernel-libs-architecture.md

use nexus_hal::{Bus, Fence};

pub struct VirtioConsole<B: Bus> {
    bus: B,
}

impl<B: Bus> VirtioConsole<B> {
    pub fn new(bus: B) -> Self {
        Self { bus }
    }

    pub fn write_byte(&self, byte: u8) {
        self.bus.write(0, byte as u32);
    }

    pub fn flush(&self, fence: &dyn Fence) {
        fence.signal();
    }
}

#[cfg(test)]
mod tests {
    use super::VirtioConsole;
    use nexus_hal::{Bus, Fence};
    use std::sync::atomic::{AtomicU32, Ordering};

    struct MockBus;

    static LAST_WRITE: AtomicU32 = AtomicU32::new(0);

    impl Bus for MockBus {
        fn read(&self, _addr: usize) -> u32 {
            0
        }

        fn write(&self, _addr: usize, value: u32) {
            LAST_WRITE.store(value, Ordering::SeqCst);
        }
    }

    struct MockFence;

    static FLUSHED: AtomicU32 = AtomicU32::new(0);

    impl Fence for MockFence {
        fn signal(&self) {
            FLUSHED.store(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn write_updates_state() {
        let console = VirtioConsole::new(MockBus);
        console.write_byte(0x41);
        assert_eq!(LAST_WRITE.load(Ordering::SeqCst), 0x41);
    }

    #[test]
    fn flush_signals() {
        let console = VirtioConsole::new(MockBus);
        console.flush(&MockFence);
        assert_eq!(FLUSHED.load(Ordering::SeqCst), 1);
    }
}
