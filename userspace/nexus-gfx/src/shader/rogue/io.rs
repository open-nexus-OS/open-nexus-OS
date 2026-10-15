// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! A group's inputs and outputs: the lower sources (s0–s2, with the first internal-source mux),
//! the upper sources (s3–s5), the internal-source selects (is1–is5), the destinations (the two
//! write ports). Each part is self-describing: its first bytes' extension bits name its form,
//! from one byte (one source, bank 1 bit, index 6 bits) to six (three sources with 11-bit
//! indices). A main or bitwise group always carries both source parts, a main group the select
//! byte; destinations follow the header's write-port bits.

use super::bits::{flag, get, pc, put, Field};
use super::RogueError;

/// A register as a part names it: its bank and its index.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reg {
    pub bank: u8,
    pub index: u16,
}

/// The register banks.
pub mod bank {
    pub const SPECIAL: u8 = 0;
    pub const TEMP: u8 = 1;
    pub const VTXIN: u8 = 2;
    pub const COEFF: u8 = 3;
    pub const SHARED: u8 = 4;
    pub const COEFF_ALT: u8 = 5;
    pub const IDX0: u8 = 6;
    pub const IDX1: u8 = 7;
}

/// The source part's forms: sources, then per source the bank and index widths.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SrcForm {
    One1b6i,
    One3b11i,
    Two1b6i1b5i,
    Two2b7i2b7i,
    Two3b11i2b8i,
    Three2b7i2b7i2b6i,
    Three3b8i2b8i3b8i,
    Three3b11i2b8i3b11i,
}

/// A source part: lower (s0–s2 and the mux) or upper (s3–s5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Srcs {
    pub form: SrcForm,
    pub upper: bool,
    /// The sources the form carries, first to last; the rest stay default.
    pub regs: [Reg; 3],
    /// The first internal source's selection (lower forms with a mux only).
    pub mux: u8,
}

// The source fields.
const EXT0: Field = &[pc(0, 7, 7)];
const SBA_1B0: Field = &[pc(0, 6, 6)];
const SA_6B0: Field = &[pc(0, 5, 0)];
const SEL: Field = &[pc(1, 7, 7)];
const EXT1: Field = &[pc(1, 6, 6)];
const MUX_2B1: Field = &[pc(1, 5, 4)];
const SBA_3B1: Field = &[pc(1, 3, 2), pc(0, 6, 6)];
const SA_11B2: Field = &[pc(2, 2, 0), pc(1, 1, 0), pc(0, 5, 0)];
const RSVD2: Field = &[pc(2, 7, 3)];
const SBB_1B1: Field = &[pc(1, 5, 5)];
const SB_5B1: Field = &[pc(1, 4, 0)];
const EXT2: Field = &[pc(2, 7, 7)];
const MUX_2B2: Field = &[pc(2, 6, 5)];
const SBA_2B2: Field = &[pc(2, 4, 4), pc(0, 6, 6)];
const SBB_2B2: Field = &[pc(2, 3, 3), pc(1, 5, 5)];
const SA_7B2: Field = &[pc(2, 2, 2), pc(0, 5, 0)];
const SB_7B2: Field = &[pc(2, 1, 0), pc(1, 4, 0)];
const SA_11B3: Field = &[pc(3, 7, 5), pc(3, 1, 1), pc(2, 2, 2), pc(0, 5, 0)];
const MUX_3B3: Field = &[pc(3, 4, 4), pc(2, 6, 5)];
const SBA_3B3: Field = &[pc(3, 3, 3), pc(2, 4, 4), pc(0, 6, 6)];
const RSVD3: Field = &[pc(3, 2, 2)];
const SB_8B3: Field = &[pc(3, 0, 0), pc(2, 1, 0), pc(1, 4, 0)];
const SBC_2B3: Field = &[pc(3, 7, 6)];
const SC_6B3: Field = &[pc(3, 5, 0)];
const RSVD4: Field = &[pc(4, 7, 7)];
const SBC_3B4: Field = &[pc(4, 7, 7), pc(3, 7, 6)];
const SC_8B4: Field = &[pc(4, 6, 5), pc(3, 5, 0)];
const MUX_3B4: Field = &[pc(4, 4, 4), pc(2, 6, 5)];
const SBA_3B4: Field = &[pc(4, 3, 3), pc(2, 4, 4), pc(0, 6, 6)];
const EXT4: Field = &[pc(4, 2, 2)];
const SA_8B4: Field = &[pc(4, 1, 1), pc(2, 2, 2), pc(0, 5, 0)];
const SB_8B4: Field = &[pc(4, 0, 0), pc(2, 1, 0), pc(1, 4, 0)];
const RSVD5: Field = &[pc(5, 7, 6)];
const RSVD5C: Field = &[pc(5, 5, 3)];
const SC_11B5: Field = &[pc(5, 5, 3), pc(4, 6, 5), pc(3, 5, 0)];
const SA_11B5: Field = &[pc(5, 2, 0), pc(4, 1, 1), pc(2, 2, 2), pc(0, 5, 0)];

