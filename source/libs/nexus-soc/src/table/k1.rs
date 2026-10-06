// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The K1 tables for every consumer Block 1 and Block 2 bring up (SD hosts, UART0,
//! pinctrl, USB, EMAC, HDMI, display controller, GPU). PROVENANCE: register
//! offsets, field positions and reset polarity are transcribed from the mainline
//! SoC driver documentation (the clock, reset and pinctrl drivers for this SoC;
//! GPL code is reference only, no line is copied); every "stock value" is the
//! board's measured APMU state, `docs/board/measurements/2026-09-22-stock-system/
//! regmap-apmu.txt`, which the tests replay. Ids are the binding header's. The power
//! domains are transcribed from the stock system's own tree (its power controller) and
//! checked against the live APMU words (`docs/board/measurements/2026-09-30-power-domains/`).

use super::{ClockEntry, DomainEntry, DomainOn, Parent, ResetEntry};
use crate::field::Field;
use crate::provider::ProviderKind::{Apbc, Apmu};

// ---- fixed-rate PLL post-dividers (PLL1 2457.6 MHz, PLL2 3000 MHz, PLL3 3200 MHz,
// all locked per MPMU POSR on the stock system) ----
const PLL1_D3: Parent = Parent { name: "pll1_d3_819p2", hz: 819_200_000 };
const PLL1_D4: Parent = Parent { name: "pll1_d4_614p4", hz: 614_400_000 };
const PLL1_D5: Parent = Parent { name: "pll1_d5_491p52", hz: 491_520_000 };
const PLL1_D6: Parent = Parent { name: "pll1_d6_409p6", hz: 409_600_000 };
const PLL1_D8: Parent = Parent { name: "pll1_d8_307p2", hz: 307_200_000 };
const PLL1_D10: Parent = Parent { name: "pll1_d10_245p76", hz: 245_760_000 };
const PLL1_D11: Parent = Parent { name: "pll1_d11_223p4", hz: 223_418_182 };
const PLL1_D13: Parent = Parent { name: "pll1_d13_189", hz: 189_046_154 };
const PLL1_D23: Parent = Parent { name: "pll1_d23_106p8", hz: 106_852_174 };
const PLL2_D3: Parent = Parent { name: "pll2_d3", hz: 1_000_000_000 };
const PLL2_D4: Parent = Parent { name: "pll2_d4", hz: 750_000_000 };
const PLL2_D5: Parent = Parent { name: "pll2_d5", hz: 600_000_000 };
const PLL2_D8: Parent = Parent { name: "pll2_d8", hz: 375_000_000 };
const PLL3_D6: Parent = Parent { name: "pll3_d6", hz: 533_333_333 };

static SDH01_PARENTS: [Parent; 7] =
    [PLL1_D6, PLL1_D4, PLL2_D8, PLL2_D5, PLL1_D11, PLL1_D13, PLL1_D23];
static SDH2_PARENTS: [Parent; 7] =
    [PLL1_D6, PLL1_D4, PLL2_D8, PLL1_D3, PLL1_D11, PLL1_D13, PLL1_D23];
static HDMI_PARENTS: [Parent; 4] = [PLL1_D6, PLL1_D5, PLL1_D4, PLL1_D8];
static GPU_PARENTS: [Parent; 8] =
    [PLL1_D4, PLL1_D5, PLL1_D3, PLL1_D6, PLL3_D6, PLL2_D3, PLL2_D4, PLL2_D5];
static PMUA_ACLK_PARENTS: [Parent; 2] = [PLL1_D10, PLL1_D8];

