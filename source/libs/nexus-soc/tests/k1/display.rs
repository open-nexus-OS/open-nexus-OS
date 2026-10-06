// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the display set (TASK-0245B P3) against the board as measured — the K1 pad map and
//! the encoder's group against the live pad words, domain 7's protocol, `hdmi_reset`, `hmclk`
//! at its demanded rate, the rates read back from the stock registers, and what the executor
//! refuses (a domain that never reports on, a stuck frequency-change bit, a write that does not
//! read back, an id the tables do not know). The fixtures (`MockBus`, the board's providers)
//! are `k1.rs`'s.
//! OWNERS: @runtime

use nexus_hal::Bus;
use nexus_soc::table::k1;
use nexus_soc::{
    bring_up, plan, BringUp, Executor, Fault, PlanError, ProviderKind, Providers, Step,
};

use super::{board_providers, DeadBus, MockBus, APMU_BASE};

// ---- Pads (TASK-0245B P3): the K1 pad map and the encoder's group, against the live words ----

#[test]
fn the_k1_pad_map_names_the_stock_registers() {
    // Pin (the GPIO number) → register offset, read live on 2026-10-03 through the stock
    // system's pinctrl debugfs: uart0, mmc1, the encoder's four, `sys-led`.
    for (pin, offset) in [
        (68, 0x114),
        (69, 0x118),
        (104, 0x1b8),
        (105, 0x1bc),
        (86, 0x1ec),
        (87, 0x1f0),
        (88, 0x1f4),
        (89, 0x1f8),
        (96, 0x1e0),
    ] {
        assert_eq!(k1::pad_offset(pin), Some(offset), "pin {pin}");
    }
    // The stock GPIO controller's `gpio-ranges`: GPIO 49 → index 50, 90 → 127, 110 → 116,
    // 111 → 131, 123 → 143.
    for (gpio, index) in [(49u32, 50u16), (90, 127), (110, 116), (111, 131), (123, 143)] {
        assert_eq!(k1::pad_offset(gpio), Some(index * 4), "gpio {gpio}");
    }
}

#[test]
fn the_encoder_pads_come_up_with_the_stock_function_and_pull() {
    let (fdt, providers) = board_providers();
    let pinctrl = providers.get(ProviderKind::Pinctrl).unwrap().base;
    let hdmi = fdt.node_at_path(HDMI).unwrap();
    // A cold register file whose drive fields hold something else: only the owned fields move.
    let bus = cold_with_sequencer();
    for off in [0x1ecusize, 0x1f0, 0x1f4, 0x1f8] {
        bus.regs.borrow_mut().insert(pinctrl + off, 2 << 10);
    }
    let Ok(BringUp::Up(report)) = bring_up(hdmi, &providers, &bus) else { panic!("hdmi up") };
    assert_eq!(report.pads, 4);
    // The live words (2026-10-03): 0xd041 on the DDC pair, 0xb041 on the status pair.
    for (off, live) in [(0x1ecusize, 0xd041u32), (0x1f0, 0xd041), (0x1f4, 0xb041), (0x1f8, 0xb041)]
    {
        let word = bus.read(pinctrl + off);
        assert_eq!(word & nexus_soc::PAD_OWNED, live & nexus_soc::PAD_OWNED, "pad 0x{off:x}");
        assert_eq!(word & !nexus_soc::PAD_OWNED, 2 << 10, "pad 0x{off:x}: drive left as found");
    }
    // From the stock words themselves nothing is written.
    let stock = cold_with_sequencer();
    for (off, live) in [(0x1ecusize, 0xd041u32), (0x1f0, 0xd041), (0x1f4, 0xb041), (0x1f8, 0xb041)]
    {
        stock.regs.borrow_mut().insert(pinctrl + off, live);
    }
    let p = plan(hdmi, &providers).unwrap();
    let before = stock.writes().len();
    let report = Executor::new(&stock).execute(&p).unwrap();
    let pad_writes = stock.writes()[before..]
        .iter()
        .filter(|(a, _)| (pinctrl..pinctrl + 0x1000).contains(a))
        .count();
    assert_eq!((report.pads, pad_writes), (4, 0), "the stock pads need no write");
}

#[test]
fn test_reject_a_pad_the_map_does_not_place() {
    for pin in [98u32, 101, 103, 128, u32::MAX] {
        assert_eq!(k1::pad_offset(pin), None, "pin {pin} is not measured");
    }
}