/// A form's layout: its size, its selector and reserved bits, its sources' fields, its mux.
struct Layout {
    size: usize,
    fixed: &'static [(Field, u32)],
    regs: &'static [(Field, Field)],
    mux: Option<Field>,
}

const fn lower(form: SrcForm) -> Layout {
    match form {
        SrcForm::One1b6i => {
            Layout { size: 1, fixed: &[(EXT0, 0)], regs: &[(SBA_1B0, SA_6B0)], mux: None }
        }
        SrcForm::One3b11i => Layout {
            size: 3,
            fixed: &[(EXT0, 1), (SEL, 0), (EXT1, 0), (RSVD2, 0)],
            regs: &[(SBA_3B1, SA_11B2)],
            mux: Some(MUX_2B1),
        },
        SrcForm::Two1b6i1b5i => Layout {
            size: 2,
            fixed: &[(EXT0, 1), (SEL, 1), (EXT1, 0)],
            regs: &[(SBA_1B0, SA_6B0), (SBB_1B1, SB_5B1)],
            mux: None,
        },
        SrcForm::Two2b7i2b7i => Layout {
            size: 3,
            fixed: &[(EXT0, 1), (SEL, 1), (EXT1, 1), (EXT2, 0)],
            regs: &[(SBA_2B2, SA_7B2), (SBB_2B2, SB_7B2)],
            mux: Some(MUX_2B2),
        },
        SrcForm::Two3b11i2b8i => Layout {
            size: 4,
            fixed: &[(EXT0, 1), (SEL, 1), (EXT1, 1), (EXT2, 1), (RSVD3, 0)],
            regs: &[(SBA_3B3, SA_11B3), (SBB_2B2, SB_8B3)],
            mux: Some(MUX_3B3),
        },
        SrcForm::Three2b7i2b7i2b6i => Layout {
            size: 4,
            fixed: &[(EXT0, 1), (SEL, 0), (EXT1, 1), (EXT2, 0)],
            regs: &[(SBA_2B2, SA_7B2), (SBB_2B2, SB_7B2), (SBC_2B3, SC_6B3)],
            mux: Some(MUX_2B2),
        },
        SrcForm::Three3b8i2b8i3b8i => Layout {
            size: 5,
            fixed: &[(EXT0, 1), (SEL, 0), (EXT1, 1), (EXT2, 1), (EXT4, 0)],
            regs: &[(SBA_3B4, SA_8B4), (SBB_2B2, SB_8B4), (SBC_3B4, SC_8B4)],
            mux: Some(MUX_3B4),
        },
        SrcForm::Three3b11i2b8i3b11i => Layout {
            size: 6,
            fixed: &[(EXT0, 1), (SEL, 0), (EXT1, 1), (EXT2, 1), (EXT4, 1), (RSVD5, 0)],
            regs: &[(SBA_3B4, SA_11B5), (SBB_2B2, SB_8B4), (SBC_3B4, SC_11B5)],
            mux: Some(MUX_3B4),
        },
    }
}

