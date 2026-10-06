// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the USB host and its on-board hub against the board as measured (TASK-0328 U3;
//! `docs/board/measurements/2026-10-04-usb-topology/`): the host node's glue word, the hub's
//! pads, lines and settle — on the stock board writing nothing, from cold every line in order,
//! and what the planner refuses before any bus access. The fixtures (`MockBus`, `Pauses`, the
//! board's providers) are `k1.rs`'s.
//! OWNERS: @runtime

use nexus_fdt::Fdt;
use nexus_hal::Bus;
use nexus_soc::{plan, Executor, Fault, PlanError, ProviderKind, Providers, Step, PAD_OWNED};

use super::{board_providers, MockBus, Pauses, APMU_BASE, GPIO_BANK3, HUB, SOC_REJECTS, USB};

/// The host node: bus domain, the three resets, the `usbdrd30` clock, then the glue word's
/// bit the stock system runs with (`0xd4282bc8 = 0x0b008000`; bits 24..25 are status — the
/// first board cycle wrote them and read `0x08008000` back — so the mask is bit 15). From cold
/// every one is written, in order; on the board as the loader left it (the clock word 0, the
/// glue word `0x08008000`) only the clock word is.
#[test]
fn the_usb_host_plan_ends_with_the_measured_glue_bit() {
    let (fdt, providers) = board_providers();
    let usb = fdt.node_at_path(USB).unwrap();
    let plan = plan(usb, &providers).unwrap();
    let steps: Vec<Step> = plan.steps().copied().collect();
    assert_eq!(steps.len(), 8, "domain + 4 resets + 2 clocks + the glue word");
    let glue = APMU_BASE + 0x3c8;
    assert_eq!(steps[7], Step::SetWord { addr: glue, mask: 0x8000, value: 0x8000 });
    let bus = MockBus::cold();
    let report = Executor::new(&bus).execute(&plan).unwrap();
    assert_eq!((report.resets_released, report.clocks_on, report.words), (4, 2, 1));
    let usb_word = APMU_BASE + 0x05c;
    assert_eq!(
        bus.writes(),
        vec![
            (usb_word, 1 << 9),
            (usb_word, 0x600),
            (usb_word, 0xe00),
            (usb_word, 0xe01),
            (usb_word, 0xf01),
            (usb_word, 0xf03),
            (glue, 0x8000),
        ],
        "ahb, vcc, phy, axi released; the two clocks on; the glue bit"
    );
    assert_eq!(report.writes, 7);
    // The board after our loader (cycle 1, 2026-10-05): the glue word's status bits are not
    // ours and bit 15 is up already — the word is left alone, the clock word written.
    let loader_left = MockBus::cold();
    loader_left.seed(glue, 0x0800_8000);
    let report = Executor::new(&loader_left).execute(&plan).unwrap();
    assert_eq!(report.writes, 6);
    assert_eq!(loader_left.read(glue), 0x0800_8000, "status bits never written");
    // The SuperSpeed (combo) PHY node: its global reset released first — PCIe port A's, bit 8
    // of the APMU's 0x3cc, SET = held (the one APMU reset of inverted polarity) — then the
    // port's "hold PHY reset" bit (bit 30) cleared and the lane select (`0x110 = 0x8`) set.
    // The stock board (0x3cc = 0x480, 0x110 = 0x8) writes nothing.
    let ss_phy = fdt.node_at_path("/soc/storage-bus/phy@c0b10000").unwrap();
    let lane = nexus_soc::plan(ss_phy, &providers).unwrap();
    let steps: Vec<Step> = lane.steps().copied().collect();
    let pcie0 = APMU_BASE + 0x3cc;
    assert_eq!(
        steps,
        vec![
            Step::ReleaseReset { addr: pcie0, mask: 1 << 8, assert_sets: true },
            Step::SetWord { addr: pcie0, mask: 1 << 30, value: 0 },
            Step::SetWord { addr: APMU_BASE + 0x110, mask: 0x8, value: 0x8 },
        ]
    );
    let stock = MockBus::stock();
    assert_eq!(Executor::new(&stock).execute(&lane).unwrap().writes, 0, "the stock words");
    // Our loader's words (cycle 9: 0x3cc = 0x40000780 — the PHY held twice — and no lane
    // select): the reset released, the hold bit cleared, the lane selected, in that order.
    let loader_left = MockBus::cold();
    loader_left.seed(pcie0, 0x4000_0780);
    let report = Executor::new(&loader_left).execute(&lane).unwrap();
    assert_eq!((report.resets_released, report.words, report.writes), (1, 2, 3));
    assert_eq!(
        loader_left.writes(),
        vec![(pcie0, 0x4000_0680), (pcie0, 0x680), (APMU_BASE + 0x110, 0x8)]
    );
    // The USB 2.0 PHY node: its clock, then the MPMU word (stock `0x14 = 0x007d0018`).
    let usb2_phy = fdt.node_at_path("/soc/storage-bus/phy@c0a30000").unwrap();
    let mpmu = providers.get(ProviderKind::Mpmu).unwrap().base;
    let phy_plan = nexus_soc::plan(usb2_phy, &providers).unwrap();
    let steps: Vec<Step> = phy_plan.steps().copied().collect();
    assert_eq!(
        steps,
        vec![
            Step::GateOn { addr: APMU_BASE + 0x05c, mask: 1 << 8 },
            Step::SetWord { addr: mpmu + 0x14, mask: u32::MAX, value: 0x007d_0018 },
        ]
    );
    stock.seed(mpmu + 0x14, 0x007d_0018);
    assert_eq!(Executor::new(&stock).execute(&phy_plan).unwrap().writes, 0);
}

