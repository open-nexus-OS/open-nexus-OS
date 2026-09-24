// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! DMA buffer ownership — a buffer belongs to the CPU or to the device, never both
//! (TASK-0286 P4b, RFC-0098 C4; closes TASK-0284's ownership prototype).
//!
//! [`DmaBuffer`] is CPU-owned: its bytes can be read and written. [`DmaBuffer::for_device`]
//! consumes it and returns an [`InFlight`] that exposes only the runs the device is
//! programmed with — no bytes, so the CPU cannot touch memory the device may be reading or
//! writing, and a buffer cannot be submitted twice (both are compile errors, see below).
//! [`InFlight::for_cpu`] hands it back. The cache maintenance a non-coherent device needs
//! happens exactly at those two transitions: to the device, `clean` (CPU writes reach
//! memory); from or both ways, `flush` (no dirty line can later overwrite what the device
//! wrote); back to the CPU after the device may have written, `flush` again (the CPU reads
//! memory, not stale lines). A coherent device costs nothing; a non-coherent device on a
//! machine without Zicbom is refused at construction.
//!
//! The memory and the cache instructions are traits so the host is the oracle:
//! [`DmaMemory`] is `nexus_abi::DmaVmo` on the OS (a VMO mapped for DMA, zero on create),
//! [`CacheOps`] is [`Zicbom`] (`nexus_abi::cache_clean` / `cache_flush`).
//!
//! A buffer goes to the device and comes back:
//! ```
//! # use nexus_driverkit::{DmaBuffer, DmaMemory, Direction, Zicbom};
//! # use nexus_abi::{DmaCoherence, DmaRun};
//! # struct Mem(Vec<u8>);
//! # impl DmaMemory for Mem {
//! #     fn bytes(&self) -> &[u8] { &self.0 }
//! #     fn bytes_mut(&mut self) -> &mut [u8] { &mut self.0 }
//! #     fn runs(&self) -> &[DmaRun] { &[] }
//! # }
//! let mut buf = DmaBuffer::new(Mem(vec![0; 64]), DmaCoherence::Coherent, Zicbom).unwrap();
//! buf.bytes_mut()[0] = 1;
//! let in_flight = buf.for_device(Direction::Bidirectional);
//! // ... program the device with `in_flight.runs()`, ring the doorbell, wait ...
//! let buf = in_flight.for_cpu();
//! assert_eq!(buf.bytes()[0], 1);
//! ```
//! The CPU cannot read a buffer in flight:
//! ```compile_fail
//! # use nexus_driverkit::{DmaBuffer, DmaMemory, Direction, Zicbom};
//! # use nexus_abi::{DmaCoherence, DmaRun};
//! # struct Mem(Vec<u8>);
//! # impl DmaMemory for Mem {
//! #     fn bytes(&self) -> &[u8] { &self.0 }
//! #     fn bytes_mut(&mut self) -> &mut [u8] { &mut self.0 }
//! #     fn runs(&self) -> &[DmaRun] { &[] }
//! # }
//! let buf = DmaBuffer::new(Mem(vec![0; 64]), DmaCoherence::Coherent, Zicbom).unwrap();
//! let in_flight = buf.for_device(Direction::FromDevice);
//! let _ = in_flight.bytes();
//! ```
//! nor submit it twice:
//! ```compile_fail
//! # use nexus_driverkit::{DmaBuffer, DmaMemory, Direction, Zicbom};
//! # use nexus_abi::{DmaCoherence, DmaRun};
//! # struct Mem(Vec<u8>);
//! # impl DmaMemory for Mem {
//! #     fn bytes(&self) -> &[u8] { &self.0 }
//! #     fn bytes_mut(&mut self) -> &mut [u8] { &mut self.0 }
//! #     fn runs(&self) -> &[DmaRun] { &[] }
//! # }
//! let buf = DmaBuffer::new(Mem(vec![0; 64]), DmaCoherence::Coherent, Zicbom).unwrap();
//! let first = buf.for_device(Direction::ToDevice);
//! let second = buf.for_device(Direction::ToDevice);
//! ```

use nexus_abi::{DmaCoherence, DmaRun};

