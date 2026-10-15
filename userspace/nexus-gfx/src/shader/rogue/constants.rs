// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The shader core's constant registers (TASK-0215 5c-2 P5a): the special bank's fixed values as
//! the open compiler's description lists them — 96 values at their register index, some read
//! through the floor or negate modifier. A program's constant is one of these or comes from its
//! push constants; the backend reads a plain one as its register (`fadd.sat x, sc0`, `fmad x,
//! sc75, y` in the compiler's own code, `tests/rogue_asm.rs`) and builds a modified one once.

/// One constant register value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConstReg {
    /// The value's bits (an integer, or a float's bits).
    pub bits: u32,
    /// Its register in the special bank.
    pub index: u16,
    /// Read through the floor modifier.
    pub flr: bool,
    /// Read through the negate modifier.
    pub neg: bool,
}

const fn c(bits: u32, index: u16, flr: bool, neg: bool) -> ConstReg {
    ConstReg { bits, index, flr, neg }
}

/// The table, in the description's order.
pub const TABLE: [ConstReg; 96] = [
    c(0x00000000, 0, false, false),   // 0
    c(0x00000001, 1, false, false),   // 1
    c(0x00000002, 2, false, false),   // 2
    c(0x00000003, 3, false, false),   // 3
    c(0x00000004, 4, false, false),   // 4
    c(0x00000005, 5, false, false),   // 5
    c(0x00000006, 6, false, false),   // 6
    c(0x00000007, 7, false, false),   // 7
    c(0x00000008, 8, false, false),   // 8
    c(0x00000009, 9, false, false),   // 9
    c(0x0000000a, 10, false, false),  // 10
    c(0x0000000b, 11, false, false),  // 11
    c(0x0000000c, 12, false, false),  // 12
    c(0x0000000d, 13, false, false),  // 13
    c(0x0000000e, 14, false, false),  // 14
    c(0x0000000f, 15, false, false),  // 15
    c(0x00000010, 16, false, false),  // 16
    c(0x00000011, 17, false, false),  // 17
    c(0x00000012, 18, false, false),  // 18
    c(0x00000013, 19, false, false),  // 19
    c(0x00000014, 20, false, false),  // 20
    c(0x00000015, 21, false, false),  // 21
    c(0x00000016, 22, false, false),  // 22
    c(0x00000017, 23, false, false),  // 23
    c(0x00000018, 24, false, false),  // 24
    c(0x00000019, 25, false, false),  // 25
    c(0x0000001a, 26, false, false),  // 26
    c(0x0000001b, 27, false, false),  // 27
    c(0x0000001c, 28, false, false),  // 28
    c(0x0000001d, 29, false, false),  // 29
    c(0x0000001e, 30, false, false),  // 30
    c(0x0000001f, 31, false, false),  // 31
    c(0x0000007f, 147, false, false), // 127
    c(0x37800000, 134, false, false), // 1.52588e-05
    c(0x38000000, 135, false, false), // 3.05176e-05
    c(0x38800000, 88, false, false),  // 6.10352e-05
    c(0x39000000, 87, false, false),  // 0.00012207
    c(0x39800000, 86, false, false),  // 0.000244141
    c(0x3a000000, 85, false, false),  // 0.000488281
    c(0x3a800000, 84, false, false),  // 0.000976562
    c(0x3b000000, 83, false, false),  // 0.00195312
    c(0x3b4d2e1c, 136, false, false), // 0.0031308
    c(0x3b800000, 82, false, false),  // 0.00390625
    c(0x3c000000, 81, false, false),  // 0.0078125
    c(0x3c800000, 80, false, false),  // 0.015625
    c(0x3d000000, 79, false, false),  // 0.03125
    c(0x3d25aee6, 156, false, false), // 0.04045
    c(0x3d6147ae, 140, false, false), // 0.055
    c(0x3d800000, 78, false, false),  // 0.0625
    c(0x3d9e8391, 157, false, false), // 0.0773994
    c(0x3e000000, 77, false, false),  // 0.125
    c(0x3e2aaaab, 153, false, false), // 0.166667
    c(0x3e800000, 76, false, false),  // 0.25
    c(0x3e9a209b, 145, false, false), // 0.30103
    c(0x3ea2f983, 128, false, false), // 0.31831
    c(0x3eaaaaab, 152, false, false), // 0.333333
    c(0x3ebc5ab2, 90, false, false),  // 0.367879
    c(0x3ed55555, 138, false, false), // 0.416667
    c(0x3f000000, 75, false, false),  // 0.5
    c(0x3f22f983, 129, false, false), // 0.63662
    c(0x3f317218, 146, false, false), // 0.693147
    c(0x3f3504f3, 92, false, false),  // 0.707107
    c(0x3f490fdb, 93, false, false),  // 0.785398
    c(0x3f72a76f, 158, false, false), // 0.947867
    c(0x3f800000, 64, false, false),  // 1
    c(0x3f860a92, 151, false, false), // 1.0472
    c(0x3f870a3d, 139, false, false), // 1.055
    c(0x3fa2f983, 130, false, false), // 1.27324
    c(0x3fb504f3, 91, false, false),  // 1.41421
    c(0x3fb8aa3b, 155, false, false), // 1.4427
    c(0x3fc90fdb, 94, false, false),  // 1.5708
    c(0x40000000, 65, false, false),  // 2
    c(0x4019999a, 159, false, false), // 2.4
    c(0x402df854, 89, false, false),  // 2.71828
    c(0x40490fdb, 95, false, false),  // 3.14159
    c(0x40549a78, 154, false, false), // 3.32193
    c(0x40800000, 66, false, false),  // 4
    c(0x40c90fdb, 131, false, false), // 6.28319
    c(0x41000000, 67, false, false),  // 8
    c(0x41490fdb, 132, false, false), // 12.5664
    c(0x414eb852, 137, false, false), // 12.92
    c(0x41800000, 68, false, false),  // 16
    c(0x41c90fdb, 133, false, false), // 25.1327
    c(0x42000000, 69, false, false),  // 32
    c(0x42800000, 70, false, false),  // 64
    c(0x43000000, 71, false, false),  // 128
    c(0x43800000, 72, false, false),  // 256
    c(0x44000000, 73, false, false),  // 512
    c(0x44800000, 74, false, false),  // 1024
    c(0x4b000000, 149, false, false), // 8.38861e+06
    c(0x4b800000, 150, false, false), // 1.67772e+07
    c(0x7f7fffff, 148, false, false), // 3.40282e+38
    c(0x7f800000, 142, false, false), // inf
    c(0x7fff7fff, 144, false, false), // nan
    c(0x80000000, 141, false, false), // -0
    c(0xffffffff, 143, false, false), // nan
];

/// The register holding `bits`, a plain one before a modified one.
pub fn find(bits: u32) -> Option<ConstReg> {
    let plain = TABLE.iter().find(|r| r.bits == bits && !r.flr && !r.neg);
    plain.or_else(|| TABLE.iter().find(|r| r.bits == bits)).copied()
}
