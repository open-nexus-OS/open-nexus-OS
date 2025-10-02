use net_virtio::VirtioNet;
use nexus_hal::{Bus, DmaBuffer, Fence};

struct BusStub;

impl Bus for BusStub {
    fn read(&self, _addr: usize) -> u32 {
        1
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
        0
    }
}

struct FenceStub;

impl Fence for FenceStub {
    fn signal(&self) {}
}

#[test]
fn net_status() {
    let net = VirtioNet::new(BusStub);
    assert_eq!(net.read_status(), 1);
    net.kick_queue(&BufStub, &FenceStub);
}
