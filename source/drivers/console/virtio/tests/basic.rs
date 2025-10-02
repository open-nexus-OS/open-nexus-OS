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