/// The upper forms: the lower layouts without the mux (its bits zero); the three-source forms
/// with a 2-bit third bank and an 8-bit third index.
const fn upper(form: SrcForm) -> Layout {
    match form {
        SrcForm::One3b11i => Layout {
            size: 3,
            fixed: &[(EXT0, 1), (SEL, 0), (EXT1, 0), (MUX_2B1, 0), (RSVD2, 0)],
            regs: &[(SBA_3B1, SA_11B2)],
            mux: None,
        },
        SrcForm::Two2b7i2b7i => Layout {
            size: 3,
            fixed: &[(EXT0, 1), (SEL, 1), (EXT1, 1), (EXT2, 0), (MUX_2B2, 0)],
            regs: &[(SBA_2B2, SA_7B2), (SBB_2B2, SB_7B2)],
            mux: None,
        },
        SrcForm::Two3b11i2b8i => Layout {
            size: 4,
            fixed: &[(EXT0, 1), (SEL, 1), (EXT1, 1), (EXT2, 1), (RSVD3, 0), (MUX_3B3, 0)],
            regs: &[(SBA_3B3, SA_11B3), (SBB_2B2, SB_8B3)],
            mux: None,
        },
        SrcForm::Three2b7i2b7i2b6i => Layout {
            size: 4,
            fixed: &[(EXT0, 1), (SEL, 0), (EXT1, 1), (EXT2, 0), (MUX_2B2, 0)],
            regs: &[(SBA_2B2, SA_7B2), (SBB_2B2, SB_7B2), (SBC_2B3, SC_6B3)],
            mux: None,
        },
        SrcForm::Three3b8i2b8i3b8i => Layout {
            size: 5,
            fixed: &[
                (EXT0, 1),
                (SEL, 0),
                (EXT1, 1),
                (EXT2, 1),
                (EXT4, 0),
                (MUX_3B4, 0),
                (RSVD4, 0),
            ],
            regs: &[(SBA_3B4, SA_8B4), (SBB_2B2, SB_8B4), (SBC_2B3, SC_8B4)],
            mux: None,
        },
        SrcForm::Three3b11i2b8i3b11i => Layout {
            size: 6,
            fixed: &[
                (EXT0, 1),
                (SEL, 0),
                (EXT1, 1),
                (EXT2, 1),
                (EXT4, 1),
                (MUX_3B4, 0),
                (RSVD4, 0),
                (RSVD5, 0),
                (RSVD5C, 0),
            ],
            regs: &[(SBA_3B4, SA_11B5), (SBB_2B2, SB_8B4), (SBC_2B3, SC_8B4)],
            mux: None,
        },
        other => lower(other),
    }
}

impl SrcForm {
    /// The form the first bytes of a source part name.
    fn of(b: &[u8], at: usize) -> Result<Self, RogueError> {
        let need = |n: usize| if b.len() < n { Err(RogueError::Truncated { at }) } else { Ok(()) };
        need(1)?;
        if !flag(b, EXT0) {
            return Ok(Self::One1b6i);
        }
        need(2)?;
        let (sel, ext1) = (flag(b, SEL), flag(b, EXT1));
        if !ext1 {
            return Ok(if sel { Self::Two1b6i1b5i } else { Self::One3b11i });
        }
        need(3)?;
        if !flag(b, EXT2) {
            return Ok(if sel { Self::Two2b7i2b7i } else { Self::Three2b7i2b7i2b6i });
        }
        if sel {
            return Ok(Self::Two3b11i2b8i);
        }
        need(5)?;
        Ok(if flag(b, EXT4) { Self::Three3b11i2b8i3b11i } else { Self::Three3b8i2b8i3b8i })
    }
}

impl Srcs {
    /// Bytes the part takes.
    pub fn size(&self) -> usize {
        self.layout().size
    }

    fn layout(&self) -> Layout {
        if self.upper {
            upper(self.form)
        } else {
            lower(self.form)
        }
    }

    /// Decodes a source part at the start of `b`.
    pub fn decode(b: &[u8], upper_part: bool, at: usize) -> Result<Self, RogueError> {
        let form = SrcForm::of(b, at)?;
        let mut s = Self { form, upper: upper_part, regs: [Reg::default(); 3], mux: 0 };
        let l = s.layout();
        if b.len() < l.size {
            return Err(RogueError::Truncated { at });
        }
        for (f, v) in l.fixed {
            if get(b, f) != *v {
                return Err(RogueError::Reserved { at, what: "source part" });
            }
        }
        for (i, (bank, index)) in l.regs.iter().enumerate() {
            s.regs[i] = Reg { bank: get(b, bank) as u8, index: get(b, index) as u16 };
        }
        if let Some(m) = l.mux {
            s.mux = get(b, m) as u8;
        }
        Ok(s)
    }