// ---- APMU registers (offsets in the 0x400 window) ----
const APMU_SDH0_CLK_RES_CTRL: u16 = 0x054;
const APMU_SDH1_CLK_RES_CTRL: u16 = 0x058;
const APMU_USB_CLK_RES_CTRL: u16 = 0x05c;
const APMU_GPU_CLK_RES_CTRL: u16 = 0x0cc;
const APMU_SDH2_CLK_RES_CTRL: u16 = 0x0e0;
const APMU_HDMI_CLK_RES_CTRL: u16 = 0x1b8;
const APMU_ACLK_CLK_CTRL: u16 = 0x388;
// The PCIe port A word: its application clocks and resets, and the combo PHY's global reset
// (the SuperSpeed USB lane shares the PHY — TASK-0328 U3, board cycle 10). Stock 0x480.
const APMU_PCIE_CLK_RES_CTRL_0: u16 = 0x3cc;
const APMU_EMAC0_CLK_RES_CTRL: u16 = 0x3e4;
const APMU_EMAC1_CLK_RES_CTRL: u16 = 0x3ec;
const APMU_PWR_STATUS: u16 = 0x0f0;
const APMU_HDMI_PWR_CTRL: u16 = 0x3f4;
// ---- APBC registers ----
const APBC_UART1_CLK_RST: u16 = 0x00; // = uart0 in the binding
                                      // The GPIO block's core (24 MHz crystal) and bus gates (mainline v6.16 ccu-k1: APBC_GPIO_CLK_RST
                                      // 0x08, gpio_clk BIT1 parent vctcxo_24m, gpio_bus_clk BIT0) — TASK-0260B P3, the boot LED.
const APBC_GPIO_CLK_RST: u16 = 0x08;
const APBC_AIB_CLK_RST: u16 = 0x3c;

// ---- ids (config/board/include/dt-bindings/clock/spacemit,k1-syscon.h) ----
pub const CLK_UART0: u32 = 0;
pub const CLK_AIB: u32 = 42;
pub const CLK_UART0_BUS: u32 = 52;
pub const CLK_GPIO: u32 = 9;
pub const CLK_GPIO_BUS: u32 = 61;
pub const CLK_AIB_BUS: u32 = 94;
pub const CLK_SDH_AXI: u32 = 10;
pub const CLK_SDH0: u32 = 11;
pub const CLK_SDH1: u32 = 12;
pub const CLK_SDH2: u32 = 13;
pub const CLK_USB_P1: u32 = 14;
pub const CLK_USB_AXI: u32 = 15;
pub const CLK_USB30: u32 = 16;
pub const CLK_GPU: u32 = 22;
pub const CLK_HDMI: u32 = 26;
pub const CLK_PMUA_ACLK: u32 = 27;
pub const CLK_PCIE0_MASTER: u32 = 28;
pub const CLK_PCIE0_SLAVE: u32 = 29;
pub const CLK_PCIE0_DBI: u32 = 30;
pub const CLK_EMAC0_BUS: u32 = 37;
pub const CLK_EMAC0_PTP: u32 = 38;
pub const CLK_EMAC1_BUS: u32 = 39;
pub const CLK_EMAC1_PTP: u32 = 40;
pub const RESET_UART0: u32 = 0;
pub const RESET_AIB: u32 = 42;
pub const RESET_SDH_AXI: u32 = 2;
pub const RESET_SDH0: u32 = 3;
pub const RESET_SDH1: u32 = 4;
pub const RESET_SDH2: u32 = 5;
pub const RESET_USBP1_AXI: u32 = 6;
pub const RESET_USB_AXI: u32 = 7;
pub const RESET_USB30_AHB: u32 = 8;
pub const RESET_USB30_VCC: u32 = 9;
pub const RESET_USB30_PHY: u32 = 10;
pub const RESET_GPU: u32 = 16;
pub const RESET_HDMI: u32 = 22;
pub const RESET_PCIE0_MASTER: u32 = 23;
pub const RESET_PCIE0_SLAVE: u32 = 24;
pub const RESET_PCIE0_DBI: u32 = 25;
pub const RESET_PCIE0_GLOBAL: u32 = 26;
pub const RESET_EMAC0: u32 = 35;
pub const RESET_EMAC1: u32 = 36;
// ---- The GPIO block (TASK-0328 U3): four banks of 32 lines; the first three at 4-byte strides,
// the fourth at +0x100 (the kernel's boot LED and the loader drive bank 3 line 0 through the
// same map). Per bank: the level register, the direction register, set/clear the level, set
// the direction — the set/clear forms are masked writes, so two writers of one bank never
// clobber each other's lines. Measured on the stock system 2026-10-04 (`docs/board/
// measurements/2026-10-04-usb-topology/`): bank 3's level word 0x38184002 and direction word
// 0x981ac003 show the hub's lines 97, 123 and 124 (bits 1, 27, 28) as outputs driven high.
const GPIO_BANKS: [u16; 4] = [0x000, 0x004, 0x008, 0x100];
/// The level of every line of the bank (an output reads its driven level).
pub const GPIO_LEVEL: u16 = 0x00;
/// The direction of every line (set = output).
pub const GPIO_DIRECTION: u16 = 0x0c;
/// Set the level of the masked lines.
pub const GPIO_SET: u16 = 0x18;
/// Clear the level of the masked lines.
pub const GPIO_CLEAR: u16 = 0x24;
/// Make the masked lines outputs.
pub const GPIO_SET_DIRECTION: u16 = 0x54;