/// The hub node: its three pads on the GPIO function first, then the hub's two lines, the
/// measured 200 ms VBUS delay, then VBUS — bank 3 of the GPIO block, lines 27, 28 and 1.
#[test]
fn the_hub_plan_puts_the_pads_first_then_the_lines_the_settle_and_vbus() {
    let (fdt, providers) = board_providers();
    let hub = fdt.node_at_path(HUB).unwrap();
    let pinctrl = providers.get(ProviderKind::Pinctrl).unwrap().base;
    let steps: Vec<Step> = plan(hub, &providers).unwrap().steps().copied().collect();
    assert_eq!(
        steps,
        vec![
            Step::PadSet { addr: pinctrl + 0x1e4, mask: PAD_OWNED, value: 0xa041 & PAD_OWNED },
            Step::PadSet { addr: pinctrl + 0x23c, mask: PAD_OWNED, value: 0xa040 & PAD_OWNED },
            Step::PadSet { addr: pinctrl + 0x240, mask: PAD_OWNED, value: 0xd040 & PAD_OWNED },
            Step::GpioOut { bank: GPIO_BANK3, bit: 1 << 27, high: true },
            Step::GpioOut { bank: GPIO_BANK3, bit: 1 << 28, high: true },
            Step::Settle { ms: 200 },
            Step::GpioOut { bank: GPIO_BANK3, bit: 1 << 1, high: true },
        ]
    );
}

/// On the stock board the hub is powered already: every line is an output driven high, every
/// pad on its function — the bring-up writes nothing and only settles.
#[test]
fn the_hub_on_the_stock_board_is_already_powered_and_only_settles() {
    let (fdt, providers) = board_providers();
    let hub = fdt.node_at_path(HUB).unwrap();
    let plan = plan(hub, &providers).unwrap();
    let bus = MockBus::stock().with_stock_hub(&providers);
    let pauses = Pauses::default();
    let report = Executor::with_pause(&bus, &pauses).execute(&plan).unwrap();
    assert_eq!((report.pads, report.gpios, report.settled_ms, report.writes), (3, 3, 200, 0));
    assert_eq!(*pauses.0.borrow(), vec![200]);
    assert!(bus.writes().is_empty(), "{:x?}", bus.writes());
}

/// From cold each line is made an output and driven high through the bank's masked set
/// registers (never a read-modify-write of another writer's lines), VBUS only after the settle.
#[test]
fn the_hub_from_cold_drives_each_line_out_and_high_in_order() {
    let (fdt, providers) = board_providers();
    let hub = fdt.node_at_path(HUB).unwrap();
    let plan = plan(hub, &providers).unwrap();
    let pinctrl = providers.get(ProviderKind::Pinctrl).unwrap().base;
    let bus = MockBus::cold().with_gpio_bank3();
    let pauses = Pauses::default();
    let report = Executor::with_pause(&bus, &pauses).execute(&plan).unwrap();
    assert_eq!((report.pads, report.gpios, report.settled_ms, report.writes), (3, 3, 200, 9));
    assert_eq!(
        bus.writes(),
        vec![
            (pinctrl + 0x1e4, 0xa041),
            (pinctrl + 0x23c, 0xa040),
            (pinctrl + 0x240, 0xc040),
            (GPIO_BANK3 + 0x54, 1 << 27),
            (GPIO_BANK3 + 0x18, 1 << 27),
            (GPIO_BANK3 + 0x54, 1 << 28),
            (GPIO_BANK3 + 0x18, 1 << 28),
            (GPIO_BANK3 + 0x54, 1 << 1),
            (GPIO_BANK3 + 0x18, 1 << 1),
        ]
    );
    assert_eq!(bus.read(GPIO_BANK3), (1 << 27) | (1 << 28) | (1 << 1), "the levels read back");
    assert_eq!(*pauses.0.borrow(), vec![200]);
}