    /// Writes the part into `out[..self.size()]`.
    pub fn encode(&self, out: &mut [u8]) -> Result<(), RogueError> {
        let l = self.layout();
        let out =
            out.get_mut(..l.size).ok_or(RogueError::TooWide { what: "source part", value: 0 })?;
        out.fill(0);
        for (f, v) in l.fixed {
            put(out, f, *v, "source selector")?;
        }
        for (i, (bank, index)) in l.regs.iter().enumerate() {
            put(out, bank, u32::from(self.regs[i].bank), "source bank")?;
            put(out, index, u32::from(self.regs[i].index), "source index")?;
        }
        match l.mux {
            Some(m) => put(out, m, u32::from(self.mux), "source mux"),
            None if self.mux != 0 => {
                Err(RogueError::TooWide { what: "source mux", value: u32::from(self.mux) })
            }
            None => Ok(()),
        }
    }
}

/// The internal-source selects: is1–is5 (is0 is the lower part's mux).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Iss {
    pub is1: u8,
    pub is2: u8,
    pub is3: u8,
    pub is4: u8,
    pub is5: u8,
}

const IS5: Field = &[pc(0, 7, 6)];
const IS4: Field = &[pc(0, 5, 4)];
const IS3: Field = &[pc(0, 3, 2)];
const IS2: Field = &[pc(0, 1, 1)];
const IS1: Field = &[pc(0, 0, 0)];

impl Iss {
    pub fn decode(b: &[u8], at: usize) -> Result<Self, RogueError> {
        if b.is_empty() {
            return Err(RogueError::Truncated { at });
        }
        Ok(Self {
            is1: get(b, IS1) as u8,
            is2: get(b, IS2) as u8,
            is3: get(b, IS3) as u8,
            is4: get(b, IS4) as u8,
            is5: get(b, IS5) as u8,
        })
    }

    pub fn encode(&self, out: &mut [u8]) -> Result<(), RogueError> {
        let out = out.get_mut(..1).ok_or(RogueError::TooWide { what: "iss", value: 0 })?;
        out[0] = 0;
        put(out, IS1, u32::from(self.is1), "is1")?;
        put(out, IS2, u32::from(self.is2), "is2")?;
        put(out, IS3, u32::from(self.is3), "is3")?;
        put(out, IS4, u32::from(self.is4), "is4")?;
        put(out, IS5, u32::from(self.is5), "is5")
    }
}

/// The destination part's forms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DstForm {
    One1b6i,
    One3b11i,
    Two1b7i1b6i,
    Two3b8i3b8i,
    Two3b11i3b11i,
}

/// The destinations: one (the write port the header names) or both.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dests {
    pub form: DstForm,
    /// One destination: the register; two: write port 0's.
    pub d0: Reg,
    /// Two destinations: write port 1's.
    pub d1: Reg,
}

const DEXT0: Field = &[pc(0, 7, 7)];
const DBN_1B0: Field = &[pc(0, 6, 6)];
const DN_6B0: Field = &[pc(0, 5, 0)];
const DRSVD1: Field = &[pc(1, 7, 7)];
const DBN_3B1: Field = &[pc(1, 3, 2), pc(0, 6, 6)];
const DN_11B1: Field = &[pc(1, 6, 4), pc(1, 1, 0), pc(0, 5, 0)];
const DB0_1B0: Field = &[pc(0, 7, 7)];
const D0_7B0: Field = &[pc(0, 6, 0)];
const DEXT1: Field = &[pc(1, 7, 7)];
const DB1_1B1: Field = &[pc(1, 6, 6)];
const D1_6B1: Field = &[pc(1, 5, 0)];
const DEXT2: Field = &[pc(2, 7, 7)];
const DB0_3B2: Field = &[pc(2, 2, 1), pc(0, 7, 7)];
const D0_8B2: Field = &[pc(2, 0, 0), pc(0, 6, 0)];
const DB1_3B2: Field = &[pc(2, 6, 5), pc(1, 6, 6)];
const D1_8B2: Field = &[pc(2, 4, 3), pc(1, 5, 0)];
const DRSVD3: Field = &[pc(3, 7, 6)];
const D0_11B3: Field = &[pc(3, 2, 0), pc(2, 0, 0), pc(0, 6, 0)];
const D1_11B3: Field = &[pc(3, 5, 3), pc(2, 4, 3), pc(1, 5, 0)];

