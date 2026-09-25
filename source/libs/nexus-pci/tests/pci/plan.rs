// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the planner over a synthetic configuration space.
//! OWNERS: @runtime @drivers

use nexus_pci::config::{BAR0, CMD_MASTER, CMD_MEMORY, COMMAND};
use nexus_pci::{enable_bus_master, plan, Bar, Bdf, PlanError, Refusal};

use crate::host::{translated_host, virt_host};
use crate::mock::{io, mem32, mem64, Mock, Spec, HOST_BRIDGE, SD_HOST};

fn endpoint(bars: [u32; 6]) -> Spec {
    Spec { id: 0x1234_1af4, class: 0x0100_0000, header: 0, pin: 0, bars }
}

fn bar(pci: u64, size: u64, span: u64) -> Option<Bar> {
    Some(Bar { size, pci, cpu: pci, span, is64: false, prefetchable: false })
}

#[test]
fn qemus_sd_host_gets_a_page_of_its_own_its_line_and_memory_decoding_only() {
    let mock = Mock::with(&[(0, 0, HOST_BRIDGE), (1, 0, SD_HOST)]);
    let plan = plan(&virt_host(), &mock).unwrap();
    assert_eq!(plan.len(), 2);
    let sd = plan.of_class(0x0805).next().unwrap();
    assert_eq!(sd.bdf, Bdf { bus: 0, dev: 1, func: 0 });
    assert_eq!((sd.vendor, sd.device, sd.class), (0x1b36, 0x0007, 0x08_05_01));
    assert_eq!(sd.bars[0], bar(0x4000_0000, 256, 4096));
    assert_eq!((sd.pin, sd.irq), (1, 33));
    assert_eq!(mock.bar(1, 0, 0), 0x4000_0000);
    assert_eq!(mock.command(1, 0), CMD_MEMORY, "memory decoding on, bus mastering off");
    assert_eq!(mock.writes_to(0), 0, "the host bridge is never written");
}

#[test]
fn a_64_bit_bar_goes_to_the_64_bit_window_and_equal_spans_keep_bus_order() {
    let (low, high) = mem64(16 * 1024, true);
    let gpu = endpoint([0, mem32(4096), 0, 0, low, high]);
    let mock = Mock::with(&[(0, 0, HOST_BRIDGE), (1, 0, SD_HOST), (2, 0, gpu)]);
    let plan = plan(&virt_host(), &mock).unwrap();
    let gpu = plan.functions().find(|f| f.bdf.dev == 2).unwrap();
    let wide = Bar {
        size: 16 * 1024,
        pci: 0x4_0000_0000,
        cpu: 0x4_0000_0000,
        span: 16 * 1024,
        is64: true,
        prefetchable: true,
    };
    assert_eq!(gpu.bars[4], Some(wide));
    assert_eq!((mock.bar(2, 0, 4), mock.bar(2, 0, 5)), (0x0000_000C, 0x4), "both halves written");
    // Two 4 KiB spans in the 32-bit window: 00:01.0 first, then 00:02.0.
    assert_eq!(plan.of_class(0x0805).next().unwrap().bars[0].unwrap().pci, 0x4000_0000);
    assert_eq!(gpu.bars[1], bar(0x4000_1000, 4096, 4096));
}

#[test]
fn bars_pack_largest_first_on_whole_pages_that_hold_nothing_else() {
    let big = endpoint([mem32(1 << 20), mem32(64 << 10), mem32(256), 0, 0, 0]);
    let small = endpoint([mem32(4096), mem32(16), 0, 0, 0, 0]);
    let mock = Mock::with(&[(3, 0, big), (4, 0, small)]);
    let plan = plan(&virt_host(), &mock).unwrap();
    let placed: Vec<Bar> = plan.functions().flat_map(|f| f.bars.into_iter().flatten()).collect();
    let mut addresses: Vec<(u64, u64)> = placed.iter().map(|b| (b.pci, b.span)).collect();
    addresses.sort_unstable();
    assert_eq!(
        addresses,
        [
            (0x4000_0000, 1 << 20),
            (0x4010_0000, 64 << 10),
            (0x4011_0000, 4096),
            (0x4011_1000, 4096),
            (0x4011_2000, 4096)
        ]
    );
    for b in &placed {
        assert!(b.pci % b.span == 0 && b.span % 4096 == 0 && b.span >= b.size, "{b:?}");
    }
}

#[test]
fn a_multi_function_device_is_walked_and_a_single_function_one_is_not() {
    let multi = Spec { header: 0x80, ..endpoint([0; 6]) };
    let mock = Mock::with(&[
        (3, 0, multi),
        (3, 2, endpoint([0; 6])),
        (4, 0, endpoint([0; 6])),
        (4, 1, endpoint([0; 6])),
    ]);
    let plan = plan(&virt_host(), &mock).unwrap();
    let found: Vec<(u8, u8)> = plan.functions().map(|f| (f.bdf.dev, f.bdf.func)).collect();
    assert_eq!(found, [(3, 0), (3, 2), (4, 0)], "04.1 aliases a single-function device");
}