// ---- The display set (TASK-0245B P3): domain 7, hdmi_reset, hmclk at its demanded rate ----

const DPU: &str = "/soc/multimedia-bus/display@c0440000";
const HDMI: &str = "/soc/hdmi@c0400500";
const HDMI_PWR_CTRL: usize = APMU_BASE + 0x3f4;
const PWR_STATUS: usize = APMU_BASE + 0x0f0;
const HDMI_CLK_RES: usize = APMU_BASE + 0x1b8;

/// A cold register file whose power sequencer brings domain 7 up once its control word
/// holds the mode bit and the request, and whose hmclk frequency-change bit clears itself.
fn cold_with_sequencer() -> MockBus {
    let mut bus = MockBus::cold();
    bus.sequencer = Some((HDMI_PWR_CTRL, (1 << 4) | (1 << 0), PWR_STATUS, 1 << 15));
    bus.self_clearing.push((HDMI_CLK_RES, 1 << 29));
    bus
}

#[test]
fn the_display_pipeline_names_what_the_stock_tree_names() {
    let (fdt, providers) = board_providers();
    for path in [DPU, HDMI] {
        let p = plan(fdt.node_at_path(path).unwrap(), &providers).unwrap();
        let steps: Vec<Step> = p.steps().copied().take(4).collect();
        let hmclk = nexus_soc::table::clock(ProviderKind::Apmu, k1::CLK_HDMI).unwrap();
        assert_eq!(
            steps,
            vec![
                Step::DomainOn {
                    id: k1::PD_HDMI,
                    ctrl: HDMI_PWR_CTRL,
                    mode: 1 << 4,
                    request: 1 << 0,
                    status: PWR_STATUS,
                    on: 1 << 15
                },
                Step::ReleaseReset { addr: HDMI_CLK_RES, mask: 1 << 9, assert_sets: false },
                Step::GateOn { addr: HDMI_CLK_RES, mask: 1 << 0 },
                // 491.52 MHz = pll1_d5 (mux 1) / 1 (divider 0).
                Step::SetRate { clock: hmclk, window: APMU_BASE, mux: 1, div: 0 },
            ],
            "{path}: domain 7, hdmi_reset, hmclk — none of the five DSI clocks"
        );
        let regs: Vec<usize> = p.registers().iter().take(3).collect();
        assert_eq!(regs, vec![HDMI_PWR_CTRL, PWR_STATUS, HDMI_CLK_RES], "each register once");
    }
    // The controller names nothing more; the encoder names its four pads besides: function 1,
    // the DDC pair pulled up, the status pair pulled down (the stock words' owned fields).
    assert_eq!(plan(fdt.node_at_path(DPU).unwrap(), &providers).unwrap().len(), 4);
    let pinctrl = providers.get(ProviderKind::Pinctrl).unwrap().base;
    let p = plan(fdt.node_at_path(HDMI).unwrap(), &providers).unwrap();
    let pads: Vec<Step> = p.steps().copied().skip(4).collect();
    let pad = |off: usize, live: u32| Step::PadSet {
        addr: pinctrl + off,
        mask: nexus_soc::PAD_OWNED,
        value: live & nexus_soc::PAD_OWNED,
    };
    assert_eq!(
        pads,
        vec![pad(0x1ec, 0xd041), pad(0x1f0, 0xd041), pad(0x1f4, 0xb041), pad(0x1f8, 0xb041)]
    );
}

/// The stock board's register file with the encoder's four pads as read live (2026-10-03).
fn stock_with_encoder_pads(providers: &Providers) -> MockBus {
    let bus = MockBus::stock();
    let pinctrl = providers.get(ProviderKind::Pinctrl).unwrap().base;
    for (off, live) in [(0x1ecusize, 0xd041u32), (0x1f0, 0xd041), (0x1f4, 0xb041), (0x1f8, 0xb041)]
    {
        bus.regs.borrow_mut().insert(pinctrl + off, live);
    }
    bus
}

#[test]
fn the_display_pipeline_on_the_stock_board_is_up_without_a_write() {
    let (fdt, providers) = board_providers();
    let bus = stock_with_encoder_pads(&providers);
    for path in [DPU, HDMI] {
        let Ok(BringUp::Up(report)) = bring_up(fdt.node_at_path(path).unwrap(), &providers, &bus)
        else {
            panic!("{path} is up on the stock board")
        };
        let counts = (report.domains, report.resets_released, report.clocks_on, report.rates_set);
        assert_eq!(counts, (1, 1, 1, 1));
        assert_eq!(report.writes, 0, "{path}: the stock desktop is on HDMI: {:?}", bus.writes());
    }
    let hmclk = nexus_soc::table::clock(ProviderKind::Apmu, k1::CLK_HDMI).unwrap();
    assert_eq!(Executor::new(&bus).rate(hmclk, APMU_BASE), 491_520_000);
}