struct DstLayout {
    size: usize,
    fixed: &'static [(Field, u32)],
    d0: (Field, Field),
    d1: Option<(Field, Field)>,
}

const fn dst_layout(form: DstForm) -> DstLayout {
    match form {
        DstForm::One1b6i => {
            DstLayout { size: 1, fixed: &[(DEXT0, 0)], d0: (DBN_1B0, DN_6B0), d1: None }
        }
        DstForm::One3b11i => DstLayout {
            size: 2,
            fixed: &[(DEXT0, 1), (DRSVD1, 0)],
            d0: (DBN_3B1, DN_11B1),
            d1: None,
        },
        DstForm::Two1b7i1b6i => DstLayout {
            size: 2,
            fixed: &[(DEXT1, 0)],
            d0: (DB0_1B0, D0_7B0),
            d1: Some((DB1_1B1, D1_6B1)),
        },
        DstForm::Two3b8i3b8i => DstLayout {
            size: 3,
            fixed: &[(DEXT1, 1), (DEXT2, 0)],
            d0: (DB0_3B2, D0_8B2),
            d1: Some((DB1_3B2, D1_8B2)),
        },
        DstForm::Two3b11i3b11i => DstLayout {
            size: 4,
            fixed: &[(DEXT1, 1), (DEXT2, 1), (DRSVD3, 0)],
            d0: (DB0_3B2, D0_11B3),
            d1: Some((DB1_3B2, D1_11B3)),
        },
    }
}

impl Dests {
    pub fn size(&self) -> usize {
        dst_layout(self.form).size
    }

    /// Decodes the destinations: `dual` when the header names both write ports.
    pub fn decode(b: &[u8], dual: bool, at: usize) -> Result<Self, RogueError> {
        let need = |n: usize| if b.len() < n { Err(RogueError::Truncated { at }) } else { Ok(()) };
        need(1)?;
        let form = if !dual {
            if flag(b, DEXT0) {
                DstForm::One3b11i
            } else {
                DstForm::One1b6i
            }
        } else {
            need(2)?;
            if !flag(b, DEXT1) {
                DstForm::Two1b7i1b6i
            } else {
                need(3)?;
                if flag(b, DEXT2) {
                    DstForm::Two3b11i3b11i
                } else {
                    DstForm::Two3b8i3b8i
                }
            }
        };
        let l = dst_layout(form);
        need(l.size)?;
        for (f, v) in l.fixed {
            if get(b, f) != *v {
                return Err(RogueError::Reserved { at, what: "destination part" });
            }
        }
        let reg = |(bank, index): (Field, Field)| Reg {
            bank: get(b, bank) as u8,
            index: get(b, index) as u16,
        };
        Ok(Self { form, d0: reg(l.d0), d1: l.d1.map(reg).unwrap_or_default() })
    }

    pub fn encode(&self, out: &mut [u8]) -> Result<(), RogueError> {
        let l = dst_layout(self.form);
        let out =
            out.get_mut(..l.size).ok_or(RogueError::TooWide { what: "destinations", value: 0 })?;
        out.fill(0);
        for (f, v) in l.fixed {
            put(out, f, *v, "destination selector")?;
        }
        put(out, l.d0.0, u32::from(self.d0.bank), "destination bank")?;
        put(out, l.d0.1, u32::from(self.d0.index), "destination index")?;
        if let Some((bank, index)) = l.d1 {
            put(out, bank, u32::from(self.d1.bank), "destination bank")?;
            put(out, index, u32::from(self.d1.index), "destination index")?;
        }
        Ok(())
    }

    /// Whether the form carries both write ports.
    pub const fn dual(&self) -> bool {
        matches!(self.form, DstForm::Two1b7i1b6i | DstForm::Two3b8i3b8i | DstForm::Two3b11i3b11i)
    }
}