/// The offset of `bank`'s registers inside the GPIO window (`None`: no such bank).
pub const fn gpio_bank(bank: u32) -> Option<u16> {
    if (bank as usize) < GPIO_BANKS.len() {
        Some(GPIO_BANKS[bank as usize])
    } else {
        None
    }
}

// ---- power-domain ids (config/board/include/dt-bindings/power/spacemit,k1-pmu.h) ----
pub const PD_BUS: u32 = 0;
pub const PD_GPU: u32 = 2;
pub const PD_HDMI: u32 = 7;

const fn gate(
    provider: crate::provider::ProviderKind,
    id: u32,
    name: &'static str,
    reg: u16,
    gate: u32,
    fixed_parent_hz: u64,
) -> ClockEntry {
    ClockEntry {
        provider,
        id,
        name,
        reg,
        gate,
        mux: None,
        div: None,
        fc: 0,
        fc_reg: reg,
        parents: &[],
        fixed_parent_hz,
    }
}

#[allow(clippy::too_many_arguments)]
const fn mux_div_gate(
    provider: crate::provider::ProviderKind,
    id: u32,
    name: &'static str,
    reg: u16,
    div: Field,
    fc: u32,
    fc_reg: u16,
    mux: Field,
    gate: u32,
    parents: &'static [Parent],
) -> ClockEntry {
    ClockEntry {
        provider,
        id,
        name,
        reg,
        gate,
        mux: Some(mux),
        div: Some(div),
        fc,
        fc_reg,
        parents,
        fixed_parent_hz: 0,
    }
}

const BIT0: u32 = 1 << 0;
const BIT1: u32 = 1 << 1;
const BIT2: u32 = 1 << 2;
const BIT3: u32 = 1 << 3;
const BIT4: u32 = 1 << 4;
const BIT5: u32 = 1 << 5;
const BIT8: u32 = 1 << 8;
const BIT9: u32 = 1 << 9;
const BIT10: u32 = 1 << 10;
const BIT11: u32 = 1 << 11;
const BIT15: u32 = 1 << 15;
const BIT29: u32 = 1 << 29;

/// pmua_aclk on the stock system: mux 1 (pll1_d8 307.2 MHz), div 0.
const PMUA_ACLK_STOCK_HZ: u64 = 307_200_000;

