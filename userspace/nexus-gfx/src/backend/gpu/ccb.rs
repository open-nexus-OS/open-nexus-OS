// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The three command buffers as rings (TASK-0329 P1): the kernel command buffer
//! (host → firmware: 128 slots of 88 bytes, a return-slot word per slot the firmware marks
//! EXECUTED; capacity is slots − 1), the firmware-to-host buffer (32 slots, the host advances
//! the read offset), and the client command buffer (per queue, a 32 KiB byte ring of
//! header + payload commands that NEVER wrap: a padding command fills the tail). The rings are
//! pure over a memory trait — the device driver implements it over its uncached firmware
//! memory with the needed fences, the host over byte vectors with a scripted firmware.
//! OWNERS: @gpu
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below

use super::fwif::{self, CcbCmdHeader, FwccbCmd, KccbCmd};

/// The shared memory behind the kernel command buffer.
pub trait KccbMemory {
    /// The control block's `read_offset` (the firmware advances it).
    fn read_offset(&self) -> u32;
    /// Publishes a new `write_offset` (after the command bytes are visible to the device).
    fn set_write_offset(&mut self, offset: u32);
    /// Copies a command into `slot`.
    fn write_slot(&mut self, slot: u32, cmd: &[u8; KccbCmd::SIZE]);
    /// The return slot word.
    fn rtn(&self, slot: u32) -> u32;
    fn set_rtn(&mut self, slot: u32, value: u32);
}

/// Why a kernel command could not be sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KccbError {
    /// Every slot holds a command the firmware has not consumed.
    Full,
}

/// The host's side of the kernel command buffer: its own write offset (the control block's
/// is the published copy).
#[derive(Clone, Copy, Debug)]
pub struct Kccb {
    write_offset: u32,
    num_cmds: u32,
}

impl Default for Kccb {
    fn default() -> Self {
        Self::new()
    }
}

impl Kccb {
    pub const fn new() -> Self {
        Self { write_offset: 0, num_cmds: fwif::kccb::NUM_CMDS }
    }

    pub const fn wrap_mask(&self) -> u32 {
        self.num_cmds - 1
    }

    /// Commands the firmware has not consumed.
    pub fn used(&self, m: &impl KccbMemory) -> u32 {
        let rd = m.read_offset() & self.wrap_mask();
        if self.write_offset >= rd {
            self.write_offset - rd
        } else {
            self.write_offset + self.num_cmds - rd
        }
    }

    /// Whether one more command fits (capacity is slots − 1: a full ring is never ambiguous
    /// with an empty one).
    pub fn has_room(&self, m: &impl KccbMemory) -> bool {
        self.used(m) < self.num_cmds - 1
    }

    /// Writes `cmd` into the next slot, clears its return slot, publishes the write offset;
    /// returns the slot (whose return word the caller waits for). The caller kicks.
    pub fn send(&mut self, m: &mut impl KccbMemory, cmd: &KccbCmd) -> Result<u32, KccbError> {
        if !self.has_room(m) {
            return Err(KccbError::Full);
        }
        let slot = self.write_offset;
        m.write_slot(slot, &cmd.to_bytes());
        m.set_rtn(slot, fwif::kccb::RTN_SLOT_NO_RESPONSE);
        self.write_offset = (slot + 1) & self.wrap_mask();
        m.set_write_offset(self.write_offset);
        Ok(slot)
    }

    /// Whether the firmware executed the command in `slot`.
    pub fn executed(&self, m: &impl KccbMemory, slot: u32) -> bool {
        m.rtn(slot) & fwif::kccb::RTN_SLOT_CMD_EXECUTED != 0
    }

    /// Whether every command was consumed.
    pub fn idle(&self, m: &impl KccbMemory) -> bool {
        (m.read_offset() & self.wrap_mask()) == self.write_offset
    }
}

/// The shared memory behind the firmware-to-host buffer.
pub trait FwccbMemory {
    fn write_offset(&self) -> u32;
    fn read_offset(&self) -> u32;
    fn set_read_offset(&mut self, offset: u32);
    fn read_slot(&self, slot: u32) -> [u8; FwccbCmd::SIZE];
}

/// The host's reader of the firmware-to-host buffer.
#[derive(Clone, Copy, Debug, Default)]
pub struct Fwccb;

impl Fwccb {
    /// The next command the firmware wrote, if any; the read offset advances past it. A
    /// command without the magic is returned too (the caller drops it with a line).
    pub fn next(&self, m: &mut impl FwccbMemory) -> Option<FwccbCmd> {
        let rd = m.read_offset();
        if rd == m.write_offset() {
            return None;
        }
        let cmd = FwccbCmd::from_bytes(&m.read_slot(rd & (fwif::fwccb::NUM_CMDS - 1)));
        m.set_read_offset((rd + 1) & (fwif::fwccb::NUM_CMDS - 1));
        Some(cmd)
    }
}