#[test]
fn the_display_pipeline_from_cold_raises_the_domain_request_then_sets_the_rate() {
    let (fdt, providers) = board_providers();
    let bus = cold_with_sequencer();
    let dpu = fdt.node_at_path(DPU).unwrap();
    let Ok(BringUp::Up(report)) = bring_up(dpu, &providers, &bus) else { panic!("up from cold") };
    assert_eq!(
        bus.writes(),
        vec![
            (HDMI_PWR_CTRL, 1 << 4),              // the sequencer's mode, the request low
            (HDMI_PWR_CTRL, (1 << 4) | (1 << 0)), // the request raised: 0x11, the stock word
            (HDMI_CLK_RES, 1 << 9),               // hdmi_reset released
            (HDMI_CLK_RES, (1 << 9) | (1 << 0)),  // hmclk gated on
            (HDMI_CLK_RES, 0x221),                // mux 1 = pll1_d5, divider 0
            (HDMI_CLK_RES, 0x221 | (1 << 29)),    // the frequency change, cleared by the SoC
        ],
        "one write at a time, in RFC-0106 order"
    );
    assert_eq!(report.writes, 6);
    assert_eq!(bus.read(PWR_STATUS) & (1 << 15), 1 << 15, "domain 7 reports on");
    let hmclk = nexus_soc::table::clock(ProviderKind::Apmu, k1::CLK_HDMI).unwrap();
    assert_eq!(Executor::new(&bus).rate(hmclk, APMU_BASE), 491_520_000);
    // The encoder shares all three: its bring-up after the controller's writes only its pads.
    let hdmi = fdt.node_at_path(HDMI).unwrap();
    let before = bus.writes().len();
    let Ok(BringUp::Up(again)) = bring_up(hdmi, &providers, &bus) else { panic!("hdmi up") };
    let pinctrl = providers.get(ProviderKind::Pinctrl).unwrap().base;
    let writes = bus.writes();
    assert_eq!((again.writes, again.pads), (4, 4));
    assert!(writes[before..].iter().all(|(a, _)| (pinctrl..pinctrl + 0x1000).contains(a)));
}

#[test]
fn a_domain_left_requested_but_off_gets_a_fresh_rising_request() {
    let (fdt, providers) = board_providers();
    let bus = cold_with_sequencer();
    // The control word already reads 0x11 while the status says off (a request that never
    // landed): the executor drops the request and raises it again.
    bus.regs.borrow_mut().insert(HDMI_PWR_CTRL, 0x11);
    let p = plan(fdt.node_at_path(DPU).unwrap(), &providers).unwrap();
    Executor::new(&bus).execute(&p).unwrap();
    assert_eq!(bus.writes()[..2], [(HDMI_PWR_CTRL, 0x10), (HDMI_PWR_CTRL, 0x11)]);
}

#[test]
fn test_reject_a_domain_that_never_reports_on() {
    let (fdt, providers) = board_providers();
    let mut bus = cold_with_sequencer();
    bus.sequencer = None; // the request lands nowhere
    let p = plan(fdt.node_at_path(DPU).unwrap(), &providers).unwrap();
    let err = Executor::new(&bus).execute(&p).unwrap_err();
    assert_eq!(err, Fault::DomainStuck { addr: PWR_STATUS, value: 0 });
    assert_eq!((err.step(), err.register()), ("domain", Some((PWR_STATUS, 0))));
    assert_eq!(bus.writes().len(), 2, "no reset or clock touched in a domain that is off");
}

#[test]
fn test_reject_a_domain_without_a_measured_protocol() {
    let (fdt, providers) = board_providers();
    let gpu = fdt.node_at_path("/soc/multimedia-bus/gpu@cac00000").unwrap();
    // The GPU's domain is software-sequenced and unmeasured: refused before any bus access.
    assert_eq!(plan(gpu, &providers), Err(PlanError::DomainUnsupported(k1::PD_GPU)));
}

