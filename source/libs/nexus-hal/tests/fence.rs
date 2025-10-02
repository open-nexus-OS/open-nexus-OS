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