/// The shared memory behind a client command buffer.
pub trait CccbMemory {
    /// The control block's `read_offset` (the firmware advances it).
    fn read_offset(&self) -> u32;
    /// Writes `bytes` at `offset` inside the ring.
    fn write(&mut self, offset: u32, bytes: &[u8]);
}

/// Why a client command could not be written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CccbError {
    /// Not enough free bytes for the command (and the padding a wrap would need).
    Full,
    /// Larger than the ring could ever hold.
    TooLarge,
}

/// The host's writer of one client command buffer: its own write offset (the kick carries it
/// to the firmware; the firmware's `read_offset` says what it consumed).
#[derive(Clone, Copy, Debug)]
pub struct Cccb {
    write_offset: u32,
    size: u32,
}

const fn align8(n: u32) -> u32 {
    (n + 7) & !7
}

impl Default for Cccb {
    fn default() -> Self {
        Self::new()
    }
}

impl Cccb {
    pub const fn new() -> Self {
        Self { write_offset: 0, size: fwif::ccb::SIZE }
    }

    pub const fn write_offset(&self) -> u32 {
        self.write_offset
    }

    pub const fn wrap_mask(&self) -> u32 {
        self.size - 1
    }

    /// Bytes a command with `payload` bytes takes: the header + the payload aligned to 8.
    pub const fn size_with_header(payload: u32) -> u32 {
        CcbCmdHeader::SIZE as u32 + align8(payload)
    }

    fn space(&self, read_offset: u32) -> u32 {
        (read_offset.wrapping_sub(self.write_offset).wrapping_add(self.size - 1)) & (self.size - 1)
    }

    /// Whether a command sequence of `bytes` fits (with the tail padding a wrap would need).
    pub fn fits(&self, m: &impl CccbMemory, mut bytes: u32) -> bool {
        let remaining = self.size - self.write_offset;
        if bytes + CcbCmdHeader::SIZE as u32 > remaining {
            bytes += remaining;
        }
        self.space(m.read_offset()) >= bytes
    }

    /// Writes one command (`cmd_type`, `payload`) with its header. A command that would cross
    /// the end is preceded by a padding command that fills the tail, so no command ever wraps.
    pub fn write_command(
        &mut self,
        m: &mut impl CccbMemory,
        cmd_type: u32,
        payload: &[u8],
        ext_job_ref: u32,
        int_job_ref: u32,
    ) -> Result<(), CccbError> {
        let payload_len = u32::try_from(payload.len()).map_err(|_| CccbError::TooLarge)?;
        let sz = Self::size_with_header(payload_len);
        if sz > self.size - CcbCmdHeader::SIZE as u32 {
            return Err(CccbError::TooLarge);
        }
        let remaining = self.size - self.write_offset;
        let needs_pad = remaining < sz + CcbCmdHeader::SIZE as u32;
        let required = if needs_pad { sz + remaining } else { sz };
        if self.space(m.read_offset()) < required {
            return Err(CccbError::Full);
        }
        if needs_pad {
            let pad = CcbCmdHeader {
                cmd_type: fwif::ccb::PADDING,
                cmd_size: remaining - CcbCmdHeader::SIZE as u32,
                ..Default::default()
            };
            m.write(self.write_offset, &pad.to_bytes());
            self.write_offset = 0;
        }
        let header = CcbCmdHeader {
            cmd_type,
            cmd_size: align8(payload_len),
            ext_job_ref,
            int_job_ref,
            work_est: [0; 24],
        };
        m.write(self.write_offset, &header.to_bytes());
        m.write(self.write_offset + CcbCmdHeader::SIZE as u32, payload);
        self.write_offset += sz;
        Ok(())
    }

    /// An `UPDATE` command: the firmware writes `ufo.value` to `ufo.addr` when it reaches it.
    pub fn write_update(
        &mut self,
        m: &mut impl CccbMemory,
        ufo: fwif::Ufo,
        job_ref: u32,
    ) -> Result<(), CccbError> {
        let mut p = [0u8; 8];
        p[0..4].copy_from_slice(&ufo.addr.to_le_bytes());
        p[4..8].copy_from_slice(&ufo.value.to_le_bytes());
        self.write_command(m, fwif::ccb::UPDATE, &p, job_ref, job_ref)
    }