/// Every clock the tables know. Stock values in comments = the measured board.
pub static CLOCKS: &[ClockEntry] = &[
    // SDH: gate BIT4, div 8..10, FC BIT11, mux 5..7; the AXI gate (all three
    // hosts) is BIT3 of the SDH0 register. Stock: SDH0 0x411b, SDH1/SDH2 0x52
    // (mux 2 = pll2_d8 375 MHz, div 0, gate on).
    gate(Apmu, CLK_SDH_AXI, "sdh_axi_aclk", APMU_SDH0_CLK_RES_CTRL, BIT3, PMUA_ACLK_STOCK_HZ),
    mux_div_gate(
        Apmu,
        CLK_SDH0,
        "sdh0_clk",
        APMU_SDH0_CLK_RES_CTRL,
        Field::new(8, 3),
        BIT11,
        APMU_SDH0_CLK_RES_CTRL,
        Field::new(5, 3),
        BIT4,
        &SDH01_PARENTS,
    ),
    mux_div_gate(
        Apmu,
        CLK_SDH1,
        "sdh1_clk",
        APMU_SDH1_CLK_RES_CTRL,
        Field::new(8, 3),
        BIT11,
        APMU_SDH1_CLK_RES_CTRL,
        Field::new(5, 3),
        BIT4,
        &SDH01_PARENTS,
    ),
    mux_div_gate(
        Apmu,
        CLK_SDH2,
        "sdh2_clk",
        APMU_SDH2_CLK_RES_CTRL,
        Field::new(8, 3),
        BIT11,
        APMU_SDH2_CLK_RES_CTRL,
        Field::new(5, 3),
        BIT4,
        &SDH2_PARENTS,
    ),
    // USB: one register, three gates. Stock 0x0f33 (all on).
    gate(Apmu, CLK_USB_AXI, "usb_axi_clk", APMU_USB_CLK_RES_CTRL, BIT1, PMUA_ACLK_STOCK_HZ),
    gate(Apmu, CLK_USB_P1, "usb_p1_aclk", APMU_USB_CLK_RES_CTRL, BIT5, PMUA_ACLK_STOCK_HZ),
    gate(Apmu, CLK_USB30, "usb30_clk", APMU_USB_CLK_RES_CTRL, BIT8, PMUA_ACLK_STOCK_HZ),
    // PCIe port A's application clocks (the combo PHY's calibration runs on them). Stock
    // 0x480: all three off once the stock PHY driver has calibrated.
    gate(Apmu, CLK_PCIE0_DBI, "pcie0_dbi_clk", APMU_PCIE_CLK_RES_CTRL_0, BIT0, PMUA_ACLK_STOCK_HZ),
    gate(
        Apmu,
        CLK_PCIE0_SLAVE,
        "pcie0_slave_clk",
        APMU_PCIE_CLK_RES_CTRL_0,
        BIT1,
        PMUA_ACLK_STOCK_HZ,
    ),
    gate(
        Apmu,
        CLK_PCIE0_MASTER,
        "pcie0_master_clk",
        APMU_PCIE_CLK_RES_CTRL_0,
        BIT2,
        PMUA_ACLK_STOCK_HZ,
    ),
    // GPU: gate BIT4, div 12..14, FC BIT15, mux 18..20. Stock 0x12.
    mux_div_gate(
        Apmu,
        CLK_GPU,
        "gpu_clk",
        APMU_GPU_CLK_RES_CTRL,
        Field::new(12, 3),
        BIT15,
        APMU_GPU_CLK_RES_CTRL,
        Field::new(18, 3),
        BIT4,
        &GPU_PARENTS,
    ),
    // HDMI: gate BIT0, div 1..4, FC BIT29, mux 5..7. Stock 0x01040321.
    mux_div_gate(
        Apmu,
        CLK_HDMI,
        "hdmi_mclk",
        APMU_HDMI_CLK_RES_CTRL,
        Field::new(1, 4),
        BIT29,
        APMU_HDMI_CLK_RES_CTRL,
        Field::new(5, 3),
        BIT0,
        &HDMI_PARENTS,
    ),
    // The APMU bus clock itself: mux bit 0, div 1..2, FC BIT4. Stock 0x1.
    ClockEntry {
        provider: Apmu,
        id: CLK_PMUA_ACLK,
        name: "pmua_aclk",
        reg: APMU_ACLK_CLK_CTRL,
        gate: 0,
        mux: Some(Field::new(0, 1)),
        div: Some(Field::new(1, 2)),
        fc: BIT4,
        fc_reg: APMU_ACLK_CLK_CTRL,
        parents: &PMUA_ACLK_PARENTS,
        fixed_parent_hz: 0,
    },
    // EMAC: bus gate BIT0, PTP gate BIT15 (from pll2_d6 500 MHz). Stock 0xa007.
    gate(Apmu, CLK_EMAC0_BUS, "emac0_bus_clk", APMU_EMAC0_CLK_RES_CTRL, BIT0, PMUA_ACLK_STOCK_HZ),
    gate(Apmu, CLK_EMAC0_PTP, "emac0_ptp_clk", APMU_EMAC0_CLK_RES_CTRL, BIT15, 500_000_000),
    gate(Apmu, CLK_EMAC1_BUS, "emac1_bus_clk", APMU_EMAC1_CLK_RES_CTRL, BIT0, PMUA_ACLK_STOCK_HZ),
    gate(Apmu, CLK_EMAC1_PTP, "emac1_ptp_clk", APMU_EMAC1_CLK_RES_CTRL, BIT15, 500_000_000),
    // The display pipeline (controller + HDMI encoder) runs on `hdmi_mclk` alone — the five
    // DSI clocks are off with the picture on (2026-09-29-display-regs), so no entry names them.
    // APBC: the console and the pad controller. Core gate BIT1 (mux 4..6),
    // bus gate BIT0. On before the kernel runs (the SPL's console).
    ClockEntry {
        provider: Apbc,
        id: CLK_UART0,
        name: "uart0_clk",
        reg: APBC_UART1_CLK_RST,
        gate: BIT1,
        mux: Some(Field::new(4, 3)),
        div: None,
        fc: 0,
        fc_reg: APBC_UART1_CLK_RST,
        parents: &[],
        fixed_parent_hz: 0,
    },
    gate(Apbc, CLK_UART0_BUS, "uart0_bus_clk", APBC_UART1_CLK_RST, BIT0, 0),
    gate(Apbc, CLK_GPIO, "gpio_clk", APBC_GPIO_CLK_RST, BIT1, 24_000_000),
    gate(Apbc, CLK_GPIO_BUS, "gpio_bus_clk", APBC_GPIO_CLK_RST, BIT0, 0),
    gate(Apbc, CLK_AIB, "aib_clk", APBC_AIB_CLK_RST, BIT1, 0),
    gate(Apbc, CLK_AIB_BUS, "aib_bus_clk", APBC_AIB_CLK_RST, BIT0, 0),
];

