// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The SoC as our loader leaves it against the stock system (a measurement, TASK-0328 U3):
//! at ready — before any bring-up — socd reads every APMU and MPMU word the stock dumps hold
//! (`docs/board/measurements/2026-09-22-stock-system/regmap-{apmu,mpmu}.txt`, `off: val` per
//! line, the stock desktop running) and prints the ones that differ, `off:ours/stock`, in
//! lines that fit the console. The diff is where a block our tables do not cover stays gated
//! (board cycle 7: both USB PHYs dead with every known gate open). Pure over `Bus`,
//! host-tested; nothing is written.

use core::fmt::Write as _;

use nexus_hal::Bus;

use crate::verdict::Marker;

/// The stock APMU window (`0xd428_2800`, 0x400 bytes).
pub const APMU: &str =
    include_str!("../../../../../docs/board/measurements/2026-09-22-stock-system/regmap-apmu.txt");
/// The stock MPMU window (`0xd405_0000`; our tree maps 0x209c of its 0x3000 bytes).
pub const MPMU: &str =
    include_str!("../../../../../docs/board/measurements/2026-09-22-stock-system/regmap-mpmu.txt");

/// Differences printed per window at most (a measurement line budget, not a truth bound: the
/// count in the head says how many there were).
pub const DIFFS_MAX: usize = 256;

/// The `(offset, word)` pairs of a dump.
pub fn words(dump: &str) -> impl Iterator<Item = (usize, u32)> + '_ {
    dump.lines().filter_map(|line| {
        let (off, val) = line.split_once(": ")?;
        Some((
            usize::from_str_radix(off.trim(), 16).ok()?,
            u32::from_str_radix(val.trim(), 16).ok()?,
        ))
    })
}

/// Every word of `dump` inside `len` of the window at `base` that reads unlike the stock, as
/// `socd: <name> vs stock (N differ) off:ours/stock …` lines through `emit`.
pub fn report<B: Bus>(
    name: &str,
    base: usize,
    len: usize,
    dump: &str,
    bus: &B,
    mut emit: impl FnMut(&str),
) {
    let mut diffs = 0usize;
    let mut printed = 0usize;
    let mut line = Marker::new();
    let mut head = true;
    let mut pending: [(usize, u32, u32); DIFFS_MAX] = [(0, 0, 0); DIFFS_MAX];
    for (off, stock) in words(dump) {
        if off + 4 > len {
            continue;
        }
        let ours = bus.read(base + off);
        if ours == stock {
            continue;
        }
        if diffs < DIFFS_MAX {
            pending[diffs] = (off, ours, stock);
        }
        diffs += 1;
    }
    if diffs == 0 {
        let _ = write!(line, "socd: {name} vs stock (0 differ)");
        emit(line.as_str());
        return;
    }
    for &(off, ours, stock) in pending.iter().take(diffs.min(DIFFS_MAX)) {
        if head {
            let _ = write!(line, "socd: {name} vs stock ({diffs} differ)");
            head = false;
        }
        if !line.push_fmt(format_args!(" {off:03x}:{ours:x}/{stock:x}")) {
            emit(line.as_str());
            line = Marker::new();
            let _ = write!(line, "socd: {name} vs stock (cont)");
            let _ = line.push_fmt(format_args!(" {off:03x}:{ours:x}/{stock:x}"));
        }
        printed += 1;
    }
    if printed > 0 {
        emit(line.as_str());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct Regs(std::collections::HashMap<usize, u32>, RefCell<usize>);
    impl Bus for Regs {
        fn read(&self, addr: usize) -> u32 {
            *self.1.borrow_mut() += 1;
            self.0.get(&addr).copied().unwrap_or(0)
        }
        fn write(&self, _: usize, _: u32) {
            panic!("a measurement never writes");
        }
    }

    const BASE: usize = 0xd428_2800;

    #[test]
    fn the_stock_dumps_parse_and_the_apmu_holds_the_usb_words() {
        let apmu: Vec<_> = words(APMU).collect();
        assert_eq!(apmu.len(), 256);
        assert!(apmu.contains(&(0x05c, 0x0f33)));
        assert!(apmu.contains(&(0x110, 0x8)));
        assert!(apmu.contains(&(0x3c8, 0x0b00_8000)));
        assert!(words(MPMU).count() > 2000);
    }

    #[test]
    fn only_the_words_that_differ_are_printed_with_ours_and_the_stock() {
        let stock: std::collections::HashMap<usize, u32> =
            words(APMU).map(|(o, v)| (BASE + o, v)).collect();
        let mut ours = stock.clone();
        ours.insert(BASE + 0x05c, 0);
        ours.insert(BASE + 0x110, 0);
        let bus = Regs(ours, RefCell::new(0));
        let mut lines = Vec::new();
        report("apmu", BASE, 0x400, APMU, &bus, |l| lines.push(l.to_string()));
        assert_eq!(lines, ["socd: apmu vs stock (2 differ) 05c:0/f33 110:0/8"]);
        assert_eq!(*bus.1.borrow(), 256, "one read per stock word");
        let same = Regs(stock, RefCell::new(0));
        let mut lines = Vec::new();
        report("apmu", BASE, 0x400, APMU, &same, |l| lines.push(l.to_string()));
        assert_eq!(lines, ["socd: apmu vs stock (0 differ)"]);
    }

    /// A cold window differs everywhere: the lines stay inside the console's line, the count
    /// names them all, the words beyond the window's length are never read.
    #[test]
    fn a_cold_window_wraps_its_lines_and_stops_at_the_window() {
        let bus = Regs(std::collections::HashMap::new(), RefCell::new(0));
        let mut lines = Vec::new();
        report("mpmu", 0xd405_0000, 0x209c, MPMU, &bus, |l| lines.push(l.to_string()));
        assert!(lines.len() > 1);
        assert!(lines[0].starts_with("socd: mpmu vs stock ("));
        assert!(lines[1].starts_with("socd: mpmu vs stock (cont) "));
        assert!(lines.iter().all(|l| l.len() <= 240));
        assert_eq!(*bus.1.borrow(), words(MPMU).filter(|(o, _)| o + 4 <= 0x209c).count());
    }
}
