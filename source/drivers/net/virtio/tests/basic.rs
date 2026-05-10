use net_virtio::{
    QueueSetup, VirtioNetMmio, VIRTIO_DEVICE_ID_NET, VIRTIO_MMIO_MAGIC, VIRTIO_MMIO_VERSION_MODERN,
};
use nexus_hal::Bus;

// Integration test for the public VirtioNetMmio API.
//
// Note: the MMIO offsets are part of the virtio-mmio spec; this test uses numeric offsets to avoid
// exporting additional constants from the driver crate just for tests.
struct BusStub {
    regs: std::cell::RefCell<[u32; 0x200 / 4]>,
    writes: std::cell::RefCell<Vec<(usize, u32)>>,
}

impl Bus for BusStub {
    fn read(&self, _addr: usize) -> u32 {
        self.regs.borrow()[_addr / 4]
    }

    fn write(&self, addr: usize, value: u32) {
        self.writes.borrow_mut().push((addr, value));
        // Mirror the write into the register array so subsequent reads observe it.
        self.regs.borrow_mut()[addr / 4] = value;
    }
}

impl BusStub {
    fn new() -> Self {
        Self {
            regs: std::cell::RefCell::new([0; 0x200 / 4]),
            writes: std::cell::RefCell::new(Vec::new()),
        }
    }

    fn set(&self, off: usize, v: u32) {
        self.regs.borrow_mut()[off / 4] = v;
    }
}

#[test]
fn net_status() {
    // Register offsets (virtio-mmio).
    const REG_MAGIC: usize = 0x000;
    const REG_VERSION: usize = 0x004;
    const REG_DEVICE_ID: usize = 0x008;
    const REG_VENDOR_ID: usize = 0x00c;
    const REG_QUEUE_NUM_MAX: usize = 0x034;

    let bus = BusStub::new();
    bus.set(REG_MAGIC, VIRTIO_MMIO_MAGIC);
    bus.set(REG_VERSION, VIRTIO_MMIO_VERSION_MODERN);
    bus.set(REG_DEVICE_ID, VIRTIO_DEVICE_ID_NET);
    bus.set(REG_VENDOR_ID, 0x554d4551); // "QEMU"
    bus.set(REG_QUEUE_NUM_MAX, 256);

    let dev = VirtioNetMmio::new(bus);
    let info = dev.probe().expect("probe should succeed");
    assert_eq!(info.device_id, VIRTIO_DEVICE_ID_NET);

    dev.reset();
    let _accepted = dev.negotiate_features(0).expect("negotiate features should succeed");
    dev.setup_queue(
        0,
        &QueueSetup { size: 8, desc_paddr: 0x1000, avail_paddr: 0x2000, used_paddr: 0x3000 },
    )
    .expect("setup queue should succeed");
    dev.notify_queue(0);
}