const fn apmu_reset(id: u32, name: &'static str, reg: u16, mask: u32) -> ResetEntry {
    ResetEntry { provider: Apmu, id, name, reg, mask, assert_sets: false }
}

const fn apbc_reset(id: u32, name: &'static str, reg: u16) -> ResetEntry {
    ResetEntry { provider: Apbc, id, name, reg, mask: BIT2, assert_sets: true }
}

/// Every reset line the tables know. APMU: the bit SET means released — except the combo
/// PHY's global reset (`pcie0_global`), whose bit SET asserts (the stock word 0x480 runs it
/// clear; our loader left it set and the PHY block read all zeros — board cycle 9); APBC:
/// bit 2 SET asserts.
pub static RESETS: &[ResetEntry] = &[
    apmu_reset(RESET_SDH_AXI, "sdh_axi", APMU_SDH0_CLK_RES_CTRL, BIT0),
    apmu_reset(RESET_SDH0, "sdh0", APMU_SDH0_CLK_RES_CTRL, BIT1),
    apmu_reset(RESET_SDH1, "sdh1", APMU_SDH1_CLK_RES_CTRL, BIT1),
    apmu_reset(RESET_SDH2, "sdh2", APMU_SDH2_CLK_RES_CTRL, BIT1),
    apmu_reset(RESET_USBP1_AXI, "usbp1_axi", APMU_USB_CLK_RES_CTRL, BIT4),
    apmu_reset(RESET_USB_AXI, "usb_axi", APMU_USB_CLK_RES_CTRL, BIT0),
    apmu_reset(RESET_USB30_AHB, "usb30_ahb", APMU_USB_CLK_RES_CTRL, BIT9),
    apmu_reset(RESET_USB30_VCC, "usb30_vcc", APMU_USB_CLK_RES_CTRL, BIT10),
    apmu_reset(RESET_USB30_PHY, "usb30_phy", APMU_USB_CLK_RES_CTRL, BIT11),
    apmu_reset(RESET_PCIE0_DBI, "pcie0_dbi", APMU_PCIE_CLK_RES_CTRL_0, BIT3),
    apmu_reset(RESET_PCIE0_SLAVE, "pcie0_slave", APMU_PCIE_CLK_RES_CTRL_0, BIT4),
    apmu_reset(RESET_PCIE0_MASTER, "pcie0_master", APMU_PCIE_CLK_RES_CTRL_0, BIT5),
    ResetEntry {
        provider: Apmu,
        id: RESET_PCIE0_GLOBAL,
        name: "pcie0_global",
        reg: APMU_PCIE_CLK_RES_CTRL_0,
        mask: BIT8,
        assert_sets: true,
    },
    apmu_reset(RESET_GPU, "gpu", APMU_GPU_CLK_RES_CTRL, BIT1),
    apmu_reset(RESET_HDMI, "hdmi", APMU_HDMI_CLK_RES_CTRL, BIT9),
    apmu_reset(RESET_EMAC0, "emac0", APMU_EMAC0_CLK_RES_CTRL, BIT1),
    apmu_reset(RESET_EMAC1, "emac1", APMU_EMAC1_CLK_RES_CTRL, BIT1),
    apbc_reset(RESET_UART0, "uart0", APBC_UART1_CLK_RST),
    apbc_reset(RESET_AIB, "aib", APBC_AIB_CLK_RST),
];