/// The bank's level word follows the pin (board cycle 5, 2026-10-05: the read right after the
/// set still showed the old level, the marker's later read the new one): the read-back reads
/// again, bounded — a level that shows within the bound is a write that took; one that never
/// shows is the fault, with the word read last.
#[test]
fn a_level_that_follows_the_pin_late_still_reads_back_within_the_bound() {
    let (fdt, providers) = board_providers();
    let hub = fdt.node_at_path(HUB).unwrap();
    let plan = plan(hub, &providers).unwrap();
    let pauses = Pauses::default();
    let late = MockBus::cold().with_gpio_bank3().with_gpio_lag(3);
    let report = Executor::with_pause(&late, &pauses).execute(&plan).unwrap();
    assert_eq!((report.gpios, report.writes), (3, 9));
    assert_eq!(late.read(GPIO_BANK3), (1 << 27) | (1 << 28) | (1 << 1));
    let never =
        MockBus::cold().with_gpio_bank3().with_gpio_lag(nexus_soc::GPIO_LEVEL_POLL_READS + 2);
    let fault = Fault::ReadBack { addr: GPIO_BANK3, value: 0 };
    assert_eq!(Executor::with_pause(&never, &pauses).execute(&plan), Err(fault));
}

/// A plan with a settle needs a pause: without one the bring-up faults at the settle — the
/// hub's lines are up, VBUS was never raised — and a line that will not read back faults too.
#[test]
fn test_reject_a_settle_without_a_pause_and_a_line_that_does_not_read_back() {
    let (fdt, providers) = board_providers();
    let hub = fdt.node_at_path(HUB).unwrap();
    let plan = plan(hub, &providers).unwrap();
    let bus = MockBus::cold().with_gpio_bank3();
    assert_eq!(Executor::new(&bus).execute(&plan), Err(Fault::NoPause));
    assert_eq!(bus.writes().len(), 7, "the pads and the hub's two lines, no VBUS");
    assert_eq!(bus.read(GPIO_BANK3) & (1 << 1), 0, "VBUS stays low");
    // A dead GPIO block (no bank semantics: a write never shows in the direction word).
    let dead = MockBus::cold();
    let pauses = Pauses::default();
    let fault = Fault::ReadBack { addr: GPIO_BANK3 + 0x0c, value: 0 };
    assert_eq!(Executor::with_pause(&dead, &pauses).execute(&plan), Err(fault));
}

/// Refused before any bus access: a bank the block does not have, a line past 32, a start-up
/// delay no settle may be, glue words from a provider without cells for them; an active-low
/// line is driven low.
#[test]
fn test_reject_hub_lines_off_the_block_and_a_settle_too_long() {
    let fdt = Fdt::new(SOC_REJECTS).unwrap();
    let providers =
        Providers::from_tree(&fdt, |n| n.reg(0).ok().flatten().map(|r| r.addr as usize));
    let refused = |path: &str| plan(fdt.node_at_path(path).unwrap(), &providers).unwrap_err();
    assert_eq!(refused("/hub-bad-bank"), PlanError::GpioUnknown(4));
    assert_eq!(refused("/hub-bad-line"), PlanError::GpioUnknown(32));
    assert_eq!(refused("/hub-long-settle"), PlanError::SettleTooLong(5000));
    assert_eq!(refused("/glue-no-cells"), PlanError::ProviderKindUnknown);
    let low = plan(fdt.node_at_path("/hub-active-low").unwrap(), &providers).unwrap();
    let steps: Vec<Step> = low.steps().copied().collect();
    assert_eq!(steps, vec![Step::GpioOut { bank: GPIO_BANK3, bit: 1 << 27, high: false }]);
    // The board's hub without its GPIO window: refused by the provider, not a wild write.
    let (board, mut without_gpio) = board_providers();
    without_gpio = {
        let mut p = Providers::new();
        for kind in [ProviderKind::Apmu, ProviderKind::Pinctrl] {
            p.set(without_gpio.get(kind).unwrap());
        }
        p
    };
    let hub = board.node_at_path(HUB).unwrap();
    assert_eq!(plan(hub, &without_gpio), Err(PlanError::ProviderUnknown(ProviderKind::Gpio)));
}