    /// A `FENCE_PR` command: the firmware waits until every word holds its value.
    pub fn write_fence(
        &mut self,
        m: &mut impl CccbMemory,
        ufos: &[fwif::Ufo],
        job_ref: u32,
    ) -> Result<(), CccbError> {
        let mut p = [0u8; 8 * 64];
        if ufos.is_empty() || ufos.len() > 64 {
            return Err(CccbError::TooLarge);
        }
        for (i, u) in ufos.iter().enumerate() {
            p[i * 8..i * 8 + 4].copy_from_slice(&u.addr.to_le_bytes());
            p[i * 8 + 4..i * 8 + 8].copy_from_slice(&u.value.to_le_bytes());
        }
        self.write_command(m, fwif::ccb::FENCE_PR, &p[..ufos.len() * 8], job_ref, job_ref)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use alloc::vec::Vec;

    struct KMem {
        rd: u32,
        wr: u32,
        slots: Vec<[u8; 88]>,
        rtn: Vec<u32>,
    }

    impl KMem {
        fn new() -> Self {
            Self { rd: 0, wr: 0, slots: vec![[0; 88]; 128], rtn: vec![0xEE; 128] }
        }
        /// The scripted firmware consumes `n` commands and marks them executed.
        fn firmware_runs(&mut self, n: u32) {
            for _ in 0..n {
                let s = self.rd as usize;
                assert_ne!(self.rd, self.wr, "the firmware never reads past the write offset");
                self.rtn[s] = fwif::kccb::RTN_SLOT_CMD_EXECUTED;
                self.rd = (self.rd + 1) & 127;
            }
        }
    }

    impl KccbMemory for KMem {
        fn read_offset(&self) -> u32 {
            self.rd
        }
        fn set_write_offset(&mut self, offset: u32) {
            self.wr = offset;
        }
        fn write_slot(&mut self, slot: u32, cmd: &[u8; 88]) {
            self.slots[slot as usize] = *cmd;
        }
        fn rtn(&self, slot: u32) -> u32 {
            self.rtn[slot as usize]
        }
        fn set_rtn(&mut self, slot: u32, value: u32) {
            self.rtn[slot as usize] = value;
        }
    }

    #[test]
    fn kernel_commands_take_slots_in_order_and_the_return_word_says_executed() {
        let mut m = KMem::new();
        let mut k = Kccb::new();
        let s0 = k.send(&mut m, &KccbCmd::health_check()).unwrap();
        let s1 = k.send(&mut m, &KccbCmd::health_check()).unwrap();
        assert_eq!((s0, s1), (0, 1));
        assert_eq!(m.wr, 2);
        assert_eq!(m.rtn[0], 0, "cleared before the firmware answers");
        assert!(!k.executed(&m, s0));
        assert!(!k.idle(&m));
        m.firmware_runs(1);
        assert!(k.executed(&m, s0));
        assert!(!k.executed(&m, s1));
        m.firmware_runs(1);
        assert!(k.idle(&m));
        assert_eq!(&m.slots[0][0..4], &fwif::kccb::HEALTH_CHECK.to_le_bytes());
    }

    #[test]
    fn test_reject_kernel_command_when_every_slot_is_held() {
        let mut m = KMem::new();
        let mut k = Kccb::new();
        for _ in 0..127 {
            k.send(&mut m, &KccbCmd::health_check()).unwrap();
        }
        assert_eq!(k.used(&m), 127);
        assert_eq!(k.send(&mut m, &KccbCmd::health_check()), Err(KccbError::Full));
        m.firmware_runs(1);
        assert_eq!(k.send(&mut m, &KccbCmd::health_check()), Ok(127));
        // Wrapped: the next slot is 0 again.
        m.firmware_runs(1);
        assert_eq!(k.send(&mut m, &KccbCmd::health_check()), Ok(0));
    }

    struct FMem {
        rd: u32,
        wr: u32,
        slots: Vec<[u8; 88]>,
    }

    impl FwccbMemory for FMem {
        fn write_offset(&self) -> u32 {
            self.wr
        }
        fn read_offset(&self) -> u32 {
            self.rd
        }
        fn set_read_offset(&mut self, offset: u32) {
            self.rd = offset;
        }
        fn read_slot(&self, slot: u32) -> [u8; 88] {
            self.slots[slot as usize]
        }
    }

    #[test]
    fn firmware_messages_are_read_in_order_and_the_read_offset_follows() {
        let mut slots = vec![[0u8; 88]; 32];
        slots[31][0..4].copy_from_slice(&fwif::fwccb::UPDATE_STATS.to_le_bytes());
        slots[0][0..4].copy_from_slice(&fwif::fwccb::REQUEST_GPU_RESTART.to_le_bytes());
        let mut m = FMem { rd: 31, wr: 1, slots };
        let f = Fwccb;
        assert_eq!(f.next(&mut m).map(|c| c.cmd_type), Some(fwif::fwccb::UPDATE_STATS));
        assert_eq!(m.rd, 0, "wrapped");
        assert_eq!(f.next(&mut m).map(|c| c.cmd_type), Some(fwif::fwccb::REQUEST_GPU_RESTART));
        assert!(f.next(&mut m).is_none());
    }

    struct CMem {
        rd: u32,
        bytes: Vec<u8>,
    }

    impl CccbMemory for CMem {
        fn read_offset(&self) -> u32 {
            self.rd
        }
        fn write(&mut self, offset: u32, bytes: &[u8]) {
            let o = offset as usize;
            self.bytes[o..o + bytes.len()].copy_from_slice(bytes);
        }
    }

    fn header_at(m: &CMem, off: usize) -> (u32, u32) {
        let w = |o: usize| u32::from_le_bytes(m.bytes[o..o + 4].try_into().unwrap());
        (w(off), w(off + 4))
    }

    #[test]
    fn a_client_command_is_header_plus_aligned_payload() {
        let mut m = CMem { rd: 0, bytes: vec![0; 32768] };
        let mut c = Cccb::new();
        c.write_update(&mut m, fwif::Ufo { addr: 0xC000_1000, value: 7 }, 1).unwrap();
        assert_eq!(c.write_offset(), 48);
        assert_eq!(header_at(&m, 0), (fwif::ccb::UPDATE, 8));
        assert_eq!(&m.bytes[40..44], &0xC000_1000u32.to_le_bytes());
        assert_eq!(&m.bytes[44..48], &7u32.to_le_bytes());
        // A 5-byte payload is padded to 8.
        c.write_command(&mut m, fwif::ccb::NULL, &[1, 2, 3, 4, 5], 2, 2).unwrap();
        assert_eq!(c.write_offset(), 48 + 48);
        assert_eq!(header_at(&m, 48), (fwif::ccb::NULL, 8));
    }

    #[test]
    fn a_command_never_wraps_a_padding_command_fills_the_tail() {
        let mut m = CMem { rd: 0, bytes: vec![0; 32768] };
        let mut c = Cccb::new();
        // Fill to 32768 − 64: a 48-byte command would leave 16 bytes — less than a header.
        let big = [0u8; 32768 - 64 - 40];
        c.write_command(&mut m, fwif::ccb::NULL, &big, 0, 0).unwrap();
        assert_eq!(c.write_offset(), 32768 - 64);
        // The firmware consumed everything so far (room exists), the next command must pad.
        m.rd = 32768 - 64;
        c.write_update(&mut m, fwif::Ufo { addr: 1, value: 2 }, 3).unwrap();
        assert_eq!(header_at(&m, 32768 - 64), (fwif::ccb::PADDING, 64 - 40));
        assert_eq!(header_at(&m, 0), (fwif::ccb::UPDATE, 8));
        assert_eq!(c.write_offset(), 48);
    }

    #[test]
    fn test_reject_client_command_that_does_not_fit() {
        let mut m = CMem { rd: 0, bytes: vec![0; 32768] };
        let mut c = Cccb::new();
        let huge = [0u8; 32768];
        assert_eq!(c.write_command(&mut m, fwif::ccb::NULL, &huge, 0, 0), Err(CccbError::TooLarge));
        // The largest command the ring can ever hold: header + payload = size − header (the
        // wrap rule needs a header's worth of tail). One byte more never fits.
        let too_big = [0u8; 32768 - 40 - 40 + 8];
        assert_eq!(
            c.write_command(&mut m, fwif::ccb::NULL, &too_big, 0, 0),
            Err(CccbError::TooLarge)
        );
        let largest = [0u8; 32768 - 40 - 40];
        assert_eq!(c.write_command(&mut m, fwif::ccb::NULL, &largest, 0, 0), Ok(()));
        assert_eq!(c.write_offset(), 32768 - 40);
        assert!(!c.fits(&m, 48), "nothing consumed, nothing fits");
        assert_eq!(
            c.write_update(&mut m, fwif::Ufo { addr: 1, value: 1 }, 0),
            Err(CccbError::Full)
        );
        m.rd = c.write_offset();
        assert!(c.fits(&m, 48));
        assert_eq!(c.write_update(&mut m, fwif::Ufo { addr: 1, value: 1 }, 0), Ok(()));
        assert_eq!(c.write_offset(), 48, "padded through the tail, wrapped");
    }

    #[test]
    fn test_reject_fence_with_no_or_too_many_words() {
        let mut m = CMem { rd: 0, bytes: vec![0; 32768] };
        let mut c = Cccb::new();
        assert_eq!(c.write_fence(&mut m, &[], 0), Err(CccbError::TooLarge));
        let many = [fwif::Ufo { addr: 1, value: 1 }; 65];
        assert_eq!(c.write_fence(&mut m, &many, 0), Err(CccbError::TooLarge));
        assert_eq!(c.write_fence(&mut m, &many[..2], 0), Ok(()));
        assert_eq!(header_at(&m, 0), (fwif::ccb::FENCE_PR, 16));
    }
}
