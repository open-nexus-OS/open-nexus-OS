// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: a configuration space whose functions decode like hardware: a BAR keeps the
//! address bits its size allows and its read-only type bits, reads all ones where nothing
//! answers, and every write is logged.
//! OWNERS: @runtime @drivers

use std::cell::RefCell;
use std::collections::BTreeMap;

use nexus_pci::config::{BAR0, CLASS, COMMAND, HEADER, ID, INTERRUPT};
use nexus_pci::{Bdf, ConfigSpace};

/// A function: ids, class register, header byte, pin, and what each BAR decodes.
#[derive(Clone, Copy)]
pub struct Spec {
    pub id: u32,
    pub class: u32,
    pub header: u8,
    pub pin: u8,
    /// The value each BAR reads back after all ones (0: none).
    pub bars: [u32; 6],
}

pub const HOST_BRIDGE: Spec =
    Spec { id: 0x0008_1b36, class: 0x0600_0000, header: 0, pin: 0, bars: [0; 6] };
pub const SD_HOST: Spec = Spec {
    id: 0x0007_1b36,
    class: 0x0805_0100,
    header: 0,
    pin: 1,
    bars: [mem32(256), 0, 0, 0, 0, 0],
};

pub const fn mem32(size: u32) -> u32 {
    !(size - 1) & 0xFFFF_FFF0
}

/// A 64-bit BAR: (low, high) read-back values.
pub const fn mem64(size: u64, prefetchable: bool) -> (u32, u32) {
    let mask = !(size - 1);
    ((mask as u32 & 0xFFFF_FFF0) | 0x4 | if prefetchable { 0x8 } else { 0 }, (mask >> 32) as u32)
}

pub const fn io(size: u32) -> u32 {
    (!(size - 1) & 0xFFFF_FFFC) | 1
}

struct State {
    spec: Spec,
    bars: [u32; 6],
    command: u32,
}

#[derive(Default)]
pub struct Mock {
    functions: RefCell<BTreeMap<Bdf, State>>,
    pub writes: RefCell<Vec<(Bdf, u16, u32)>>,
}

impl Mock {
    pub fn with(functions: &[(u8, u8, Spec)]) -> Self {
        let mock = Self::default();
        for &(dev, func, spec) in functions {
            let state = State { spec, bars: [0; 6], command: 0 };
            mock.functions.borrow_mut().insert(Bdf { bus: 0, dev, func }, state);
        }
        mock
    }

    pub fn command(&self, dev: u8, func: u8) -> u32 {
        self.functions.borrow()[&Bdf { bus: 0, dev, func }].command
    }

    pub fn bar(&self, dev: u8, func: u8, i: usize) -> u32 {
        self.functions.borrow()[&Bdf { bus: 0, dev, func }].bars[i]
    }

    pub fn writes_to(&self, dev: u8) -> usize {
        self.writes.borrow().iter().filter(|(b, _, _)| b.dev == dev).count()
    }
}

impl ConfigSpace for Mock {
    fn read(&self, bdf: Bdf, reg: u16) -> u32 {
        let functions = self.functions.borrow();
        let Some(f) = functions.get(&bdf) else { return u32::MAX };
        match reg {
            ID => f.spec.id,
            COMMAND => f.command,
            CLASS => f.spec.class,
            HEADER => u32::from(f.spec.header) << 16,
            INTERRUPT => u32::from(f.spec.pin) << 8,
            r if (BAR0..BAR0 + 24).contains(&r) => f.bars[usize::from((r - BAR0) / 4)],
            _ => 0,
        }
    }

    fn write(&self, bdf: Bdf, reg: u16, value: u32) {
        self.writes.borrow_mut().push((bdf, reg, value));
        let mut functions = self.functions.borrow_mut();
        let Some(f) = functions.get_mut(&bdf) else { return };
        match reg {
            COMMAND => f.command = value & 0xFFFF,
            r if (BAR0..BAR0 + 24).contains(&r) => {
                let i = usize::from((r - BAR0) / 4);
                let decode = f.spec.bars[i];
                // A 64-bit BAR's high half: every bit of its read-back is writable.
                let high =
                    i > 0 && (f.spec.bars[i - 1] >> 1) & 3 == 2 && f.spec.bars[i - 1] & 1 == 0;
                let (writable, fixed) = if high {
                    (decode, 0)
                } else if decode & 1 != 0 {
                    (decode & !0x3, decode & 0x3)
                } else {
                    (decode & !0xF, decode & 0xF)
                };
                f.bars[i] = (value & writable) | fixed;
            }
            _ => {}
        }
    }
}
