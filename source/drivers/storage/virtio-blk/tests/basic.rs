use nexus_hal::{Bus, DmaBuffer};
use storage_virtio_blk::VirtioBlk;

struct BusStub;

impl Bus for BusStub {
    fn read(&self, addr: usize) -> u32 {
        if addr == 0 {
            0
        } else {
            1
        }
    }

    fn write(&self, _addr: usize, _value: u32) {}
}

struct BufStub;

impl DmaBuffer for BufStub {
    fn as_ptr(&self) -> *const u8 {
        core::ptr::null()
    }

    fn as_mut_ptr(&mut self) -> *mut u8 {
        core::ptr::null_mut()
    }

    fn len(&self) -> usize {
        512
    }
}

#[test]
fn capacity_and_read() {
    let blk = VirtioBlk::new(BusStub);
    assert_eq!(blk.capacity(), (1_u64 << 32) | 0);
    let mut buf = BufStub;
    blk.read_block(&mut buf);
    assert_eq!(buf.len(), 512);
}
