use nexus_hal::{Bus, DmaBuffer};

pub struct VirtioBlk<B: Bus> {
    bus: B,
}

impl<B: Bus> VirtioBlk<B> {
    pub fn new(bus: B) -> Self {
        Self { bus }
    }

    pub fn capacity(&self) -> u64 {
        let low = self.bus.read(0) as u64;
        let high = self.bus.read(4) as u64;
        (high << 32) | low
    }

    pub fn read_block<T: DmaBuffer>(&self, buffer: &mut T) {
        let _ = buffer.as_mut_ptr();
    }
}

#[cfg(test)]
mod tests {
    use super::VirtioBlk;
    use nexus_hal::{Bus, DmaBuffer};

    struct MockBus;

    impl Bus for MockBus {
        fn read(&self, addr: usize) -> u32 {
            match addr {
                0 => 0x0000_0000, // low 32 bits
                4 => 0x0000_0001, // high 32 bits
                _ => 0,
            }
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
            512
        }
    }

    #[test]
    fn capacity_combines_high_low() {
        let blk = VirtioBlk::new(MockBus);
        assert_eq!(blk.capacity(), 1u64 << 32);
    }

    #[test]
    fn read_block_stubs() {
        let blk = VirtioBlk::new(MockBus);
        let mut buf = MockBuf;
        blk.read_block(&mut buf);
        assert_eq!(buf.len(), 512);
    }
}
