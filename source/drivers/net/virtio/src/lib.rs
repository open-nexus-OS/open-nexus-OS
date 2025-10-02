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
