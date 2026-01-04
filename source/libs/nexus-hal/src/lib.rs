#![cfg_attr(not(test), no_std)]
//! CONTEXT: Userland HAL traits (Bus, DmaBuffer, Fence)
//! OWNERS: @runtime
//! PUBLIC API: Bus, DmaBuffer, Fence
//! DEPENDS_ON: core
//! INVARIANTS: Pure traits; no unsafe; test-only mocks in this crate
//! ADR: docs/adr/0016-kernel-libs-architecture.md

/// Basic bus access trait shared by user drivers.
pub trait Bus {
    fn read(&self, addr: usize) -> u32;
    fn write(&self, addr: usize, value: u32);
}

/// Safe DMA buffer abstraction.
pub trait DmaBuffer {
    fn as_ptr(&self) -> *const u8;
    fn as_mut_ptr(&mut self) -> *mut u8;
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Marker trait for devices supporting fenced submissions.
pub trait Fence {
    fn signal(&self);
}

#[cfg(test)]
mod tests {
    use super::{Bus, DmaBuffer, Fence};

    struct MockBus(u32);

    impl Bus for MockBus {
        fn read(&self, _addr: usize) -> u32 {
            self.0
        }

        fn write(&self, _addr: usize, _value: u32) {}
    }

    struct MockBuf([u8; 4]);

    impl DmaBuffer for MockBuf {
        fn as_ptr(&self) -> *const u8 {
            self.0.as_ptr()
        }

        fn as_mut_ptr(&mut self) -> *mut u8 {
            self.0.as_mut_ptr()
        }

        fn len(&self) -> usize {
            self.0.len()
        }
    }

    struct MockFence;

    impl Fence for MockFence {
        fn signal(&self) {}
    }

    #[test]
    fn bus_read_returns_value() {
        let bus = MockBus(10);
        assert_eq!(Bus::read(&bus, 0), 10);
    }

    #[test]
    fn dma_len_matches() {
        let buf = MockBuf([0; 4]);
        assert_eq!(DmaBuffer::len(&buf), 4);
    }

    #[test]
    fn fence_signal_compiles() {
        let fence = MockFence;
        fence.signal();
    }
}
