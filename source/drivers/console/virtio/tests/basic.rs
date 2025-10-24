//! CONTEXT: Basic tests for VirtIO console driver
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 1 unit test
//!
//! TEST_SCOPE:
//!   - Console byte writing
//!   - Fence signaling
//!   - Bus interface integration
//!
//! TEST_SCENARIOS:
//!   - test_console_flow(): Test console write and fence signaling
//!
//! DEPENDENCIES:
//!   - console_virtio::VirtioConsole: Console driver
//!   - nexus_hal::{Bus, Fence}: HAL interfaces
//!   - BusStub, FenceStub: Test implementations
//!
//! ADR: docs/adr/0016-kernel-libs-architecture.md

use console_virtio::VirtioConsole;
use nexus_hal::{Bus, Fence};
use std::sync::atomic::{AtomicU32, Ordering};

struct BusStub;
static WRITES: AtomicU32 = AtomicU32::new(0);

impl Bus for BusStub {
    fn read(&self, _addr: usize) -> u32 {
        0
    }

    fn write(&self, _addr: usize, value: u32) {
        WRITES.store(value, Ordering::SeqCst);
    }
}

struct FenceStub;
static SIGNALS: AtomicU32 = AtomicU32::new(0);

impl Fence for FenceStub {
    fn signal(&self) {
        SIGNALS.store(1, Ordering::SeqCst);
    }
}

#[test]
fn console_flow() {
    let console = VirtioConsole::new(BusStub);
    console.write_byte(0x55);
    console.flush(&FenceStub);
    assert_eq!(WRITES.load(Ordering::SeqCst), 0x55);
    assert_eq!(SIGNALS.load(Ordering::SeqCst), 1);
}