#[test]
fn bridges_are_recorded_and_never_programmed() {
    let bridge =
        Spec { id: 0x000c_1b36, class: 0x0604_0000, header: 1, pin: 0, bars: [mem32(4096); 6] };
    let mock = Mock::with(&[(0, 0, HOST_BRIDGE), (5, 0, bridge)]);
    let plan = plan(&virt_host(), &mock).unwrap();
    let found = plan.functions().find(|f| f.bdf.dev == 5).unwrap();
    assert!(found.bridge && found.bars.iter().all(Option::is_none));
    assert_eq!(mock.writes_to(5), 0);
}

#[test]
fn test_reject_bars_the_specification_does_not_allow() {
    let (low, _) = mem64(4096, false);
    let odd = endpoint([io(256), 0xFFFF_F0F0, mem32(4096), 0, 0, low]);
    let mock = Mock::with(&[(6, 0, odd)]);
    let plan = plan(&virt_host(), &mock).unwrap();
    let f = plan.functions().next().unwrap();
    assert_eq!(f.refused[0], Some(Refusal::Io));
    assert_eq!(f.refused[1], Some(Refusal::BadSize), "not a power of two");
    assert_eq!(f.refused[5], Some(Refusal::BadSize), "a 64-bit BAR in the last register");
    assert_eq!(f.bars[2], bar(0x4000_0000, 4096, 4096), "the rest goes on");
}

#[test]
fn test_reject_a_bar_no_window_has_room_for() {
    let (low, high) = mem64(32 << 30, false);
    let huge = endpoint([mem32(1 << 31), low, high, 0, 0, 0]);
    let mock = Mock::with(&[(7, 0, huge)]);
    let plan = plan(&virt_host(), &mock).unwrap();
    let f = plan.functions().next().unwrap();
    // 2 GiB in a 1 GiB window; 32 GiB in a 16 GiB window, and too large below 4 GiB.
    assert_eq!(f.refused[0], Some(Refusal::NoRoom));
    assert_eq!(f.refused[1], Some(Refusal::NoRoom));
    assert_eq!(mock.command(7, 0), 0, "nothing placed: decoding stays off");
}

#[test]
fn test_reject_more_functions_than_a_plan_holds() {
    let multi = Spec { header: 0x80, ..endpoint([0; 6]) };
    let mut functions = Vec::new();
    for dev in 0..5 {
        for func in 0..8 {
            functions.push((dev, func, multi));
        }
    }
    assert_eq!(plan(&virt_host(), &Mock::with(&functions)), Err(PlanError::TooManyFunctions));
}

#[test]
fn a_pin_without_a_route_gets_no_line() {
    let mock = Mock::with(&[(1, 0, SD_HOST)]);
    let plan = plan(&translated_host(), &mock).unwrap();
    let sd = plan.functions().next().unwrap();
    assert_eq!((sd.pin, sd.irq), (1, 0));
    // The translated host's window: PCI 16 MiB is CPU 0xa100_0000.
    assert_eq!(sd.bars[0].map(|b| (b.pci, b.cpu)), Some((0x100_0000, 0xa100_0000)));
}

#[test]
fn planning_again_gives_the_same_plan_and_leaves_the_same_state() {
    let (low, high) = mem64(16 * 1024, true);
    let functions =
        [(0, 0, HOST_BRIDGE), (1, 0, SD_HOST), (2, 0, endpoint([0, mem32(4096), 0, 0, low, high]))];
    let mock = Mock::with(&functions);
    let first = plan(&virt_host(), &mock).unwrap();
    let state = (mock.bar(1, 0, 0), mock.bar(2, 0, 1), mock.bar(2, 0, 4), mock.command(2, 0));
    let second = plan(&virt_host(), &mock).unwrap();
    assert_eq!(first, second);
    assert_eq!(
        state,
        (mock.bar(1, 0, 0), mock.bar(2, 0, 1), mock.bar(2, 0, 4), mock.command(2, 0))
    );
    assert_eq!(plan(&virt_host(), &Mock::with(&functions)).unwrap(), first, "a fresh bus too");
}

#[test]
fn bus_mastering_is_the_grants_alone() {
    let mock = Mock::with(&[(1, 0, SD_HOST), (2, 0, endpoint([mem32(4096), 0, 0, 0, 0, 0]))]);
    plan(&virt_host(), &mock).unwrap();
    let mastered =
        mock.writes.borrow().iter().any(|&(_, reg, v)| reg == COMMAND && v & CMD_MASTER != 0);
    assert!(!mastered, "the plan never lets a function master the bus");
    enable_bus_master(&mock, Bdf { bus: 0, dev: 1, func: 0 });
    assert_eq!(mock.command(1, 0), CMD_MEMORY | CMD_MASTER);
    assert_eq!(mock.command(2, 0), CMD_MEMORY);
    let _ = BAR0;
}
