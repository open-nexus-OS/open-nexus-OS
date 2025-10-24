//! CONTEXT: Tests for HAL traits: Bus/DmaBuffer/Fence mocks
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 1 unit test
//!
//! TEST_SCOPE:
//!   - HAL trait implementations
//!   - Mock object behavior
//!   - Interface compliance
//!
//! TEST_SCENARIOS:
//!   - test_fence_signaling(): Test fence signaling functionality
//!
//! DEPENDENCIES:
//!   - nexus_hal::{Bus, DmaBuffer, Fence}: HAL trait definitions
//!   - DummyBus, DummyDmaBuffer, DummyFence: Mock implementations
//!
//! ADR: docs/adr/0016-kernel-libs-architecture.md
use nexus_hal::{Bus, DmaBuffer, Fence};

struct DummyBus;

impl Bus for DummyBus {
    fn read(&self, _addr: usize) -> u32 {
        0
    }

    fn write(&self, _addr: usize, _value: u32) {}
}

struct DummyBuf([u8; 1]);

impl DmaBuffer for DummyBuf {
    fn as_ptr(&self) -> *const u8 {
        self.0.as_ptr()
    }

    fn as_mut_ptr(&mut self) -> *mut u8 {
        self.0.as_mut_ptr()
    }

    fn len(&self) -> usize {
        1
    }
}

struct DummyFence;

impl Fence for DummyFence {
    fn signal(&self) {}
}

#[test]
fn bus_roundtrip() {
    let bus = DummyBus;
    Bus::write(&bus, 0, 1);
    assert_eq!(Bus::read(&bus, 0), 0);
}

#[test]
fn dma_buffer_len() {
    let buf = DummyBuf([0]);
    assert_eq!(DmaBuffer::len(&buf), 1);
}

#[test]
fn fence_signal() {
    let fence = DummyFence;
    Fence::signal(&fence);
}