/// The memory behind a DMA buffer: the CPU's view and the device's.
pub trait DmaMemory {
    /// The CPU's view.
    fn bytes(&self) -> &[u8];
    /// The CPU's view, writable.
    fn bytes_mut(&mut self) -> &mut [u8];
    /// What the device is programmed with.
    fn runs(&self) -> &[DmaRun];
}

/// The cache instructions a non-coherent device needs, in `block`-byte cache blocks.
pub trait CacheOps {
    /// Write back (the device will read).
    fn clean(&self, bytes: &[u8], block: usize);
    /// Write back and drop (the device will write, or wrote).
    fn flush(&self, bytes: &[u8], block: usize);
}

/// The harts' Zicbom instructions (`nexus_abi::cache_clean` / `cache_flush`; no-ops
/// off RISC-V hardware).
#[derive(Clone, Copy, Debug, Default)]
pub struct Zicbom;

impl CacheOps for Zicbom {
    fn clean(&self, bytes: &[u8], block: usize) {
        nexus_abi::cache_clean(bytes, block);
    }
    fn flush(&self, bytes: &[u8], block: usize) {
        nexus_abi::cache_flush(bytes, block);
    }
}

#[cfg(nexus_env = "os")]
impl DmaMemory for nexus_abi::DmaVmo {
    fn bytes(&self) -> &[u8] {
        nexus_abi::DmaVmo::bytes(self)
    }
    fn bytes_mut(&mut self) -> &mut [u8] {
        nexus_abi::DmaVmo::bytes_mut(self)
    }
    fn runs(&self) -> &[DmaRun] {
        nexus_abi::DmaVmo::runs(self)
    }
}

/// Which way the data moves in a transfer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// The device reads what the CPU wrote.
    ToDevice,
    /// The device writes what the CPU will read.
    FromDevice,
    /// Both.
    Bidirectional,
}

/// Why a buffer could not be made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DmaError {
    /// The device does not snoop the caches and the harts have no Zicbom.
    Unmaintainable,
}

/// A DMA buffer the CPU owns.
pub struct DmaBuffer<M: DmaMemory, C: CacheOps> {
    mem: M,
    cache: C,
    /// `Some(block)` when the device needs maintenance.
    block: Option<usize>,
}

/// A DMA buffer the device owns: its runs, not its bytes.
#[must_use = "a buffer in flight comes back to the CPU only through `for_cpu`"]
pub struct InFlight<M: DmaMemory, C: CacheOps> {
    buf: DmaBuffer<M, C>,
    dir: Direction,
}

impl<M: DmaMemory, C: CacheOps> DmaBuffer<M, C> {
    /// Wrap `mem` for a device of the given coherence (its capability's
    /// `nexus_abi::device_dma_coherence`).
    pub fn new(mem: M, coherence: DmaCoherence, cache: C) -> Result<Self, DmaError> {
        let block = match coherence {
            DmaCoherence::Coherent => None,
            DmaCoherence::Maintained { block } => Some(block),
            DmaCoherence::Unmaintainable => return Err(DmaError::Unmaintainable),
        };
        Ok(Self { mem, cache, block })
    }

    /// The CPU's view.
    pub fn bytes(&self) -> &[u8] {
        self.mem.bytes()
    }

    /// The CPU's view, writable.
    pub fn bytes_mut(&mut self) -> &mut [u8] {
        self.mem.bytes_mut()
    }

    /// What the device will be programmed with (descriptors are written while the
    /// CPU still owns the buffer).
    pub fn runs(&self) -> &[DmaRun] {
        self.mem.runs()
    }

    /// Bytes.
    pub fn len(&self) -> usize {
        self.mem.bytes().len()
    }

    /// True for an empty buffer.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Hand the buffer to the device for a transfer in `dir`.
    pub fn for_device(self, dir: Direction) -> InFlight<M, C> {
        if let Some(block) = self.block {
            match dir {
                Direction::ToDevice => self.cache.clean(self.mem.bytes(), block),
                Direction::FromDevice | Direction::Bidirectional => {
                    self.cache.flush(self.mem.bytes(), block)
                }
            }
        }
        InFlight { buf: self, dir }
    }

    /// The memory back (e.g. to drop it).
    pub fn into_memory(self) -> M {
        self.mem
    }
}

