// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the vector side of the unit soak (`unitsoak`, RFC-0119): a thread's pattern in
//!   every byte of `v0`–`v31`, in the control register, and in the length and type it set —
//!   the two threads share no byte, no length and no type. The registers are written and
//!   read whole (`vl1re8.v`, `vs1r.v`), so the pattern covers every byte whatever the length
//!   says; the comparison is in Rust over the bytes read back. The routines below are the
//!   only vector instructions of this program.
//!   Run only when the kernel's answer names the vector unit; the buffers hold registers of
//!   the width the answer states, and `holds` reads this thread's own width first.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU marker ladder (just test-os) and the board — `SELFTEST: vsoak ok`.
//! RFC: docs/rfcs/RFC-0119-a-thread-s-floating-point-and-vector-registers-are-its-own-kept-from-first-use.md

use core::sync::atomic::{AtomicUsize, Ordering};

use super::unitsoak::Who;

core::arch::global_asm!(
    ".option push",
    ".option arch, +v",
    // The control registers by number: 0x00f vcsr, 0xc20 vl, 0xc21 vtype, 0xc22 vlenb.
    // __vsoak_seed(a0 = the bytes of 32 registers, a1 = length, a2 = type, a3 = control).
    ".globl __vsoak_seed",
    ".type __vsoak_seed, @function",
    "__vsoak_seed:",
    "csrr t2, 0xc22",
    ".irp n, 0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31",
    r"vl1re8.v v\n, (a0)",
    "add a0, a0, t2",
    ".endr",
    "vsetvl x0, a1, a2",
    "csrw 0x00f, a3",
    "ret",
    // __vsoak_read(a0 = room for the bytes of 32 registers, a1 = three words: the length,
    // the type, the control register).
    ".globl __vsoak_read",
    ".type __vsoak_read, @function",
    "__vsoak_read:",
    "csrr t0, 0xc20",
    "sd t0, 0(a1)",
    "csrr t0, 0xc21",
    "sd t0, 8(a1)",
    "csrr t0, 0x00f",
    "sd t0, 16(a1)",
    "csrr t2, 0xc22",
    ".irp n, 0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31",
    r"vs1r.v v\n, (a0)",
    "add a0, a0, t2",
    ".endr",
    "ret",
    // __vsoak_bytes() -> a0: the width of this hart's vector registers in bytes.
    ".globl __vsoak_bytes",
    ".type __vsoak_bytes, @function",
    "__vsoak_bytes:",
    "csrr a0, 0xc22",
    "ret",
    ".option pop",
);

extern "C" {
    fn __vsoak_seed(bytes: *const u8, length: u64, kind: u64, control: u64);
    fn __vsoak_read(bytes: *mut u8, words: *mut [u64; 3]);
    fn __vsoak_bytes() -> usize;
}

const REGISTERS: usize = 32;
/// The widest register the soak has room for — the widest the kernel keeps (RFC-0119).
const MAX_BYTES: usize = 256;
/// Bits of `differs` past the registers: the control register, the length, the type.
const CONTROL_DIFFERS: u64 = 1 << 32;
const LENGTH_DIFFERS: u64 = 1 << 33;
const TYPE_DIFFERS: u64 = 1 << 34;

/// The width of a register in bytes, from the kernel's answer (`holds`).
static BYTES: AtomicUsize = AtomicUsize::new(0);
/// Each thread's registers in memory: what it loads its pattern from and reads them back to.
static mut ROOM: [[u8; REGISTERS * MAX_BYTES]; 2] = [[0; REGISTERS * MAX_BYTES]; 2];

/// A thread's pattern beside the bytes: a type (element width, tail and mask policy), a
/// length below that type's maximum, and a control register (rounding mode, saturation).
#[derive(Clone, Copy)]
struct Pattern {
    salt: usize,
    kind: u64,
    /// The bytes of one element of the type.
    element_bytes: usize,
    /// The length is the most elements one register holds, less this.
    short_by: usize,
    control: u64,
}

const fn pattern(who: Who) -> Pattern {
    match who {
        // 8-bit elements, tail and mask agnostic; rounding mode 2, saturated.
        Who::Main => {
            Pattern { salt: 0x5A, kind: 0xC0, element_bytes: 1, short_by: 3, control: 0b101 }
        }
        // 16-bit elements, tail and mask undisturbed; rounding mode 1.
        Who::Partner => {
            Pattern { salt: 0xC3, kind: 0x08, element_bytes: 2, short_by: 1, control: 0b010 }
        }
    }
}

impl Pattern {
    const fn byte(self, register: usize, index: usize) -> u8 {
        self.salt.wrapping_add(register * 41).wrapping_add(index * 7) as u8
    }

    const fn length(self, bytes: usize) -> u64 {
        (bytes / self.element_bytes - self.short_by) as u64
    }
}

/// The width this thread reads from its own registers — the soak runs when it is the
/// `stated` one. 0, without a read, for a stated width the soak has no room for.
pub(super) fn holds(stated: usize) -> usize {
    if stated > MAX_BYTES || stated < 16 {
        return 0;
    }
    // SAFETY: reads a control register of the vector unit, which the kernel's answer names.
    let own = unsafe { __vsoak_bytes() };
    BYTES.store(if own == stated { own } else { 0 }, Ordering::Release);
    own
}

fn room(who: Who) -> *mut [u8; REGISTERS * MAX_BYTES] {
    // SAFETY: an address only; each thread of a soak touches its own half.
    unsafe { core::ptr::addr_of_mut!(ROOM[who as usize]) }
}

/// Writes `who`'s pattern into `v0`–`v31`, the length and type, and the control register.
pub(super) fn seed(who: Who) {
    let bytes = BYTES.load(Ordering::Acquire);
    let pattern = pattern(who);
    // SAFETY: `who`'s half is this thread's alone (one main thread, one partner).
    let room = unsafe { &mut *room(who) };
    for (at, byte) in room[..REGISTERS * bytes].iter_mut().enumerate() {
        *byte = pattern.byte(at / bytes.max(1), at % bytes.max(1));
    }
    // SAFETY: loads 32 registers of `bytes` each from the room filled above (`holds`
    // checked that the registers are `bytes` wide) and sets the unit's control state.
    unsafe { __vsoak_seed(room.as_ptr(), pattern.length(bytes), pattern.kind, pattern.control) }
}

/// The registers that differ from `who`'s pattern (bit n = `v<n>`, then the control
/// register, the length, the type).
pub(super) fn differs(who: Who) -> u64 {
    let bytes = BYTES.load(Ordering::Acquire);
    let pattern = pattern(who);
    // SAFETY: as in `seed`.
    let room = unsafe { &mut *room(who) };
    let mut words = [0u64; 3];
    // SAFETY: stores 32 registers of `bytes` each into the room and three words.
    unsafe { __vsoak_read(room.as_mut_ptr(), &mut words) };
    let mut differs = 0;
    for (register, read) in room[..REGISTERS * bytes].chunks_exact(bytes.max(1)).enumerate() {
        let same =
            read.iter().enumerate().all(|(index, &byte)| byte == pattern.byte(register, index));
        differs |= u64::from(!same) << register;
    }
    let [length, kind, control] = words;
    differs
        | if control == pattern.control { 0 } else { CONTROL_DIFFERS }
        | if length == pattern.length(bytes) { 0 } else { LENGTH_DIFFERS }
        | if kind == pattern.kind { 0 } else { TYPE_DIFFERS }
}