#[test]
fn test_reject_a_rate_no_parent_makes_exactly() {
    let hmclk = nexus_soc::table::clock(ProviderKind::Apmu, k1::CLK_HDMI).unwrap();
    assert_eq!(hmclk.select(491_520_000), Some((1, 0)), "pll1_d5 / 1");
    assert_eq!(hmclk.select(245_760_000), Some((1, 1)), "pll1_d5 / 2 — no rounding elsewhere");
    assert_eq!(hmclk.select(500_000_000), None, "met exactly or refused, never rounded");
    let uart = nexus_soc::table::clock(ProviderKind::Apbc, k1::CLK_UART0).unwrap();
    assert_eq!(uart.select(14_745_600), None, "a clock without a divider sets no rate");
}

#[test]
fn rates_read_back_from_the_stock_registers_match_the_clock_tree() {
    let bus = MockBus::stock();
    let ex = Executor::new(&bus);
    let sdh2 = nexus_soc::table::clock(ProviderKind::Apmu, k1::CLK_SDH2).unwrap();
    assert_eq!(ex.rate(sdh2, APMU_BASE), 375_000_000, "sdh2_clk from pll2_d8, div 0");
    let sdh1 = nexus_soc::table::clock(ProviderKind::Apmu, k1::CLK_SDH1).unwrap();
    assert_eq!(ex.rate(sdh1, APMU_BASE), 375_000_000);
    let aclk = nexus_soc::table::clock(ProviderKind::Apmu, k1::CLK_PMUA_ACLK).unwrap();
    assert_eq!(ex.rate(aclk, APMU_BASE), 307_200_000, "pmua_aclk: mux 1 = pll1_d8");
    let gpu = nexus_soc::table::clock(ProviderKind::Apmu, k1::CLK_GPU).unwrap();
    assert_eq!(ex.rate(gpu, APMU_BASE), 614_400_000, "gpu_clk: mux 0 = pll1_d4, div 0");
}

#[test]
fn a_mux_change_triggers_the_frequency_change_bit_and_reads_back() {
    let mut bus = MockBus::stock();
    bus.self_clearing.push((APMU_BASE + 0x0e0, 1 << 11));
    let ex = Executor::new(&bus);
    let sdh2 = nexus_soc::table::clock(ProviderKind::Apmu, k1::CLK_SDH2).unwrap();
    // pll1_d4 614.4 MHz / 2
    ex.set_mux_div(sdh2, APMU_BASE, 1, 1).unwrap();
    assert_eq!(ex.rate(sdh2, APMU_BASE), 307_200_000);
    let writes = bus.writes();
    assert_eq!(writes.len(), 2, "fields, then the FC bit");
    assert_eq!(writes[1].1 & (1 << 11), 1 << 11);
    assert_eq!(ex.set_mux_div(sdh2, APMU_BASE, 8, 0), Err(Fault::Range));
}

#[test]
fn test_reject_stuck_frequency_change_bit() {
    let mut bus = MockBus::stock();
    bus.stuck_fc = Some((APMU_BASE + 0x0e0, 1 << 11));
    let ex = Executor::new(&bus);
    let sdh2 = nexus_soc::table::clock(ProviderKind::Apmu, k1::CLK_SDH2).unwrap();
    assert!(matches!(ex.set_mux_div(sdh2, APMU_BASE, 1, 1), Err(Fault::FcStuck { .. })));
}

#[test]
fn test_reject_a_write_that_does_not_read_back() {
    let (fdt, providers) = board_providers();
    let emmc = fdt.node_at_path("/soc/storage-bus/mmc@d4281000").unwrap();
    let plan = plan(emmc, &providers).unwrap();
    let err = Executor::new(&DeadBus).execute(&plan).unwrap_err();
    assert_eq!(err, Fault::ReadBack { addr: APMU_BASE + 0x054, value: 0 });
}

#[test]
fn test_reject_an_id_the_tables_do_not_know() {
    let (fdt, providers) = board_providers();
    // A pad group binds no clocks; a node with clocks of an unknown provider kind
    // is refused by kind, an unknown id by id — both before any bus access.
    let sd = fdt.node_at_path("/soc/storage-bus/mmc@d4280000").unwrap();
    let p = plan(sd, &providers).unwrap();
    assert!(p.steps().any(|s| matches!(s, Step::GateOn { .. })));
    let mut no_apmu = Providers::new();
    no_apmu.set(providers.get(ProviderKind::Apbc).unwrap());
    assert_eq!(plan(sd, &no_apmu), Err(PlanError::ProviderUnknown(ProviderKind::Apmu)));
}