impl<M: DmaMemory, C: CacheOps> InFlight<M, C> {
    /// What the device is programmed with.
    pub fn runs(&self) -> &[DmaRun] {
        self.buf.mem.runs()
    }

    /// The transfer's direction.
    pub fn direction(&self) -> Direction {
        self.dir
    }

    /// The device is done: the CPU owns the buffer again.
    pub fn for_cpu(self) -> DmaBuffer<M, C> {
        if let Some(block) = self.buf.block {
            if self.dir != Direction::ToDevice {
                self.buf.cache.flush(self.buf.mem.bytes(), block);
            }
        }
        self.buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::vec::Vec;

    struct Mem {
        bytes: Vec<u8>,
        runs: Vec<DmaRun>,
    }

    impl Mem {
        fn new(len: usize) -> Self {
            Self { bytes: vec![0; len], runs: vec![DmaRun { bus: 0x8040_0000, len: len as u64 }] }
        }
    }

    impl DmaMemory for Mem {
        fn bytes(&self) -> &[u8] {
            &self.bytes
        }
        fn bytes_mut(&mut self) -> &mut [u8] {
            &mut self.bytes
        }
        fn runs(&self) -> &[DmaRun] {
            &self.runs
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Op {
        Clean(usize, usize),
        Flush(usize, usize),
    }

    #[derive(Default)]
    struct Recorder(RefCell<Vec<Op>>);

    impl CacheOps for &Recorder {
        fn clean(&self, bytes: &[u8], block: usize) {
            self.0.borrow_mut().push(Op::Clean(bytes.len(), block));
        }
        fn flush(&self, bytes: &[u8], block: usize) {
            self.0.borrow_mut().push(Op::Flush(bytes.len(), block));
        }
    }

    fn round_trip(coherence: DmaCoherence, dir: Direction) -> Vec<Op> {
        let rec = Recorder::default();
        let mut buf = DmaBuffer::new(Mem::new(300), coherence, &rec).unwrap();
        buf.bytes_mut()[0] = 0xa5;
        let in_flight = buf.for_device(dir);
        assert_eq!(in_flight.runs(), &[DmaRun { bus: 0x8040_0000, len: 300 }]);
        assert_eq!(in_flight.direction(), dir);
        let buf = in_flight.for_cpu();
        assert_eq!(buf.bytes()[0], 0xa5);
        rec.0.take()
    }

    const MAINTAINED: DmaCoherence = DmaCoherence::Maintained { block: 64 };

    #[test]
    fn a_coherent_device_costs_no_maintenance_in_any_direction() {
        for dir in [Direction::ToDevice, Direction::FromDevice, Direction::Bidirectional] {
            assert!(round_trip(DmaCoherence::Coherent, dir).is_empty());
        }
    }

    #[test]
    fn to_the_device_the_whole_buffer_is_cleaned_once_and_nothing_on_return() {
        assert_eq!(round_trip(MAINTAINED, Direction::ToDevice), [Op::Clean(300, 64)]);
    }

    #[test]
    fn from_the_device_it_is_flushed_before_and_after() {
        assert_eq!(
            round_trip(MAINTAINED, Direction::FromDevice),
            [Op::Flush(300, 64), Op::Flush(300, 64)]
        );
        assert_eq!(
            round_trip(MAINTAINED, Direction::Bidirectional),
            [Op::Flush(300, 64), Op::Flush(300, 64)]
        );
    }

    #[test]
    fn test_reject_dma_buffer_for_an_unmaintainable_device() {
        let rec = Recorder::default();
        let refused = DmaBuffer::new(Mem::new(64), DmaCoherence::Unmaintainable, &rec);
        assert_eq!(refused.err(), Some(DmaError::Unmaintainable));
        assert!(rec.0.borrow().is_empty());
    }

    #[test]
    fn descriptors_are_written_from_the_runs_while_the_cpu_owns_the_buffer() {
        let buf = DmaBuffer::new(Mem::new(4096), DmaCoherence::Coherent, Zicbom).unwrap();
        assert_eq!(buf.runs().len(), 1);
        assert_eq!(buf.len(), 4096);
        assert!(!buf.is_empty());
        assert_eq!(buf.into_memory().bytes.len(), 4096);
    }
}
