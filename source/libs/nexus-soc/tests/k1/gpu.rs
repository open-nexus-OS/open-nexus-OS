// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The GPU's glue (TASK-0329 G1, RFC-0106 Phase 3 "the GPU set"): domain 2 by its
//! software sequence (`docs/board/measurements/2026-09-30-power-domains/` row 2 — control
//! `0x0d0`, sleep1 bit 2, sleep2 bit 3, isolation bit 1, status bit 0 of `0x0f0`; the vendor's
//! domain driver as reference for the order and the settles), `gpu_clk` gated on and set to
//! the stock rate (APMU `0xcc` stock `0x12`: mux 0 = pll1_d4 = 614.4 MHz, divider 0), the
//! reset released AFTER the clock with a microsecond between (`nexus,clock-before-reset`, the
//! open driver's manual sequence). The stock words are the goldens.
//! OWNERS: @runtime @gpu
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: this file

use nexus_hal::Bus;
use nexus_soc::class::{self, Class};
use nexus_soc::table::k1;
use nexus_soc::{bring_up, bring_up_with, plan, BringUp, BringUpError, Fault, ProviderKind, Step};

use super::{board_providers, MockBus, Pauses, APMU_BASE};

const GPU: &str = "/soc/multimedia-bus/gpu@cac00000";
const GPU_PWR_CTRL: usize = APMU_BASE + 0x0d0;
const PWR_STATUS: usize = APMU_BASE + 0x0f0;
const GPU_CLK_RES: usize = APMU_BASE + 0x0cc;

/// A cold register file whose domain 2 reports on once its control word holds the three bits
/// (the hardware's answer to the software sequence), and whose gpu_clk frequency-change bit
/// clears itself.
fn cold_gpu() -> MockBus {
    let mut bus = MockBus::cold();
    bus.sequencer = Some((GPU_PWR_CTRL, 0xe, PWR_STATUS, 1 << 0));
    bus.self_clearing.push((GPU_CLK_RES, 1 << 15));
    bus
}

#[test]
fn the_gpu_node_plans_the_domain_the_clock_first_then_the_reset_then_the_rate() {
    let (fdt, providers) = board_providers();
    let gpu = fdt.node_at_path(GPU).expect("the board tree names the GPU");
    let p = plan(gpu, &providers).unwrap();
    let steps: Vec<Step> = p.steps().copied().collect();
    let gpu_clk = nexus_soc::table::clock(ProviderKind::Apmu, k1::CLK_GPU).unwrap();
    assert_eq!(
        steps,
        vec![
            Step::DomainOnSoftware {
                id: k1::PD_GPU,
                ctrl: GPU_PWR_CTRL,
                sleep1: 1 << 2,
                sleep2: 1 << 3,
                isolation: 1 << 1,
                status: PWR_STATUS,
                on: 1 << 0,
            },
            Step::GateOn { addr: GPU_CLK_RES, mask: 1 << 4 },
            Step::SettleUs { us: 1 },
            Step::ReleaseReset { addr: GPU_CLK_RES, mask: 1 << 1, assert_sets: false },
            // 614.4 MHz = pll1_d4 (mux 0) / 1 (divider 0): the stock word's selection.
            Step::SetRate { clock: gpu_clk, window: APMU_BASE, mux: 0, div: 0 },
        ],
        "domain 2, the clock before the reset, the stock rate"
    );
    let regs: Vec<usize> = p.registers().iter().collect();
    assert_eq!(regs, vec![GPU_PWR_CTRL, PWR_STATUS, GPU_CLK_RES]);
    assert_eq!(class::of(gpu), Some(Class::Gpu));
}

#[test]
fn the_gpu_from_cold_walks_the_vendor_sequence_and_ends_at_the_stock_words() {
    let (fdt, providers) = board_providers();
    let bus = cold_gpu();
    let pauses = Pauses::default();
    let gpu = fdt.node_at_path(GPU).unwrap();
    let Ok(BringUp::Up(report)) = bring_up_with(gpu, &providers, &bus, &pauses) else {
        panic!("up from cold")
    };
    assert_eq!(
        bus.writes(),
        vec![
            (GPU_PWR_CTRL, 1 << 2),              // sleep1
            (GPU_PWR_CTRL, (1 << 2) | (1 << 3)), // sleep2
            (GPU_PWR_CTRL, 0xe),                 // isolation released: the stock word
            (GPU_CLK_RES, 1 << 4),               // gpu_clk gated on
            (GPU_CLK_RES, (1 << 4) | (1 << 1)),  // the reset released: 0x12, the stock word
                                                 // (mux 0 / divider 0 already: no rate write)
        ],
        "one write at a time, the vendor's order, the clock before the reset"
    );
    assert_eq!(*pauses.1.borrow(), vec![20, 20, 10, 1], "the settles in microseconds");
    assert!(pauses.0.borrow().is_empty(), "no millisecond settle");
    assert_eq!(
        (report.domains, report.clocks_on, report.resets_released, report.rates_set),
        (1, 1, 1, 1)
    );
    assert_eq!(report.writes, 5);
    assert_eq!(report.settled_us, 51);
    assert_eq!(bus.read(PWR_STATUS) & 1, 1, "domain 2 reports on");
    assert_eq!(bus.read(GPU_CLK_RES), 0x12, "the stock word");
    let gpu_clk = nexus_soc::table::clock(ProviderKind::Apmu, k1::CLK_GPU).unwrap();
    assert_eq!(nexus_soc::Executor::new(&bus).rate(gpu_clk, APMU_BASE), 614_400_000);
}

#[test]
fn the_gpu_on_the_stock_board_is_up_without_a_write() {
    let (fdt, providers) = board_providers();
    let bus = MockBus::stock();
    let gpu = fdt.node_at_path(GPU).unwrap();
    let pauses = Pauses::default();
    let Ok(BringUp::Up(report)) = bring_up_with(gpu, &providers, &bus, &pauses) else {
        panic!("stock up")
    };
    assert_eq!(report.writes, 0, "the stock words need no write");
    assert_eq!(*pauses.1.borrow(), vec![1], "only the reset's clock cycles");
}

#[test]
fn test_reject_a_software_domain_that_never_reports_on() {
    let (fdt, providers) = board_providers();
    let mut bus = MockBus::cold(); // no sequencer: the status never rises
    bus.self_clearing.push((GPU_CLK_RES, 1 << 15));
    let pauses = Pauses::default();
    let gpu = fdt.node_at_path(GPU).unwrap();
    assert_eq!(
        bring_up_with(gpu, &providers, &bus, &pauses).err(),
        Some(BringUpError::Fault(Fault::DomainStuck { addr: PWR_STATUS, value: 0 }))
    );
    assert_eq!(bus.read(GPU_PWR_CTRL), 0xe, "the walk was complete before the poll gave up");
    assert_eq!(bus.read(GPU_CLK_RES), 0, "nothing after the domain was touched");
}

#[test]
fn test_reject_a_software_domain_without_a_pause() {
    let (fdt, providers) = board_providers();
    let bus = cold_gpu();
    let gpu = fdt.node_at_path(GPU).unwrap();
    assert_eq!(bring_up(gpu, &providers, &bus).err(), Some(BringUpError::Fault(Fault::NoPause)));
    assert!(bus.writes().is_empty(), "no bit before the first settle can be honoured");
}