/// The register offset of pad `pin` (the binding's pin number — the GPIO number) inside the
/// pinctrl window: the pads are not laid out in pin order. Measured 2026-10-03 on the stock system
/// (`docs/board/measurements/2026-10-03-first-light/`): its GPIO controller's `gpio-ranges` (GPIO
/// 49 → index 50, 90 → 127, 96 → 120, 110 → 116, 111 → 131, 123 → 143) and its live pad
/// registers through the pinctrl debugfs (uart0 68/69 at 0x114/0x118, mmc1 104/105 at
/// 0x1b8/0x1bc, the HDMI encoder's 86..89 at 0x1ec..0x1f8, `sys-led` GPIO 96 at 0x1e0). Pins
/// 98..103, which the stock layout orders differently, stay unmapped until measured.
pub const fn pad_offset(pin: u32) -> Option<u16> {
    let index = match pin {
        0..=85 => pin + 1,
        86..=92 => pin + 37,
        93..=97 => pin + 24,
        104..=110 => pin + 6,
        111..=127 => pin + 20,
        _ => return None,
    };
    Some((index * 4) as u16)
}

/// Every power domain the tables know. HDMI (the display controller and the encoder) is
/// hardware-sequenced: mode bit 4 of its control word hands it to the power sequencer, a
/// rising request bit 0 asks for power, bit 15 of the APMU power status word reports it up.
/// Stock, with the desktop on HDMI: control `0x3f4 = 0x11`, status `0x0f0 = 0x8009`. The
/// status word's offset is the one APMU word whose bits match all seven switchable domains'
/// states (the stock tree names the bits, not the register). The GPU and VPU domains are
/// software-sequenced (isolation and two sleep bits) and stay unlisted until a consumer
/// measures them.
pub static DOMAINS: &[DomainEntry] = &[
    DomainEntry { provider: Apmu, id: PD_BUS, name: "bus", on: DomainOn::Always },
    DomainEntry {
        provider: Apmu,
        id: PD_HDMI,
        name: "hdmi",
        on: DomainOn::Sequenced {
            ctrl: APMU_HDMI_PWR_CTRL,
            mode: BIT4,
            request: BIT0,
            status: APMU_PWR_STATUS,
            on: BIT15,
        },
    },
];
